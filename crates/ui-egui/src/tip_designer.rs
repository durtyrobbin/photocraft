//! Editable sampled tips. Draft edits stay in the UI; Apply uses the existing journaled
//! tools.setBrush command and presets retain the existing compact GrayTile representation.
use crate::{PhotocraftApp, brush_panel, brush_preview, theme::Tokens};
use egui::{Color32, Sense, Vec2};
use photocraft_engine::paint::{BrushSettings, GrayTile, TipShape};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct State {
    pub open: bool,
    pub version: u32,
    pub brush: BrushSettings,
    pub resolution: u32,
    pub radius: f32,
    pub density: f32,
    pub erase: bool,
    pub name: String,
    #[serde(skip)]
    previous: Option<[f32; 2]>,
    #[serde(skip)]
    undo: Vec<TipShape>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            version: 1,
            brush: BrushSettings::default(),
            resolution: 128,
            radius: 8.0,
            density: 1.0,
            erase: false,
            name: "My painted tip".into(),
            previous: None,
            undo: Vec::new(),
        }
    }
}
impl State {
    pub fn from_brush(brush: &BrushSettings) -> Self {
        let mut result = Self { open: true, brush: brush.clone(), ..Default::default() };
        result.resize(128);
        result.undo.clear();
        result
    }
    fn checkpoint(&mut self) {
        if self.undo.len() >= 16 {
            self.undo.remove(0);
        }
        self.undo.push(self.brush.tip.clone());
    }
    fn resize(&mut self, size: u32) {
        let size = size.clamp(32, 512);
        self.checkpoint();
        let source = self.brush.tip.bitmap().filter(|t| t.is_valid());
        let tile = GrayTile::from_fn(size, size, |x, y| {
            if let Some(t) = source {
                // Keep rectangular imported tips centered at their original aspect ratio.
                let span = t.width.max(t.height) as f32;
                let sx = (x as f32 + 0.5) / size as f32 * span - (span - t.width as f32) / 2.0;
                let sy = (y as f32 + 0.5) / size as f32 * span - (span - t.height as f32) / 2.0;
                if sx < 0.0 || sy < 0.0 || sx >= t.width as f32 || sy >= t.height as f32 { 0.0 } else { t.get(sx as u32, sy as u32) }
            } else {
                let d = ((x as f32 + 0.5 - size as f32 / 2.0).powi(2) + (y as f32 + 0.5 - size as f32 / 2.0).powi(2)).sqrt();
                photocraft_engine::paint::dab_coverage(d, size as f32 / 2.0, self.brush.hardness)
            }
        });
        self.brush.tip = TipShape::Sampled(tile);
        self.resolution = size;
    }
    fn draw(&mut self, from: [f32; 2], to: [f32; 2]) {
        if from.iter().chain(&to).any(|v| !v.is_finite()) {
            return;
        }
        let TipShape::Sampled(tile) = &mut self.brush.tip else { return };
        if !tile.is_valid() || tile.width > 512 || tile.height > 512 {
            return;
        }
        let radius = if self.radius.is_finite() { self.radius.clamp(0.5, 128.0) } else { 1.0 };
        let density = if self.density.is_finite() { self.density.clamp(0.0, 1.0) } else { 1.0 };
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        let length2 = dx * dx + dy * dy;
        for y in 0..tile.height {
            for x in 0..tile.width {
                let (px, py) = (x as f32 + 0.5 - from[0], y as f32 + 0.5 - from[1]);
                let t = if length2 > 0.0 { ((px * dx + py * dy) / length2).clamp(0.0, 1.0) } else { 0.0 };
                let distance = ((px - t * dx).powi(2) + (py - t * dy).powi(2)).sqrt();
                let coverage = (radius + 0.5 - distance).clamp(0.0, 1.0) * density;
                if coverage > 0.0
                    && let Some(value) = tile.data.get_mut((y * tile.width + x) as usize)
                {
                    let old = f32::from(*value) / 65535.0;
                    let next = if self.erase { old * (1.0 - coverage) } else { old + (1.0 - old) * coverage };
                    *value = (next * 65535.0 + 0.5) as u16;
                }
            }
        }
    }
}

pub fn window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.tip_designer.open {
        return;
    }
    let mut state = std::mem::take(&mut app.ui.tip_designer);
    if state.version != 1 || state.brush.tip.bitmap().is_none_or(|t| !t.is_valid() || t.width > 512 || t.height > 512) {
        state = State::from_brush(&app.session.tools.brush);
    }
    state.resolution = state.brush.tip.bitmap().map_or(128, |t| t.width).clamp(32, 512);
    let mut open = state.open;
    let mut apply = false;
    let mut save = false;
    let tokens = Tokens::get(ctx);
    egui::Window::new(tl!("Brush Tip Designer")).id(egui::Id::new("tip-designer")).open(&mut open).resizable(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut state.erase, false, tl!("Paint"));
                    ui.selectable_value(&mut state.erase, true, tl!("Erase"));
                    if ui.add_enabled(!state.undo.is_empty(), egui::Button::new(tl!("Undo tip edit"))).clicked()
                        && let Some(tip) = state.undo.pop()
                    {
                        state.resolution = tip.bitmap().map_or(128, |t| t.width);
                        state.brush.tip = tip;
                    }
                    if ui.button(tl!("Clear tip")).clicked() {
                        state.checkpoint();
                        state.brush.tip = TipShape::Sampled(GrayTile::from_fn(state.resolution, state.resolution, |_, _| 0.0));
                    }
                });
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(256.0), Sense::drag());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, tl!("Draw brush tip")));
                ui.painter().rect_filled(rect, 0.0, tokens.field);
                if let Some(tile) = state.brush.tip.bitmap() {
                    let mut hash = std::collections::hash_map::DefaultHasher::new();
                    (tile.width, tile.height, &tile.data, tokens.text).hash(&mut hash);
                    let texture = brush_preview::with_cache(ctx, |cache| {
                        cache.get("tip-designer-mask", hash.finish(), || {
                            let pixels: Vec<_> = tile.data.iter().map(|v| tokens.text.gamma_multiply(f32::from(*v) / 65535.0)).collect();
                            let image = egui::ColorImage::new([tile.width as usize, tile.height as usize], pixels);
                            ctx.load_texture("tip-designer-mask", image, egui::TextureOptions::NEAREST)
                        })
                    });
                    ui.painter().image(texture.id(), rect, brush_panel::full_uv(), Color32::WHITE);
                }
                if response.drag_started() {
                    state.checkpoint();
                    state.previous = None;
                }
                if response.is_pointer_button_down_on()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let local = (pos - rect.min) / rect.size() * state.resolution as f32;
                    let point = [local.x.clamp(0.0, state.resolution as f32), local.y.clamp(0.0, state.resolution as f32)];
                    state.draw(state.previous.unwrap_or(point), point);
                    state.previous = Some(point);
                    ctx.request_repaint();
                } else {
                    state.previous = None;
                }
                ui.add(egui::Slider::new(&mut state.radius, 0.5..=64.0).text(tl!("Drawing radius")));
                ui.add(egui::Slider::new(&mut state.density, 0.0..=1.0).text(tl!("Drawing density")));
                let mut resolution = state.resolution;
                egui::ComboBox::from_label(tl!("Mask resolution")).selected_text(resolution.to_string()).show_ui(ui, |ui| {
                    for size in [32, 64, 128, 256, 512] {
                        ui.selectable_value(&mut resolution, size, size.to_string());
                    }
                });
                if resolution != state.resolution {
                    state.resize(resolution);
                }
            });
            ui.vertical(|ui| {
                ui.set_width(245.0);
                ui.add(egui::Slider::new(&mut state.brush.size, 1.0..=1024.0).logarithmic(true).text(tl!("Size")));
                ui.add(egui::Slider::new(&mut state.brush.angle, -180.0..=180.0).text(tl!("Angle")));
                ui.add(egui::Slider::new(&mut state.brush.roundness, 0.05..=1.0).text(tl!("Roundness")));
                ui.add(egui::Slider::new(&mut state.brush.spacing, 0.05..=2.0).text(tl!("Spacing")));
                ui.add(egui::Slider::new(&mut state.brush.flow, 0.0..=1.0).text(tl!("Flow")));
                ui.checkbox(&mut state.brush.pressure_size, tl!("Pressure size"));
                ui.checkbox(&mut state.brush.pressure_opacity, tl!("Pressure opacity"));
                ui.checkbox(&mut state.brush.noise, tl!("Noise"));
                ui.checkbox(&mut state.brush.texture.enabled, tl!("Texture"));
                ui.checkbox(&mut state.brush.scattering.enabled, tl!("Scattering"));
                ui.add(egui::Slider::new(&mut state.brush.scattering.scatter.jitter, 0.0..=3.0).text(tl!("Scatter amount")));
                ui.checkbox(&mut state.brush.shape_dynamics.enabled, tl!("Shape Dynamics"));
                ui.add(egui::Slider::new(&mut state.brush.shape_dynamics.size.jitter, 0.0..=1.0).text(tl!("Size variation")));
                ui.horizontal(|ui| {
                    ui.label(tl!("Angle control"));
                    crate::widgets::dropdown(ui, "tip-angle", &mut state.brush.shape_dynamics.angle.control, &crate::brush_sections::CONTROLS, 130.0);
                });
                ui.checkbox(&mut state.brush.shape_dynamics.brush_projection, tl!("Brush Projection"));
                ui.checkbox(&mut state.brush.spacing_enabled, tl!("Fixed spacing"));
            });
        });
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (brush_preview::preview_sig(&state.brush), state.brush.tip.bitmap().map(|t| &t.data), tokens.text).hash(&mut hash);
        let texture = brush_preview::with_cache(ctx, |cache| {
            cache.get("tip-designer-stroke", hash.finish(), || {
                let c = tokens.text.to_normalized_gamma_f32();
                let pixels = brush_preview::preview_pixels(&state.brush, 512, 72, [c[0], c[1], c[2], 1.0]);
                ctx.load_texture("tip-designer-stroke", egui::ColorImage::from_rgba_unmultiplied([512, 72], &pixels), egui::TextureOptions::LINEAR)
            })
        });
        ui.image((texture.id(), egui::vec2(512.0, 72.0)));
        ui.horizontal(|ui| {
            apply = ui.button(tl!("Apply tip and paint")).clicked();
            ui.text_edit_singleline(&mut state.name);
            save = ui.button(tl!("Save brush preset")).clicked();
        });
    });
    state.open = open;
    if apply || save {
        if let Err(error) = app.run("tools.setBrush", json!({"brush":state.brush})) {
            app.ui.status = error;
        } else if save {
            brush_panel::run_or_status(app, "brush.presets.save", json!({"name":state.name}));
        } else {
            state.open = false;
        }
    }
    app.ui.tip_designer = state;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_editor_applies_through_command_without_editing_the_document_and_caches_previews() {
        use egui_kittest::{Harness, kittest::Queryable};
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.session.execute("file.new", json!({"width":64,"height":64})).unwrap();
        let original = app.session.active().unwrap().doc.clone();
        app.ui.tip_designer = State::from_brush(&app.session.tools.brush);
        let mut h = Harness::builder().with_size(egui::vec2(1000.0, 800.0)).build_ui_state(
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
        let renders = brush_preview::render_count(&h.ctx);
        h.run_steps(3);
        assert_eq!(brush_preview::render_count(&h.ctx), renders);
        h.get_by_label("Clear tip").click();
        h.run_steps(2);
        assert!(h.state().ui.tip_designer.brush.tip.bitmap().unwrap().data.iter().all(|v| *v == 0));
        h.get_by_label("Undo tip edit").click();
        h.run_steps(2);
        assert!(h.state().ui.tip_designer.brush.tip.bitmap().unwrap().data.iter().any(|v| *v > 0));
        h.get_by_label("Apply tip and paint").click();
        h.run_steps(2);
        assert!(!h.state().ui.tip_designer.open);
        assert!(matches!(h.state().session.tools.brush.tip, TipShape::Sampled(_)));
        assert_eq!(h.state().session.journal.last().unwrap().0, "tools.setBrush");
        assert!(std::sync::Arc::ptr_eq(&original, &h.state().session.active().unwrap().doc));
    }
    #[test]
    fn painted_tip_erases_resizes_roundtrips_and_paints_with_the_existing_engine() {
        let mut draft = State::from_brush(&BrushSettings::default());
        draft.brush.tip = TipShape::Sampled(GrayTile::from_fn(128, 128, |_, _| 0.0));
        draft.draw([32.0, 32.0], [96.0, 96.0]);
        let tile = draft.brush.tip.bitmap().unwrap();
        assert!(tile.get(64, 64) > 0.99 && tile.get(100, 10) == 0.0);
        draft.erase = true;
        draft.draw([64.0, 64.0], [64.0, 64.0]);
        assert_eq!(draft.brush.tip.bitmap().unwrap().get(64, 64), 0.0);
        draft.resize(64);
        assert_eq!(draft.brush.tip.bitmap().unwrap().width, 64);
        let restored: State = serde_json::from_value(serde_json::to_value(&draft).unwrap()).unwrap();
        assert_eq!(restored.brush, draft.brush);
        let mut session = photocraft_engine::Session::new();
        session.execute("file.new", json!({"width":128,"height":128})).unwrap();
        session.execute("layer.new.layer", json!({})).unwrap();
        session.execute("tools.setBrush", json!({"brush":restored.brush,"size":64,"pressureSize":false})).unwrap();
        session.execute("brush.presets.save", json!({"name":"Drawn mask"})).unwrap();
        session.execute("paint.stroke", json!({"points":[[64,64]],"color":"#ff0000"})).unwrap();
        let doc = &session.active().unwrap().doc;
        let layer = doc.layer(session.active().unwrap().active_layer.unwrap()).unwrap().surface().unwrap();
        assert_eq!(layer.rgba(64, 64)[3], 0.0);
        assert!(layer.rgba(54, 54)[3] > 0.9);
        session.execute("tools.setBrush", json!({"reset":true})).unwrap();
        session.execute("tools.setBrush", json!({"preset":"Drawn mask"})).unwrap();
        assert_eq!(session.tools.brush.tip, draft.brush.tip);
    }
}
