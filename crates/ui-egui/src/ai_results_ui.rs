//! Provider-independent result inspection and canvas comparison.
use crate::{PhotocraftApp, canvas::ViewXform};
use egui::{Color32, Rect, Stroke, TextureOptions, pos2};
use photocraft_doc::{Document, LayerId};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

#[cfg(test)]
#[path = "ai_results_ui_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Vertical,
    Horizontal,
    Overlay,
    Before,
    After,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub open: bool,
    pub enabled: bool,
    pub layer: Option<u64>,
    pub mode: Mode,
    pub position: f32,
    pub overlay: f32,
    pub prompt: String,
    pub material: String,
    pub lighting: String,
    pub provider: String,
    pub model: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            enabled: false,
            layer: None,
            mode: Mode::Vertical,
            position: 0.5,
            overlay: 0.5,
            prompt: String::new(),
            material: String::new(),
            lighting: String::new(),
            provider: "Imported image".into(),
            model: String::new(),
        }
    }
}
impl State {
    pub fn validate(&self) -> Result<(), String> {
        if !self.position.is_finite() || !(0.0..=1.0).contains(&self.position) || !self.overlay.is_finite() || !(0.0..=1.0).contains(&self.overlay) {
            return Err("comparison position and opacity must be finite values in 0..1".into());
        }
        if self.prompt.len() > 8000
            || self.material.len() > 2000
            || self.lighting.len() > 2000
            || self.provider.is_empty()
            || self.provider.len() > 128
            || self.model.len() > 256
        {
            return Err("result text exceeds its limit".into());
        }
        Ok(())
    }
}
pub struct Cache {
    source: Arc<Document>,
    layer: LayerId,
    display_key: u64,
    before: egui::TextureHandle,
    after: egui::TextureHandle,
}
pub fn selected(app: &PhotocraftApp, idx: usize) -> Option<LayerId> {
    let st = app.session.documents().get(idx)?;
    let id = st.active_layer.filter(|id| st.doc.layer(*id).is_some_and(|l| l.ai_result.is_some())).or(app.ui.ai_results.layer.map(LayerId))?;
    st.doc.layer(id).filter(|l| l.ai_result.is_some()).map(|_| id)
}
pub fn active(app: &PhotocraftApp, idx: usize) -> bool {
    app.ui.ai_results.enabled
        && selected(app, idx).is_some()
        && app.drag.is_none()
        && app.ui.transform.is_none()
        && app.ui.dialogs.is_empty()
        && app.ui.text_edit.is_none()
        && !crate::crop_ui::active(app)
        && app.session.documents().get(idx).is_some_and(|st| photocraft_engine::float_cmds::floating(st).is_none())
}
/// Each display gets a bounded cache. Moving the divider only changes texture clips;
/// no compositor work occurs until the document, result or display transform changes.
fn textures(app: &mut PhotocraftApp, ctx: &egui::Context, idx: usize, output: Option<u32>) -> Option<(egui::TextureId, egui::TextureId)> {
    let id = selected(app, idx)?;
    let doc = app.session.documents().get(idx)?.doc.clone();
    let (display, display_key) = crate::canvas::canvas_display(app, &doc, output);
    app.ai_result_caches.retain(|c| Arc::ptr_eq(&c.source, &doc) && c.layer == id);
    if let Some(c) = app.ai_result_caches.iter().find(|c| c.display_key == display_key) {
        return Some((c.before.id(), c.after.id()));
    }
    let mut before = (*doc).clone();
    before.layer_mut(id)?.visible = false;
    let factor = doc.size.width.max(doc.size.height).div_ceil(crate::canvas::MAX_TEXTURE).max(1);
    let w = (doc.size.width / factor).max(1);
    let h = (doc.size.height / factor).max(1);
    let make = |doc: &Document, name: &str| {
        let display_doc = photocraft_engine::mode_cmds::display_document(doc);
        let doc = display_doc.as_ref().unwrap_or(doc);
        let buffer = photocraft_compose::render_reduced(doc, w, h);
        ctx.load_texture(name, crate::canvas::display_image(display.as_deref(), &buffer), TextureOptions::LINEAR)
    };
    let before = make(&before, "ai-result-before");
    let after = make(&doc, "ai-result-after");
    let ids = (before.id(), after.id());
    if app.ai_result_caches.len() >= 4 {
        app.ai_result_caches.remove(0);
    }
    app.ai_result_caches.push(Cache { source: doc, layer: id, display_key, before, after });
    Some(ids)
}
pub fn paint(app: &mut PhotocraftApp, painter: &egui::Painter, xf: &ViewXform, idx: usize, output: Option<u32>) -> bool {
    if !active(app, idx) {
        return false;
    }
    let Some((before, after)) = textures(app, painter.ctx(), idx, output) else { return false };
    let Some(st) = app.session.documents().get(idx) else { return false };
    let size = [st.doc.size.width as f32, st.doc.size.height as f32];
    let uv = Rect::from_min_max(egui::Pos2::ZERO, pos2(1.0, 1.0));
    let draw = |texture, clip: Rect, opacity: f32| {
        crate::canvas::paint_mapped_image(&painter.with_clip_rect(clip), xf, texture, uv, size, Color32::from_white_alpha((opacity * 255.0).round() as u8))
    };
    let state = &app.ui.ai_results;
    let rect = xf.rect;
    match state.mode {
        Mode::Before => draw(before, rect, 1.0),
        Mode::After => draw(after, rect, 1.0),
        Mode::Overlay => {
            draw(before, rect, 1.0);
            draw(after, rect, state.overlay);
        }
        Mode::Vertical | Mode::Horizontal => {
            let mut first = rect;
            let mut second = rect;
            let line = if state.mode == Mode::Vertical {
                let x = rect.left() + rect.width() * state.position;
                first.max.x = x;
                second.min.x = x;
                [pos2(x, rect.top()), pos2(x, rect.bottom())]
            } else {
                let y = rect.top() + rect.height() * state.position;
                first.max.y = y;
                second.min.y = y;
                [pos2(rect.left(), y), pos2(rect.right(), y)]
            };
            draw(before, first, 1.0);
            draw(after, second, 1.0);
            painter.line_segment(line, Stroke::new(2.0, crate::theme::Tokens::get(painter.ctx()).accent));
        }
    }
    true
}
/// The whole canvas is a comparison view while enabled. A divider gesture cannot
/// reach painting; wheel, scrollbars, Space and middle-button navigation remain available.
pub fn pointer(app: &mut PhotocraftApp, ui: &egui::Ui, rect: Rect, response: &egui::Response) {
    if !matches!(app.ui.ai_results.mode, Mode::Vertical | Mode::Horizontal) {
        return;
    }
    let vertical = app.ui.ai_results.mode == Mode::Vertical;
    if response.hovered() {
        ui.ctx().set_cursor_icon(if vertical { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
    }
    if response.is_pointer_button_down_on()
        && ui.input(|i| i.pointer.primary_down())
        && let Some(p) = response.interact_pointer_pos()
    {
        let v = if vertical { (p.x - rect.left()) / rect.width().max(1.0) } else { (p.y - rect.top()) / rect.height().max(1.0) };
        app.ui.ai_results.position = v.clamp(0.0, 1.0);
        ui.ctx().request_repaint();
    }
}
pub fn shortcuts(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let Some(idx) = app.session.active_index() else {
        app.ai_result_caches.clear();
        return;
    };
    if !active(app, idx) {
        app.ai_result_caches.clear();
        return;
    }
    if ctx.text_edit_focused() {
        return;
    }
    for (key, mode) in [(egui::Key::B, Mode::Before), (egui::Key::A, Mode::After), (egui::Key::S, Mode::Vertical)] {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
            app.ui.ai_results.mode = mode;
        }
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.canvas_input_blocked_until_release |= ctx.input(|i| i.pointer.primary_down() || i.pointer.primary_released());
        app.ui.ai_results.enabled = false;
    }
}
fn report(app: &mut PhotocraftApp, result: Result<Value, String>) {
    if let Err(e) = result {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}
fn import_dialog(app: &mut PhotocraftApp, state: &State, replace: Option<LayerId>) -> Result<Value, String> {
    use crate::file_dialog::{FileDialogAnswer, FileDialogRequest};
    let st = app.session.active().ok_or("no document open")?;
    let source = st.doc.clone();
    let area = source.selection.as_ref().map_or(source.bounds(), |s| s.content_bounds().intersect(&source.bounds()));
    let recipe = photocraft_doc::ai_result::EditRecipe {
        prompt: state.prompt.clone(),
        material: state.material.clone(),
        lighting: state.lighting.clone(),
        region: area,
        context_pixels: 0,
        output_size: [area.width(), area.height()],
    };
    recipe.validate()?;
    let mut params = json!({"recipe":recipe,"provider":state.provider,"model":state.model});
    if let Some(id) = replace {
        params["replace"] = json!(id.0);
    }
    app.ask_file(
        FileDialogRequest::Open { multiple: false, initial_dir: None, extensions: Some(vec!["png".into(), "jpg".into(), "jpeg".into(), "webp".into()]) },
        move |app, answer| {
            if app.session.active().is_none_or(|st| !Arc::ptr_eq(&st.doc, &source)) {
                return Err("document changed while choosing a result; choose again".into());
            }
            match answer {
                FileDialogAnswer::Paths(paths) if paths.len() == 1 => params["path"] = json!(paths.first()),
                FileDialogAnswer::Contents(_, bytes) if bytes.len() <= photocraft_engine::ai_result_cmds::MAX_BYTES => {
                    params["data"] = json!(photocraft_paint::tile::b64_encode(&bytes))
                }
                _ => return Err("choose one image of at most 48 MiB".into()),
            }
            let result = app.run("ai.result.import", params)?;
            if let Some(id) = result.get("layer").and_then(Value::as_u64) {
                app.ui.ai_results.layer = Some(id);
            }
            Ok(result)
        },
    )
}
pub fn window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.ai_results.open {
        return;
    }
    let mut state = std::mem::take(&mut app.ui.ai_results);
    let mut open = state.open;
    let layers: Vec<_> = app
        .session
        .active()
        .map(|st| {
            st.doc.walk().into_iter().filter_map(|(_, _, l)| l.ai_result.as_ref().map(|r| (l.id, l.name.clone(), r.clone(), l.visible, l.opacity))).collect()
        })
        .unwrap_or_default();
    if let Some(id) = app.session.active().and_then(|st| st.active_layer).filter(|id| layers.iter().any(|l| l.0 == *id)) {
        state.layer = Some(id.0);
    }
    egui::Window::new(tl!("AI Results"))
        .id(egui::Id::new("ai-results"))
        .open(&mut open)
        .default_pos(pos2(80.0, 110.0))
        .default_width(310.0)
        .vscroll(true)
        .max_height((ctx.content_rect().height() - 160.0).max(100.0))
        .show(ctx, |ui| {
            egui::ComboBox::from_label(tl!("Result layer"))
                .selected_text(layers.iter().find(|l| Some(l.0.0) == state.layer).map_or(tl!("Select a result"), |l| l.1.as_str()))
                .show_ui(ui, |ui| {
                    for (id, name, _, _, _) in &layers {
                        if ui.selectable_value(&mut state.layer, Some(id.0), name).changed() {
                            let r = app.run("layer.select", json!({"layer":id.0}));
                            report(app, r);
                        }
                    }
                });
            if let Some((id, _, record, visible, opacity)) = layers.iter().find(|l| Some(l.0.0) == state.layer) {
                ui.label(format!("{} · {}", record.provider, record.model.as_deref().unwrap_or("—")));
                if record.origin == photocraft_doc::ai_result::ResultOrigin::Imported {
                    ui.weak(tl!("Imported attribution is unverified"));
                }
                ui.label(if record.status == photocraft_doc::ai_result::ResultStatus::Accepted {
                    tl!("Accepted · remains editable")
                } else {
                    tl!("Proposed result")
                });
                let mut visible = *visible;
                let mut opacity = *opacity;
                if ui.checkbox(&mut visible, tl!("Visible")).changed() {
                    let r = app.run("layer.setProps", json!({"layer":id.0,"visible":visible}));
                    report(app, r);
                }
                if ui.add(egui::Slider::new(&mut opacity, 0.0..=1.0).text(tl!("Opacity"))).changed() {
                    let r = app.run("layer.setProps", json!({"layer":id.0,"opacity":opacity,"coalesce":"ai-result-opacity"}));
                    report(app, r);
                }
                ui.horizontal(|ui| {
                    if ui.button(tl!("Accept")).clicked() {
                        let r = app.run("ai.result.accept", json!({"layer":id.0}));
                        report(app, r);
                    }
                    if ui.button(tl!("Reject")).clicked() {
                        let r = app.run("ai.result.reject", json!({"layer":id.0}));
                        report(app, r);
                        state.enabled = false;
                    }
                    if ui.button(tl!("Edit mask")).clicked() {
                        let r = app.run("layer.select", json!({"layer":id.0}));
                        report(app, r);
                        app.ui.mask_target = true;
                        app.ui.vector_mask_target = false;
                        state.enabled = false;
                    }
                });
                ui.add_enabled_ui(app.drag.is_none() && app.ui.transform.is_none(), |ui| {
                    ui.checkbox(&mut state.enabled, tl!("Compare on canvas"));
                });
                ui.horizontal_wrapped(|ui| {
                    for (mode, label) in [
                        (Mode::Vertical, tl!("Vertical")),
                        (Mode::Horizontal, tl!("Horizontal")),
                        (Mode::Overlay, tl!("Overlay")),
                        (Mode::Before, tl!("Before")),
                        (Mode::After, tl!("After")),
                    ] {
                        ui.selectable_value(&mut state.mode, mode, label);
                    }
                });
                if state.mode == Mode::Overlay {
                    ui.add(egui::Slider::new(&mut state.overlay, 0.0..=1.0).text(tl!("Overlay opacity")));
                }
                if matches!(state.mode, Mode::Vertical | Mode::Horizontal) {
                    ui.add(egui::Slider::new(&mut state.position, 0.0..=1.0).text(tl!("Divider")));
                }
                ui.weak(tl!("Before hides this result in the current document"));
                ui.weak(tl!("B: before · A: after · S: split · Esc: finish comparison"));
                ui.collapsing(tl!("Provenance"), |ui| {
                    ui.label(&record.recipe.prompt);
                    ui.monospace(&record.input_digest);
                    ui.monospace(&record.result_digest);
                });
            } else {
                state.enabled = false;
                ui.label(tl!("Select or import a result layer"));
            }
            ui.separator();
            ui.collapsing(tl!("Import a proposed result"), |ui| {
                ui.label(tl!("Result must match the selection bounds or full canvas"));
                ui.label(tl!("Requested edit"));
                ui.text_edit_multiline(&mut state.prompt);
                ui.label(tl!("Material"));
                ui.text_edit_singleline(&mut state.material);
                ui.label(tl!("Lighting"));
                ui.text_edit_singleline(&mut state.lighting);
                ui.label(tl!("Provider attribution"));
                ui.text_edit_singleline(&mut state.provider);
                ui.label(tl!("Model attribution"));
                ui.text_edit_singleline(&mut state.model);
                if ui.button(tl!("Choose result image…")).clicked() {
                    let r = import_dialog(app, &state, None);
                    report(app, r);
                }
                if let Some(id) = state.layer.filter(|id| layers.iter().any(|l| l.0.0 == *id))
                    && ui.button(tl!("Import replacement…")).clicked()
                {
                    let r = import_dialog(app, &state, Some(LayerId(id)));
                    report(app, r);
                }
            });
        });
    state.open = open;
    app.ui.ai_results = state;
}
