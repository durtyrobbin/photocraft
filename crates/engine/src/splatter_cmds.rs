//! One bounded, deterministic flick is one document transaction on the selected paint target.
use crate::{EngineError, Result, Session, commands::CommandSpec};
use photocraft_paint::splatter::Settings;
use serde_json::{Value, json};

fn bad(message: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: "paint.splatter".into(), msg: message.into() }
}

pub fn parse_settings(raw: &Value) -> Result<Settings> {
    // Reject oversized bitmap representations before GrayTile's decoder allocates.
    if let Some(mask) = raw.get("distribution").filter(|v| !v.is_null()) {
        for key in ["width", "height"] {
            if mask.get(key).and_then(Value::as_u64).is_none_or(|n| n == 0 || n > 512) {
                return Err(bad("distribution dimensions must be 1..512"));
            }
        }
        if let Some(data) = mask.get("data") {
            let bounded = match data {
                Value::String(v) => v.len() <= 699_052,
                Value::Array(v) => v.len() <= 262_144,
                _ => false,
            };
            if !bounded {
                return Err(bad("distribution data exceeds the supported size"));
            }
        }
    }
    let settings: Settings = serde_json::from_value(raw.clone()).map_err(|e| bad(e.to_string()))?;
    settings.validate().map_err(bad)?;
    Ok(settings)
}

pub fn run(s: &mut Session, p: &Value) -> Result<Value> {
    if !p.is_object() {
        return Err(bad("expected a splatter request object"));
    }
    if p.get("layer").is_some_and(|v| v.as_u64().is_none()) {
        return Err(bad("layer must be an unsigned integer"));
    }
    if let Some(target) = p.get("target") {
        let valid = target.as_str().is_some_and(|v| matches!(v, "pixels" | "mask" | "quickMask"))
            || target.as_object().is_some_and(|o| o.len() == 1 && o.get("channel").and_then(Value::as_u64).is_some_and(|v| usize::try_from(v).is_ok()));
        if !valid {
            return Err(bad("invalid paint target"));
        }
    }
    let settings = parse_settings(p.get("settings").unwrap_or(&json!({})))?;
    let center: [f64; 2] = serde_json::from_value(p.get("center").cloned().ok_or_else(|| bad("center is required"))?).map_err(|e| bad(e.to_string()))?;
    let pressure = match p.get("pressure") {
        None => 1.0,
        Some(v) => v.as_f64().ok_or_else(|| bad("pressure must be a number"))? as f32,
    };
    // Validate all geometry even for an empty/transparent flick, before opening history.
    photocraft_paint::splatter::droplets(&settings, center, pressure).map_err(bad)?;
    let color: [f32; 4] = match p.get("color") {
        None => s.tools.foreground,
        Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad(e.to_string()))?,
    };
    let id = if crate::channel_cmds::is_channel_target(p) { None } else { Some(crate::brush_cmds::layer_id(s, p)?) };
    let cmyk = crate::brush_cmds::doc_cmyk_space(s);
    let damage = photocraft_color::convert::with_cmyk_space(cmyk.as_ref(), || {
        s.edit("Flick / Splatter", |doc, _| {
            let selection = doc.selection.clone();
            let (surface, lock) = crate::channel_cmds::target_surface(doc, id, p)?;
            photocraft_paint::splatter::apply(surface, &settings, center, pressure, color, selection.as_ref(), lock).map_err(bad)
        })
    })?;
    Ok(crate::brush_cmds::damage_json(s, damage))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "paint.splatter",
        label: "Flick / Splatter",
        menu: &[],
        shortcut: None,
        params: r#"{"center":[x,y],"pressure":0..1=1,"color":[r,g,b,a]?=foreground,"settings":{"version":1,"count":0..2048,"minSize":0.5..128,"maxSize":0.5..128,"distance":0..1024,"spread":0..360,"angle":degrees,"seed":u64,"sizeVariation":0..1,"directionalBias":0..1,"density":0..1,"clustering":0..1,"opacityVariation":0..1,"colorVariation":0..1,"paintLoad":0..1,"edgeBreakup":0..1,"secondary":0..1,"distribution":GrayTile?},"layer":id?,"target":"pixels"|"mask"|"quickMask"|{"channel":i}; maximum 8,388,608 pixel visits"#,
        enabled: crate::commands::has_paintable,
        run,
        journal: true,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splatter_masks_channels_selections_and_locks_follow_existing_targets() {
        use photocraft_geom::Rect;
        let mut s = Session::new();
        s.execute("file.new", json!({"width":96,"height":96})).unwrap();
        s.execute("layer.new.layer", json!({})).unwrap();
        let id = s.active().unwrap().active_layer.unwrap();
        let mut p = json!({"center":[48,48],"color":[1,1,1,1],"settings":{"distance":24,"spread":360,"seed":87}});
        s.edit("fixture", |doc, _| {
            doc.layer_mut(id).unwrap().mask = Some(photocraft_doc::LayerMask::hide_all());
            Ok(())
        })
        .unwrap();
        s.execute("select.rect", json!({"x":0,"y":0,"width":48,"height":96})).unwrap();
        p["target"] = json!("mask");
        s.execute("paint.splatter", p.clone()).unwrap();
        let layer = s.active().unwrap().doc.layer(id).unwrap();
        assert!(layer.surface().unwrap().read_region(Rect::new(0, 0, 96, 96)).iter().all(|v| *v == 0.0));
        let mask = &layer.mask.as_ref().unwrap().surface;
        assert!(mask.read_region(Rect::new(0, 0, 48, 96)).iter().any(|v| *v > 0.0));
        assert!(mask.read_region(Rect::new(48, 0, 96, 96)).iter().all(|v| *v == 0.0));
        s.execute("channel.new", json!({})).unwrap();
        p.as_object_mut().unwrap().remove("target");
        let layers = s.active().unwrap().doc.layers.clone();
        s.execute("paint.splatter", p.clone()).unwrap();
        assert_eq!(s.active().unwrap().doc.layers, layers);
        assert!(s.active().unwrap().doc.channels[0].surface.read_region(Rect::new(0, 0, 48, 96)).iter().any(|v| *v > 0.0));
        p["target"] = json!("pixels");
        s.edit("lock", |doc, _| {
            doc.layer_mut(id).unwrap().locks.pixels = true;
            Ok(())
        })
        .unwrap();
        assert!(s.execute("paint.splatter", p.clone()).is_err());
        s.edit("hide", |doc, _| {
            let l = doc.layer_mut(id).unwrap();
            l.locks.pixels = false;
            l.visible = false;
            Ok(())
        })
        .unwrap();
        assert!(s.execute("paint.splatter", p).is_err());
    }
    #[test]
    fn splatter_transaction_replays_undoes_and_preserves_source() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width":192,"height":192,"background":"#808080"})).unwrap();
        s.execute("layer.new.layer", json!({"name":"Splatter"})).unwrap();
        let before = s.active().unwrap().doc.clone();
        let id = s.active().unwrap().active_layer.unwrap();
        let p = json!({"center":[64,96],"color":[0.1,0.5,0.9,1.0],"settings":{"seed":87,"distance":64}});
        s.execute("paint.splatter", p.clone()).unwrap();
        let after = s.active().unwrap().doc.clone();
        assert_ne!(before.layer(id).unwrap().surface(), after.layer(id).unwrap().surface());
        for old in &before.layers {
            if old.id != id {
                assert_eq!(old.surface(), after.layer(old.id).unwrap().surface());
            }
        }
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(s.active().unwrap().doc.layers, before.layers);
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(s.active().unwrap().doc.layers, after.layers);
        s.execute("edit.undo", json!({})).unwrap();
        s.execute("paint.splatter", p).unwrap();
        assert_eq!(s.active().unwrap().doc.layers, after.layers);
    }
    #[test]
    fn splatter_refuses_bad_params_atomically() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width":64,"height":64})).unwrap();
        let before = s.active().unwrap().doc.clone();
        for p in [
            json!({}),
            json!({"center":[0]}),
            json!({"center":[0,0],"pressure":"invalid"}),
            json!({"center":[0,0],"target":"misspelled"}),
            json!({"center":[0,0],"layer":"wrong"}),
            json!({"center":[0,0],"color":[0,1,0,2]}),
            json!({"center":[0,0],"settings":{"count":999999}}),
            json!({"center":[0,0],"settings":{"distribution":{"width":99999,"height":512,"data":[]}}}),
        ] {
            assert!(s.execute("paint.splatter", p).is_err());
            assert_eq!(s.active().unwrap().doc.layers, before.layers);
        }
    }
}
