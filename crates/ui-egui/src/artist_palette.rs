//! Movable artist palette, backed by command-owned pixels and cached display textures.
use crate::{PhotocraftApp, state::Tool, theme::Tokens};
use egui::{Color32, Pos2, Rect, Sense, Stroke, pos2, vec2};
use photocraft_engine::palette_cmds;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Deposit,
    Mix,
    Pickup,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub open: bool,
    pub position: [f32; 2],
    pub width: f32,
    pub mode: Mode,
    pub size: f32,
    pub ryb: bool,
    pub preset_name: String,
    #[serde(skip)]
    pub previous: Option<[f64; 2]>,
}
impl Default for State {
    fn default() -> Self {
        Self { open: true, position: [70.0, 360.0], width: 380.0, mode: Mode::Deposit, size: 28.0, ryb: false, preset_name: "Study".into(), previous: None }
    }
}
impl State {
    pub fn validate(&self) -> Result<(), String> {
        if self.position.iter().any(|v| !v.is_finite() || v.abs() > 1e6)
            || !self.width.is_finite()
            || !(280.0..=900.0).contains(&self.width)
            || !self.size.is_finite()
            || !(2.0..=128.0).contains(&self.size)
            || self.preset_name.len() > 128
        {
            return Err("invalid palette placement, scale, size or preset name".into());
        }
        Ok(())
    }
}
/// The toolbar and palette swatches share a typed drag payload. A click still opens the picker.
#[derive(Clone)]
pub struct ColorDrag(pub [f32; 4]);
#[derive(Clone)]
struct Cache {
    revision: u64,
    display_key: u64,
    texture: egui::TextureHandle,
    hues: Vec<f32>,
}

fn run(app: &mut PhotocraftApp, id: &str, p: Value) {
    if let Err(e) = app.run(id, p) {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}
fn hsv(rgb: [f32; 3]) -> [f32; 3] {
    let [h, s, v] = crate::color_picker_ui::rgb_to_hsv(rgb);
    [h / 360.0, s, v]
}
/// An explicitly approximate artist RYB interpretation: yellow moves from HSV 60° to 120°.
pub fn wheel_hue(h: f32, ryb: bool, inverse: bool) -> f32 {
    if !ryb {
        return h.rem_euclid(1.0);
    }
    let rgb = [0.0, 60.0, 120.0, 180.0, 240.0, 300.0, 360.0];
    let artist = [0.0, 120.0, 180.0, 210.0, 240.0, 300.0, 360.0];
    let (src, dst) = if inverse { (artist, rgb) } else { (rgb, artist) };
    let degrees = h.rem_euclid(1.0) * 360.0;
    for (a, b) in src.windows(2).zip(dst.windows(2)) {
        if degrees <= a[1] {
            return (b[0] + (b[1] - b[0]) * (degrees - a[0]) / (a[1] - a[0])) / 360.0;
        }
    }
    0.0
}
pub fn relationships(hues: &[f32], ryb: bool) -> [bool; 4] {
    let hues: Vec<_> = hues.iter().map(|h| wheel_hue(*h, ryb, false) * 360.0).collect();
    let near = |a: f32, b: f32| ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs() < 15.0;
    let mut found = [false; 4];
    for a in &hues {
        for b in &hues {
            let delta = (b - a).rem_euclid(360.0);
            found[0] |= near(delta, 180.0);
            found[1] |= (15.0..=45.0).contains(&delta);
            if near(delta, 120.0) && hues.iter().any(|c| near(c - a, 240.0)) {
                found[2] = true;
            }
            if near(delta, 150.0) && hues.iter().any(|c| near(c - a, 210.0)) {
                found[3] = true;
            }
        }
    }
    found
}
pub(crate) fn srgb_display(app: &PhotocraftApp) -> Option<std::sync::Arc<photocraft_engine::display_color::CanvasDisplay>> {
    // Palette paint uses SDR sRGB, like the foreground picker. Monitor conversion remains active.
    static DOCUMENT: std::sync::OnceLock<photocraft_doc::Document> = std::sync::OnceLock::new();
    let doc = DOCUMENT.get_or_init(|| {
        let mut d =
            photocraft_doc::Document::new("Palette", photocraft_doc::Size::new(256, 256), photocraft_color::ColorMode::Rgb, photocraft_color::SampleType::U16);
        d.icc_profile = Some(photocraft_cms::Builtin::Srgb.profile().to_bytes());
        d
    });
    app.session.color.canvas_display(doc).ok()
}
fn cache(app: &PhotocraftApp, ctx: &egui::Context) -> Cache {
    let palette = &app.session.tools.artist_palette;
    let id = egui::Id::new("artist-palette-image");
    let display = srgb_display(app);
    let key = display.as_ref().map_or(0, |d| d.key);
    if let Some(c) = ctx.data_mut(|d| d.get_temp::<Cache>(id)).filter(|c| c.revision == palette.revision && c.display_key == key) {
        return c;
    }
    let skin = palette.skin.as_ref().and_then(|i| palette_cmds::surface(i).ok());
    let mut buffer = photocraft_compose::Buffer::transparent(palette_cmds::bounds());
    let mut bins = [0.0f32; 36];
    for (i, out) in buffer.px.iter_mut().enumerate() {
        let (x, y) = ((i % 256) as i32, (i / 256) as i32);
        if !palette_cmds::contact(x as f32 + 0.5, y as f32 + 0.5) {
            continue;
        }
        let grain = ((y as f32 * 0.17 + (x as f32 * 0.011).sin() * 1.7).sin() * 0.025) + photocraft_engine::paint::rng::hash2(x, y, 82) * 0.018;
        let wood = [0.61 + grain, 0.40 + grain, 0.23 + grain, 1.0];
        let art = skin.as_ref().map_or(wood, |s| s.rgba(x, y));
        let under = std::array::from_fn::<_, 4, _>(|c| art[c] * art[3] + wood[c] * (1.0 - art[3]));
        let p = palette.paint.rgba(x, y);
        *out = [p[0] * p[3] + under[0] * (1.0 - p[3]), p[1] * p[3] + under[1] * (1.0 - p[3]), p[2] * p[3] + under[2] * (1.0 - p[3]), 1.0];
        let [h, s, v] = hsv([p[0], p[1], p[2]]);
        if s > 0.15
            && v > 0.1
            && let Some(bin) = bins.get_mut((h * 36.0).floor() as usize % 36)
        {
            *bin += p[3];
        }
    }
    let total = bins.iter().sum::<f32>();
    let mut top: Vec<_> = bins.iter().enumerate().filter(|(_, v)| **v > (total * 0.035).max(1.0)).collect();
    top.sort_by(|a, b| b.1.total_cmp(a.1));
    top.truncate(12);
    let hues = top.into_iter().map(|(i, _)| (i as f32 + 0.5) / 36.0).collect();
    let rgba = display.map_or_else(|| buffer.to_rgba8(), |d| d.to_rgba8(&buffer));
    let texture = ctx.load_texture("artist-palette", egui::ColorImage::from_rgba_unmultiplied([256, 256], &rgba.pixels), egui::TextureOptions::LINEAR);
    let result = Cache { revision: palette.revision, display_key: key, texture, hues };
    ctx.data_mut(|d| d.insert_temp(id, result.clone()));
    result
}
fn wheel(ui: &mut egui::Ui, hues: &[f32], ryb: bool) {
    let (r, _) = ui.allocate_exact_size(vec2(94.0, 94.0), Sense::hover());
    let center = r.center();
    let tokens = Tokens::get(ui.ctx());
    for i in 0..72 {
        let h = i as f32 / 72.0;
        let angle = h * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let color = egui::ecolor::Hsva::new(wheel_hue(h, ryb, true), 0.8, 0.9, 1.0);
        ui.painter().circle_filled(center + vec2(angle.cos(), angle.sin()) * 36.0, 4.0, Color32::from(color));
    }
    for h in hues {
        let a = wheel_hue(*h, ryb, false) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let p = center + vec2(a.cos(), a.sin()) * 36.0;
        ui.painter().circle_stroke(p, 5.0, Stroke::new(2.0, tokens.text));
    }
}
pub fn window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.artist_palette.open || app.ui.tool != Tool::MixerBrush {
        return;
    }
    let mut state = std::mem::take(&mut app.ui.artist_palette);
    if state.validate().is_err() {
        state = State::default();
    }
    let mut open = state.open;
    let cached = cache(app, ctx);
    let result = egui::Window::new(tl!("Artist Palette"))
        .id(egui::Id::new("artist-palette"))
        .open(&mut open)
        .current_pos(Pos2::from(state.position))
        .drag_area(egui::WindowDrag::Anywhere)
        .default_width(state.width)
        .default_height(650.0)
        .min_width(280.0)
        .max_width(900.0)
        .vscroll(true)
        .max_height((ctx.content_rect().height() - 100.0).max(100.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (mode, label) in [(Mode::Deposit, tl!("Deposit")), (Mode::Mix, tl!("Mix")), (Mode::Pickup, tl!("Pick up paint"))] {
                    ui.selectable_value(&mut state.mode, mode, label);
                }
                if ui.button(tl!("Undo")).clicked() {
                    run(app, "palette.undo", json!({}));
                }
                if ui.button(tl!("Clear")).clicked() {
                    run(app, "palette.clear", json!({}));
                }
            });
            if ui.button(tl!("Brush Rack")).clicked() {
                app.ui.brush_rack.open = true;
            }
            ui.add(egui::Slider::new(&mut state.size, 2.0..=128.0).text(tl!("Size")));
            if ui.button(tl!("Deposit canvas load")).clicked() {
                run(app, "palette.depositLoad", json!({"x":128,"y":96,"size":state.size.max(24.0)}));
            }
            let width = ui.available_width().clamp(260.0, 860.0);
            let (rect, response) = ui.allocate_exact_size(vec2(width, width * 0.72), Sense::click_and_drag());
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, tl!("Palette mixing surface")));
            ui.painter().image(cached.texture.id(), rect, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
            let point = |p: Pos2| {
                [
                    (f64::from((p.x - rect.left()) / rect.width()) * 256.0).clamp(0.0, 255.0),
                    (f64::from((p.y - rect.top()) / rect.height()) * 256.0).clamp(0.0, 255.0),
                ]
            };
            if let Some(payload) = response.dnd_release_payload::<ColorDrag>()
                && let Some(at) = ui.input(|i| i.pointer.latest_pos())
            {
                run(app, "palette.deposit", json!({"points":[point(at)],"color":payload.0,"size":state.size}));
            }
            if let Some(at) = response.interact_pointer_pos().filter(|_| response.clicked() || response.dragged() || response.is_pointer_button_down_on()) {
                let at = point(at);
                let begin = state.previous.is_none();
                let changed = state.previous != Some(at);
                if state.previous == Some(at) {
                    // Holding still is not an airbrush: do not repeatedly deposit the same dab.
                } else if state.mode == Mode::Pickup || ui.input(|i| i.modifiers.alt) {
                    run(app, "palette.pickup", json!({"x":at[0],"y":at[1],"size":state.size}));
                } else {
                    let points = state.previous.map_or_else(|| vec![at], |old| vec![old, at]);
                    run(app, if state.mode == Mode::Mix { "palette.mix" } else { "palette.deposit" }, json!({"points":points,"size":state.size,"begin":begin}));
                }
                state.previous = Some(at);
                if changed {
                    ctx.request_repaint();
                }
            }
            if response.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
                state.previous = None;
            }
            ui.horizontal(|ui| {
                wheel(ui, &cached.hues, state.ryb);
                ui.vertical(|ui| {
                    ui.checkbox(&mut state.ryb, tl!("RYB interpretation"));
                    ui.label(if state.ryb { tl!("Approximate artist hue wheel") } else { "RGB / HSV" });
                    let names = [tl!("Complementary"), tl!("Analogous"), tl!("Triadic"), tl!("Split-complementary")];
                    for (present, label) in relationships(&cached.hues, state.ryb).into_iter().zip(names) {
                        if present {
                            ui.label(label);
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut state.preset_name).desired_width(110.0).char_limit(128));
                if ui.button(tl!("Save palette")).clicked() {
                    run(app, "palette.presets.save", json!({"name":state.preset_name}));
                }
                ui.menu_button(tl!("Load palette"), |ui| {
                    let names: Vec<_> = app.session.tools.artist_palette.presets.iter().map(|p| p.name.clone()).collect();
                    for name in names {
                        if ui.button(&name).clicked() {
                            run(app, "palette.presets.load", json!({"name":name}));
                            ui.close();
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                if ui.button(tl!("Use current image as skin")).clicked() {
                    run(app, "palette.skin", json!({}));
                }
                if ui.button(tl!("Wood skin")).clicked() {
                    run(app, "palette.skin", json!({"clear":true}));
                }
            });
            ui.label(tl!("Drag a foreground color here; Alt-click to pick up paint"));
            ui.min_rect().width()
        });
    if let Some(r) = result {
        state.position = [r.response.rect.left(), r.response.rect.top()];
        state.width = r.inner.unwrap_or(state.width).clamp(280.0, 900.0);
    }
    state.open = open;
    app.ui.artist_palette = state;
    if cached.revision != app.session.tools.artist_palette.revision {
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_native_mix_drag_is_one_local_undo_and_window_moves_and_resizes() {
        use egui_kittest::{Harness, kittest::Queryable};
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.ui.tool = Tool::MixerBrush;
        app.ui.artist_palette.position = [40.0, 40.0];
        for (x, c) in [(85, [1, 0, 0, 1]), (155, [0, 0, 1, 1])] {
            app.run("palette.deposit", json!({"points":[[x,100]],"color":c,"size":60})).unwrap();
        }
        let before = app.session.tools.artist_palette.paint.clone();
        let mut h = Harness::builder().with_size(vec2(1200.0, 1100.0)).build_ui_state(
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
        h.get_by_label("Mix").click();
        h.run_steps(2);
        let r = h.get_by_label("Palette mixing surface").rect();
        let point = |x: f32| r.min + vec2(r.width() * x / 256.0, r.height() * 100.0 / 256.0);
        let drag = |h: &mut Harness<'_, PhotocraftApp>, from: Pos2, to: Pos2| {
            h.hover_at(from);
            h.run_steps(1);
            h.drag_at(from);
            h.run_steps(1);
            for i in 1..=6 {
                h.hover_at(from + (to - from) * (i as f32 / 6.0));
                h.run_steps(1);
            }
            h.drop_at(to);
            h.run_steps(3);
        };
        drag(&mut h, point(85.0), point(160.0));
        assert!(h.state().session.tools.artist_palette.paint != before);
        h.get_by_label("Undo").click();
        h.run_steps(3);
        assert!(h.state().session.tools.artist_palette.paint == before);
        let bounds = egui::AreaState::load(&h.ctx, egui::Id::new("artist-palette")).unwrap().rect();
        let title = pos2(bounds.center().x, bounds.top() + 12.0);
        let old = h.state().ui.artist_palette.position;
        drag(&mut h, title, title + vec2(45.0, 25.0));
        assert!(
            (h.state().ui.artist_palette.position[0] - old[0] - 45.0).abs() < 2.0,
            "palette follows native drag: {:?} -> {:?}",
            old,
            h.state().ui.artist_palette.position
        );
        let bounds = egui::AreaState::load(&h.ctx, egui::Id::new("artist-palette")).unwrap().rect();
        let old = h.state().ui.artist_palette.width;
        let corner = bounds.right_bottom() - vec2(2.0, 2.0);
        drag(&mut h, corner, corner + vec2(80.0, 25.0));
        assert!(h.state().ui.artist_palette.width > old + 40.0, "palette native resize changes the stored width");
    }
    #[test]
    fn palette_native_surface_deposit_pickup_save_clear_and_skin_are_real() {
        use egui_kittest::{Harness, kittest::Queryable};
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":64,"height":64,"background":"#3090b0"})).unwrap();
        app.run("tools.setColors", json!({"foreground":"#ed4020"})).unwrap();
        app.ui.tool = Tool::MixerBrush;
        app.ui.artist_palette.position = [40.0, 40.0];
        let original = app.session.active().unwrap().doc.clone();
        let mut h = Harness::builder().with_size(vec2(1100.0, 1000.0)).build_ui_state(
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
        h.get_by_label("Palette mixing surface").click();
        h.run_steps(3);
        assert!(h.state().session.tools.artist_palette.paint.rgba(128, 128)[3] > 0.9);
        h.get_by_label("Pick up paint").click();
        h.run_steps(2);
        h.get_by_label("Palette mixing surface").click();
        h.run_steps(2);
        assert!(h.state().session.tools.mixer.spatial.is_some());
        h.get_by_label("Save palette").click();
        h.run_steps(2);
        assert_eq!(h.state().session.tools.artist_palette.presets.len(), 1);
        h.get_by_label("Clear").click();
        h.run_steps(2);
        assert_eq!(h.state().session.tools.artist_palette.paint.rgba(128, 128)[3], 0.0);
        h.get_by_label("Undo").click();
        h.run_steps(2);
        assert!(h.state().session.tools.artist_palette.paint.rgba(128, 128)[3] > 0.9);
        h.get_by_label("Use current image as skin").click();
        h.run_steps(2);
        assert!(h.state().session.tools.artist_palette.skin.is_some());
        assert_eq!(*h.state().session.active().unwrap().doc, *original);
    }
    #[test]
    fn palette_harmony_is_view_only_and_ryb_round_trips() {
        assert!((hsv([0.0, 0.0, 1.0])[0] - 2.0 / 3.0).abs() < 1e-6);
        for i in 0..360 {
            let h = i as f32 / 360.0;
            assert!((wheel_hue(wheel_hue(h, true, false), true, true) - h).abs() < 1e-5);
        }
        assert!(relationships(&[0.0, 0.5], false)[0]);
        assert!(relationships(&[0.0, 1.0 / 3.0, 2.0 / 3.0], false)[2]);
        assert!(relationships(&[0.0, 150.0 / 360.0, 210.0 / 360.0], false)[3]);
    }
}
