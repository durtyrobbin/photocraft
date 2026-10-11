//! Studio tool preferences use the existing settings service; paint never enters document data.
use crate::PhotocraftApp;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
#[derive(Clone)]
struct Cache {
    revision: u64,
    config: Arc<Config>,
}
struct Config {
    palette: crate::artist_palette::State,
    knife: crate::knife_ui::State,
    splatter: crate::splatter_ui::State,
    tip_brush: photocraft_engine::BrushSettings,
    tip_controls: Value,
}
fn tip_controls(app: &PhotocraftApp) -> Value {
    let t = &app.ui.tip_designer;
    json!([t.open, t.version, t.resolution, t.radius, t.density, t.erase, t.name])
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    version: u32,
    palette: crate::artist_palette::State,
    knife: crate::knife_ui::State,
    splatter: crate::splatter_ui::State,
    tip: crate::tip_designer::State,
    paint: photocraft_engine::palette_cmds::Snapshot,
}
pub fn persist(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if ctx.input(|i| i.pointer.any_down()) {
        return;
    }
    let revision = app.session.tools.artist_palette.revision;
    if app.session.prefs().painting_studio.is_null()
        && revision == 0
        && !app.ui.tip_designer.open
        && !matches!(app.ui.tool, crate::state::Tool::MixerBrush | crate::state::Tool::Splatter | crate::state::Tool::PaletteKnife)
    {
        return;
    }
    let id = egui::Id::new("studio-persistence");
    let controls = tip_controls(app);
    if ctx.data_mut(|d| d.get_temp::<Cache>(id)).is_some_and(|cache| {
        cache.revision == revision
            && cache.config.palette == app.ui.artist_palette
            && cache.config.knife == app.ui.knife
            && cache.config.splatter == app.ui.splatter
            && cache.config.tip_brush == app.ui.tip_designer.brush
            && cache.config.tip_controls == controls
    }) {
        return;
    }
    let cache = Cache {
        revision,
        config: Arc::new(Config {
            palette: app.ui.artist_palette.clone(),
            knife: app.ui.knife.clone(),
            splatter: app.ui.splatter.clone(),
            tip_brush: app.ui.tip_designer.brush.clone(),
            tip_controls: controls,
        }),
    };
    let saved = Saved {
        version: 1,
        palette: app.ui.artist_palette.clone(),
        knife: app.ui.knife.clone(),
        splatter: app.ui.splatter.clone(),
        tip: app.ui.tip_designer.clone(),
        paint: app.session.tools.artist_palette.snapshot(),
    };
    if let Ok(value) = serde_json::to_value(saved)
        && value != app.session.prefs().painting_studio
    {
        app.session.prefs.edit(|p| p.painting_studio = value);
    }
    ctx.data_mut(|d| d.insert_temp(id, cache));
}
fn decode(app: &PhotocraftApp, value: &Value) -> Result<Saved, String> {
    photocraft_engine::palette_cmds::parse_snapshot(value.get("paint").ok_or("saved palette has no paint")?).map_err(|e| e.to_string())?;
    if let Some(settings) = value.get("splatter").and_then(|s| s.get("settings")) {
        photocraft_engine::splatter_cmds::parse_settings(settings).map_err(|e| e.to_string())?;
    }
    let saved = Saved::deserialize(value).map_err(|e| e.to_string())?;
    if saved.version != 1
        || saved.tip.version != 1
        || !(32..=512).contains(&saved.tip.resolution)
        || saved.tip.brush.tip.bitmap().is_some_and(|t| !t.is_valid() || t.width > 512 || t.height > 512)
    {
        return Err("unsupported studio version or tip size".into());
    }
    saved.palette.validate()?;
    saved.knife.settings.validate()?;
    saved.splatter.settings.validate()?;
    photocraft_engine::brush_cmds::resolve_brush(&app.session, &json!({"brush":saved.tip.brush}), "tools.setBrush").map_err(|e| e.to_string())?;
    Ok(saved)
}
pub fn restore(app: &mut PhotocraftApp) {
    let value = app.session.prefs().painting_studio.clone();
    if value.is_null() {
        return;
    }
    match decode(app, &value) {
        Ok(saved) => match app.session.execute("palette.restore", json!(saved.paint)) {
            Ok(_) => {
                app.ui.artist_palette = saved.palette;
                app.ui.knife = saved.knife;
                app.ui.splatter = saved.splatter;
                app.ui.tip_designer = saved.tip;
            }
            Err(e) => {
                app.ui.status = e.to_string();
                app.ui.status_error = true;
            }
        },
        Err(e) => {
            app.ui.status = format!("Saved painting studio settings were not restored: {e}");
            app.ui.status_error = true;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn studio_restart_round_trip_preserves_paint_and_placement() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.ui.artist_palette.position = [300.0, 120.0];
        app.ui.artist_palette.width = 520.0;
        app.ui.splatter.settings.count = 321;
        app.ui.knife.settings.width = 87.0;
        app.run("palette.deposit", json!({"points":[[120,100]],"color":[1,0.2,0.1,1]})).unwrap();
        let ctx = egui::Context::default();
        ctx.begin_pass(Default::default());
        persist(&mut app, &ctx);
        let _ = ctx.end_pass();
        let text = app.session.prefs_to_json();
        let mut session = photocraft_engine::Session::new();
        session.load_prefs_json(&text).unwrap();
        let next = PhotocraftApp::new(session, Default::default());
        assert_eq!(next.ui.artist_palette.position, [300.0, 120.0]);
        assert_eq!(next.ui.artist_palette.width, 520.0);
        assert_eq!(next.ui.splatter.settings.count, 321);
        assert_eq!(next.ui.knife.settings.width, 87.0);
        assert_eq!(next.session.tools.artist_palette.paint, app.session.tools.artist_palette.paint);
    }
}
