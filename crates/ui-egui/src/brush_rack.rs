//! Optional illustrated rack. Decorative artwork never participates in a brush footprint.
use crate::{PhotocraftApp, state::Tool, theme::Tokens};
use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use photocraft_engine::{
    BrushSettings,
    paint::{Control, Dynamic, GrayTile, ShapeDynamics, TipShape},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const NAMES: [&str; 8] = ["Squirrel Mop", "Sable Round", "Sable Flat", "Hog Bristle", "Filbert", "Fan", "Rigger", "Synthetic Round"];
pub const USES: [&str; 8] =
    ["Blending / Washes", "Detail / Glazing", "Flat strokes", "Heavy Texture", "Soft edges", "Blending / Texture", "Fine Lines", "General painting"];
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Slot {
    pub preset: String,
    pub artwork: Option<photocraft_engine::palette_cmds::Image>,
    pub aspect: f32,
}
impl Default for Slot {
    fn default() -> Self {
        Self { preset: String::new(), artwork: None, aspect: 0.3 }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub open: bool,
    pub offset: [f32; 2],
    pub scale: f32,
    pub selected: usize,
    pub reduced_motion: bool,
    pub slots: [Slot; 8],
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: true,
            offset: [10.0, -285.0],
            scale: 1.0,
            selected: 0,
            reduced_motion: false,
            slots: std::array::from_fn(|i| Slot { preset: format!("Studio / {}", NAMES[i]), ..Default::default() }),
        }
    }
}
impl State {
    pub fn validate(&self) -> Result<(), String> {
        if self.offset.iter().any(|v| !v.is_finite() || v.abs() > 1e6) || !self.scale.is_finite() || !(0.5..=2.0).contains(&self.scale) || self.selected >= 8 {
            return Err("invalid rack placement, scale or slot".into());
        }
        for slot in &self.slots {
            if slot.preset.is_empty() || slot.preset.len() > 255 || !slot.aspect.is_finite() || !(0.05..=20.0).contains(&slot.aspect) {
                return Err("invalid rack preset or artwork aspect".into());
            }
            if let Some(image) = &slot.artwork
                && image.iter().any(|p| p.width != 256 || p.height != 256 || !p.is_valid())
            {
                return Err("invalid rack artwork".into());
            }
        }
        Ok(())
    }
}
pub fn parse(value: &serde_json::Value) -> Result<State, String> {
    if let Some(slots) = value.get("slots") {
        let slots = slots.as_array().filter(|v| v.len() == 8).ok_or("rack requires eight slots")?;
        for slot in slots {
            if let Some(image) = slot.get("artwork").filter(|v| !v.is_null()) {
                photocraft_engine::palette_cmds::check_image(image).map_err(|e| e.to_string())?;
            }
        }
    }
    let state = State::deserialize(value).map_err(|e| e.to_string())?;
    state.validate()?;
    Ok(state)
}
pub fn factory(index: usize) -> BrushSettings {
    let (size, hardness, roundness, flow) = match index {
        0 => (90.0, 0.1, 1.0, 0.3),
        1 => (18.0, 0.7, 1.0, 0.85),
        2 => (48.0, 1.0, 0.3, 0.9),
        3 => (64.0, 1.0, 0.7, 0.8),
        4 => (48.0, 0.7, 0.45, 0.8),
        5 => (80.0, 0.8, 0.5, 0.6),
        6 => (6.0, 0.85, 0.65, 0.9),
        _ => (32.0, 1.0, 1.0, 1.0),
    };
    let mut b = BrushSettings { size, hardness, roundness, flow, pressure_size: true, pressure_opacity: true, spacing: 0.12, seed: 42, ..Default::default() };
    b.smoothing.amount = 0.1;
    if matches!(index, 2 | 3 | 5) {
        b.tip = TipShape::Sampled(GrayTile::from_fn(64, 64, |x, y| {
            let u = (x as f32 + 0.5) / 32.0 - 1.0;
            let v = (y as f32 + 0.5) / 32.0 - 1.0;
            match index {
                2 => 1.0,
                3 => {
                    if u * u + v * v <= 1.0 {
                        0.25 + 0.75 * photocraft_engine::paint::rng::hash2(x as i32 / 2, 0, 91)
                    } else {
                        0.0
                    }
                }
                _ => {
                    if u.abs() < (0.2 + 0.75 * (v + 1.0) * 0.5) && v.abs() < 0.95 {
                        if (x + 2 * y) % 7 < 4 { 1.0 } else { 0.15 }
                    } else {
                        0.0
                    }
                }
            }
        }));
    }
    if matches!(index, 2..=5) {
        b.shape_dynamics = ShapeDynamics { enabled: true, angle: Dynamic::controlled(Control::Direction), ..Default::default() };
    }
    b.mixer.wet = if index == 0 { 0.95 } else { 0.65 };
    b.mixer.mix = if index == 0 { 0.9 } else { 0.7 };
    b
}
fn choose(app: &mut PhotocraftApp, slot: &Slot, index: usize) -> Result<(), String> {
    if !app.session.tools.presets.iter().any(|p| p.name.eq_ignore_ascii_case(&slot.preset)) {
        app.run("brush.presets.save", json!({"name":slot.preset,"brush":factory(index)}))?;
    }
    app.run("tools.setBrush", json!({"preset":slot.preset}))?;
    if !matches!(app.ui.tool, Tool::Brush | Tool::MixerBrush) {
        app.ui.tool = Tool::MixerBrush;
    }
    Ok(())
}
fn report(app: &mut PhotocraftApp, result: Result<(), String>) {
    if let Err(e) = result {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}
fn illustration(ui: &egui::Ui, rect: Rect, index: usize, extend: f32) {
    let p = ui.painter();
    let scale = rect.height() / 104.0;
    let point = |x: f32, y: f32| pos2(rect.center().x + x * scale, rect.top() + y * scale - extend);
    // Original geometric artwork; size and animation never affect simulated paint.
    p.add(egui::Shape::convex_polygon(
        vec![point(-3.0, 100.0), point(3.0, 100.0), point(5.0, 43.0), point(-5.0, 43.0)],
        Color32::from_rgb(102, 59, 31),
        Stroke::NONE,
    ));
    p.rect_filled(Rect::from_min_max(point(-6.0, 33.0), point(6.0, 52.0)), 2.0, Color32::from_rgb(162, 165, 165));
    let width = match index {
        0 => 13.0,
        2 | 3 => 11.0,
        5 => 16.0,
        6 => 3.0,
        _ => 8.0,
    };
    let color = if index == 3 { Color32::from_rgb(210, 185, 137) } else { Color32::from_rgb(137, 98, 54) };
    if index == 2 || index == 3 {
        p.rect_filled(Rect::from_min_max(point(-width, 9.0), point(width, 34.0)), 1.0, color);
    } else {
        p.add(egui::Shape::convex_polygon(
            vec![point(-6.0, 35.0), point(-width, 15.0), point(0.0, if index == 5 { 13.0 } else { 2.0 }), point(width, 15.0), point(6.0, 35.0)],
            color,
            Stroke::NONE,
        ));
    }
}
#[derive(Clone)]
struct Artwork {
    display_key: u64,
    image: std::sync::Arc<photocraft_engine::palette_cmds::Image>,
    texture: egui::TextureHandle,
}
fn artwork(app: &PhotocraftApp, ctx: &egui::Context, index: usize, image: &photocraft_engine::palette_cmds::Image) -> Option<egui::TextureHandle> {
    let id = egui::Id::new(("rack-art", index));
    let display = crate::artist_palette::srgb_display(app);
    let key = display.as_ref().map_or(0, |d| d.key);
    if let Some(c) = ctx.data_mut(|d| d.get_temp::<Artwork>(id)).filter(|c| c.display_key == key && *c.image == *image) {
        return Some(c.texture);
    }
    let s = photocraft_engine::palette_cmds::surface(image).ok()?;
    let mut buffer = photocraft_compose::Buffer::transparent(photocraft_engine::palette_cmds::bounds());
    for (i, p) in buffer.px.iter_mut().enumerate() {
        *p = s.rgba((i % 256) as i32, (i / 256) as i32);
    }
    let pixels = display.map_or_else(|| buffer.to_rgba8(), |d| d.to_rgba8(&buffer)).pixels;
    let texture = ctx.load_texture(format!("rack-art-{index}"), egui::ColorImage::from_rgba_unmultiplied([256, 256], &pixels), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, Artwork { display_key: key, image: std::sync::Arc::new(image.clone()), texture: texture.clone() }));
    Some(texture)
}
pub fn window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.brush_rack.open || !app.ui.artist_palette.open || app.ui.tool != Tool::MixerBrush {
        return;
    }
    let mut state = std::mem::take(&mut app.ui.brush_rack);
    let mut open = state.open;
    if state.validate().is_err() {
        state = State::default();
    }
    let names =
        [tl!("Squirrel Mop"), tl!("Sable Round"), tl!("Sable Flat"), tl!("Hog Bristle"), tl!("Filbert"), tl!("Fan"), tl!("Rigger"), tl!("Synthetic Round")];
    let uses = [
        tl!("Blending / Washes"),
        tl!("Detail / Glazing"),
        tl!("Flat strokes"),
        tl!("Heavy Texture"),
        tl!("Soft edges"),
        tl!("Blending / Texture"),
        tl!("Fine Lines"),
        tl!("General painting"),
    ];
    let pos = Pos2::from(app.ui.artist_palette.position) + vec2(state.offset[0], state.offset[1]);
    let response = egui::Window::new(tl!("Brush Rack"))
        .id(egui::Id::new("brush-rack"))
        .open(&mut open)
        .current_pos(pos)
        .drag_area(egui::WindowDrag::Anywhere)
        .resizable(false)
        .show(ctx, |ui| {
            let t = Tokens::get(ctx);
            ui.horizontal(|ui| {
                for (i, name) in names.iter().enumerate() {
                    let (r, response) = ui.allocate_exact_size(vec2(38.0 * state.scale, 104.0 * state.scale), Sense::click());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, *name));
                    let hovered = response.hovered() || response.has_focus();
                    let extension = if state.reduced_motion { 0.0 } else { ctx.animate_bool(egui::Id::new(("rack-hover", i)), hovered) * 10.0 * state.scale };
                    if state.selected == i || hovered {
                        ui.painter().rect_filled(r, 3.0, t.field);
                    }
                    if let Some(tex) = state.slots.get(i).and_then(|s| s.artwork.as_ref()).and_then(|image| artwork(app, ctx, i, image)) {
                        let ratio = state.slots.get(i).map_or(0.3, |s| s.aspect);
                        let size = if r.width() / r.height() > ratio { vec2(r.height() * ratio, r.height()) } else { vec2(r.width(), r.width() / ratio) };
                        ui.painter().image(
                            tex.id(),
                            Rect::from_center_size(r.center() - vec2(0.0, extension), size),
                            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        illustration(ui, r, i, extension);
                    }
                    let click = response.clicked();
                    response.on_hover_text(format!("{} — {}", name, uses[i]));
                    if click {
                        state.selected = i;
                        if let Some(slot) = state.slots.get(i) {
                            let result = choose(app, slot, i);
                            report(app, result);
                        }
                    }
                }
            });
            let selected = state.selected.min(7);
            ui.label(format!("{} — {}", names[selected], uses[selected]));
            ui.menu_button(tl!("Options"), |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::Slider::new(&mut state.scale, 0.5..=2.0).text(tl!("Scale")));
                    ui.checkbox(&mut state.reduced_motion, tl!("Reduced motion"));
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.button(tl!("Edit brush tip")).clicked()
                        && let Some(slot) = state.slots.get(selected)
                    {
                        let result = choose(app, slot, selected);
                        report(app, result);
                        app.ui.tip_designer = crate::tip_designer::State::from_brush(&app.session.tools.brush);
                    }
                    if ui.button(tl!("Replace with current brush")).clicked()
                        && let Some(slot) = state.slots.get(selected)
                    {
                        let result = app.run("brush.presets.save", json!({"name":slot.preset})).map(|_| ());
                        report(app, result);
                    }
                    if ui.button(tl!("Use current image as artwork")).clicked() {
                        let aspect =
                            app.session.active().map(|st| st.doc.size.width as f32 / st.doc.size.height.max(1) as f32).unwrap_or(0.3).clamp(0.05, 20.0);
                        let result = app.run("palette.capture", json!({})).and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()));
                        match result {
                            Ok(image) => {
                                if let Some(slot) = state.slots.get_mut(selected) {
                                    slot.artwork = Some(image);
                                    slot.aspect = aspect;
                                }
                            }
                            Err(e) => report(app, Err(e)),
                        }
                    }
                });
                if ui.button(tl!("Reset")).clicked()
                    && let Some(slot) = state.slots.get_mut(selected)
                {
                    slot.artwork = None;
                    slot.aspect = 0.3;
                }
            });
            // A native text button list supplies a stable keyboard and screen-reader alternative.
            ui.menu_button(tl!("Choose brush by name"), |ui| {
                for (i, name) in names.iter().enumerate() {
                    if ui.button(*name).clicked() {
                        state.selected = i;
                        if let Some(slot) = state.slots.get(i) {
                            let result = choose(app, slot, i);
                            report(app, result);
                        }
                        ui.close();
                    }
                }
            });
        });
    if let Some(r) = response {
        state.offset = [r.response.rect.left() - app.ui.artist_palette.position[0], r.response.rect.top() - app.ui.artist_palette.position[1]];
    }
    state.open = open;
    app.ui.brush_rack = state;
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rack_rejects_oversized_artwork_and_invalid_controls_before_decoding() {
        for value in [json!({"scale":0}), json!({"selected":8}), json!({"offset":[1e20,0]}), json!({"slots":[]})] {
            assert!(parse(&value).is_err());
        }
        let mut state = json!(State::default());
        state["slots"][0]["artwork"] = json!((0..4).map(|_| json!({"width":u32::MAX,"height":u32::MAX,"data":""})).collect::<Vec<_>>());
        assert!(parse(&state).is_err());
    }
    #[test]
    fn rack_native_selection_keyboard_artwork_motion_and_drag_preserve_document() {
        use egui_kittest::{Harness, kittest::Queryable};
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":64,"height":128,"background":"#e09840"})).unwrap();
        app.ui.tool = Tool::MixerBrush;
        app.ui.artist_palette.position = [40.0, 340.0];
        let original = app.session.active().unwrap().doc.clone();
        let mut h = Harness::builder().with_size(vec2(1200.0, 1000.0)).build_ui_state(
            |ui, app: &mut PhotocraftApp| {
                let ctx = ui.ctx().clone();
                if ctx.fonts(|f| f.families().contains(&egui::FontFamily::Name("medium".into()))) {
                    window(app, &ctx);
                }
            },
            app,
        );
        PhotocraftApp::setup_context(&h.ctx, crate::theme::ThemeKind::ALL[0]);
        h.run_steps(4);
        let mop = h.get_by_label("Squirrel Mop").rect().center();
        h.hover_at(mop);
        h.run_steps(20);
        assert!(h.ctx.animate_bool(egui::Id::new(("rack-hover", 0usize)), true) > 0.5);
        h.get_by_label("Rigger").click();
        h.run_steps(3);
        assert_eq!(h.state().session.tools.brush.size, 6.0);
        h.get_by_label("Options").click();
        h.run_steps(2);
        h.get_by_label("Use current image as artwork").click();
        h.run_steps(3);
        assert!(h.state().ui.brush_rack.slots[6].artwork.is_some());
        assert_eq!(h.state().ui.brush_rack.slots[6].aspect, 0.5);
        h.get_by_label("Options").click();
        h.run_steps(2);
        h.get_by_label("Reduced motion").click();
        h.run_steps(3);
        assert!(h.state().ui.brush_rack.reduced_motion);
        h.state_mut().run("tools.setBrush", json!({"size":13})).unwrap();
        if h.query_by_label("Replace with current brush").is_none() {
            h.get_by_label("Options").click();
            h.run_steps(2);
        }
        h.get_by_label("Replace with current brush").click();
        h.run_steps(3);
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        h.get_by_label("Sable Round").click();
        h.run_steps(3);
        h.get_by_label("Rigger").click();
        h.run_steps(3);
        assert_eq!(h.state().session.tools.brush.size, 13.0);
        h.get_by_label("Choose brush by name").click();
        h.run_steps(4);
        h.query_all_by_label("Fan").last().unwrap().focus();
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(3);
        assert_eq!(h.state().ui.brush_rack.selected, 5);
        assert_eq!(h.state().session.tools.brush.size, 80.0);
        let before = h.state().ui.brush_rack.offset;
        let window = egui::AreaState::load(&h.ctx, egui::Id::new("brush-rack")).unwrap().rect();
        let title = pos2(window.center().x, window.top() + 12.0);
        h.hover_at(title);
        h.run_steps(1);
        h.drag_at(title);
        h.run_steps(1);
        for i in 1..=5 {
            h.hover_at(title + vec2(i as f32 * 8.0, i as f32 * 5.0));
            h.run_steps(1);
        }
        h.drop_at(title + vec2(40.0, 25.0));
        h.run_steps(3);
        assert!(
            (h.state().ui.brush_rack.offset[0] - before[0] - 40.0).abs() < 2.0,
            "rack follows native window drag: {:?} -> {:?}",
            before,
            h.state().ui.brush_rack.offset
        );
        let position = egui::AreaState::load(&h.ctx, egui::Id::new("brush-rack")).unwrap().rect().min;
        h.state_mut().ui.artist_palette.position[0] += 25.0;
        h.run_steps(3);
        let moved = egui::AreaState::load(&h.ctx, egui::Id::new("brush-rack")).unwrap().rect().min;
        assert!((moved.x - position.x - 25.0).abs() < 2.0, "rack stays attached when palette moves");
        h.get_by_label("Options").click();
        h.run_steps(2);
        h.get_by_label("Edit brush tip").click();
        h.run_steps(3);
        assert!(h.state().ui.tip_designer.open);
        assert!(std::sync::Arc::ptr_eq(&original, &h.state().session.active().unwrap().doc));
    }
    #[test]
    fn rack_families_select_editable_existing_presets_without_editing_artwork() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":64,"height":64})).unwrap();
        let doc = app.session.active().unwrap().doc.clone();
        let state = State::default();
        for (i, slot) in state.slots.iter().enumerate() {
            choose(&mut app, slot, i).unwrap();
            assert!(app.session.tools.presets.iter().any(|p| p.name == slot.preset));
        }
        let name = &state.slots[6].preset;
        app.run("tools.setBrush", json!({"size":13})).unwrap();
        app.run("brush.presets.save", json!({"name":name})).unwrap();
        choose(&mut app, &state.slots[6], 6).unwrap();
        assert_eq!(app.session.tools.brush.size, 13.0);
        app.run("brush.presets.save", json!({"name":name.to_lowercase(),"brush":{"size":17}})).unwrap();
        choose(&mut app, &state.slots[6], 6).unwrap();
        assert_eq!(app.session.tools.brush.size, 17.0, "case variants must preserve the artist's preset");
        assert_eq!(*app.session.active().unwrap().doc, *doc);
        assert_eq!(serde_json::from_value::<State>(json!(state)).unwrap(), state);
    }
}
