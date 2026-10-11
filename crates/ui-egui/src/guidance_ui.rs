//! Semantic guidance overlays. Drawing creates one document transaction on release.
use crate::{PhotocraftApp, canvas::ViewXform};
use egui::{Color32, pos2};
use photocraft_doc::{
    Document,
    guidance::{Point, Semantic, Stroke},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
#[cfg(test)]
#[path = "guidance_ui_tests.rs"]
mod tests;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub open: bool,
    pub show: bool,
    pub drawing: bool,
    pub layer: Option<u32>,
    pub stroke: Option<u32>,
    pub semantic: Semantic,
    pub width: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub pressure_size: bool,
    pub arrow: bool,
    pub annotation: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            show: true,
            drawing: false,
            layer: None,
            stroke: None,
            semantic: Semantic::Highlight,
            width: 12.0,
            hardness: 0.8,
            opacity: 1.0,
            pressure_size: true,
            arrow: false,
            annotation: String::new(),
        }
    }
}
impl State {
    pub fn validate(&self) -> Result<(), String> {
        self.new_stroke(vec![Point { x: 0.0, y: 0.0, pressure: 1.0 }]).validate()
    }
    fn new_stroke(&self, points: Vec<Point>) -> Stroke {
        Stroke {
            id: 1,
            semantic: self.semantic,
            points: Arc::new(points),
            width: self.width,
            hardness: self.hardness,
            opacity: self.opacity,
            pressure_size: self.pressure_size,
            arrow: self.arrow,
            annotation: self.annotation.clone(),
        }
    }
}
pub struct Pending {
    source: Arc<Document>,
    layer: u32,
    stroke: Stroke,
}
fn report(app: &mut PhotocraftApp, result: Result<Value, String>) {
    if let Err(e) = result {
        app.ui.status = e;
        app.ui.status_error = true;
    }
}
pub fn active(app: &PhotocraftApp, idx: usize) -> bool {
    let g = &app.ui.guidance;
    g.open
        && g.drawing
        && g.show
        && !app.ui.ai_results.enabled
        && app.drag.is_none()
        && app.ui.transform.is_none()
        && app.ui.dialogs.is_empty()
        && app.ui.text_edit.is_none()
        && !crate::crop_ui::active(app)
        && app.session.documents().get(idx).is_some_and(|s| photocraft_engine::float_cmds::floating(s).is_none())
}
pub fn tick(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let valid = app.session.active_index().is_some_and(|i| active(app, i));
    if !valid
        || app.ui.guidance.validate().is_err()
        || app.guidance_pending.as_ref().is_some_and(|p| app.session.active().is_none_or(|s| !Arc::ptr_eq(&s.doc, &p.source)))
    {
        if app.guidance_pending.is_some() && ctx.input(|i| i.pointer.primary_down() || i.pointer.primary_released()) {
            app.canvas_input_blocked_until_release = true;
        }
        app.guidance_pending = None;
    }
    if valid && !ctx.text_edit_focused() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.canvas_input_blocked_until_release |= ctx.input(|i| i.pointer.primary_down() || i.pointer.primary_released());
        app.guidance_pending = None;
        app.ui.guidance.drawing = false;
    }
}
/// Shared by the native canvas and document-coordinate automation.
pub fn tool_event(app: &mut PhotocraftApp, event: crate::canvas::ToolEvent) -> bool {
    use crate::canvas::ToolEvent;
    let Some(idx) = app.session.active_index().filter(|&idx| active(app, idx)) else { return false };
    if let Err(e) = app.ui.guidance.validate() {
        app.guidance_pending = None;
        app.ui.status = e;
        app.ui.status_error = true;
        return true;
    }
    let (x, y, pressure) = match event {
        ToolEvent::Down { x, y, pressure } | ToolEvent::Move { x, y, pressure } => (x, y, pressure),
        ToolEvent::Up { x, y } => (x, y, app.stylus.pressure()),
    };
    if !x.is_finite() || !y.is_finite() || !pressure.is_finite() {
        app.guidance_pending = None;
        app.ui.status = "Guidance pointer coordinates and pressure must be finite".into();
        app.ui.status_error = true;
        return true;
    }
    if matches!(event, ToolEvent::Down { .. }) {
        let Some(st) = app.session.documents().get(idx) else { return true };
        if x < 0.0 || y < 0.0 || x >= st.doc.size.width as f64 || y >= st.doc.size.height as f64 {
            return true;
        }
        let id = app.ui.guidance.layer.or_else(|| st.doc.guidance.layers.first().map(|l| l.id));
        let Some(layer) = id.filter(|id| st.doc.guidance.layers.iter().any(|l| l.id == *id && !l.locked && l.visible)) else {
            app.ui.status = tl!("Select an unlocked guidance layer").into();
            app.ui.status_error = true;
            return true;
        };
        let p = Point { x: x as f32, y: y as f32, pressure: pressure.clamp(0.0, 1.0) };
        app.guidance_pending = Some(Pending { source: st.doc.clone(), layer, stroke: app.ui.guidance.new_stroke(vec![p]) });
    } else if let Some(p) = app.guidance_pending.as_mut() {
        if app.session.documents().get(idx).is_none_or(|s| !Arc::ptr_eq(&s.doc, &p.source)) {
            app.guidance_pending = None;
            return true;
        }
        let x = (x as f32).clamp(0.0, p.source.size.width as f32);
        let y = (y as f32).clamp(0.0, p.source.size.height as f32);
        if p.stroke.points.last().is_some_and(|q| (q.x - x).hypot(q.y - y) >= 0.5) {
            if p.stroke.points.len() >= photocraft_doc::guidance::MAX_STROKE_POINTS {
                app.guidance_pending = None;
                app.ui.status = tl!("Guidance stroke limit reached; draw a shorter stroke").into();
                app.ui.status_error = true;
                return true;
            }
            Arc::make_mut(&mut p.stroke.points).push(Point { x, y, pressure: pressure.clamp(0.0, 1.0) });
        }
    }
    if matches!(event, ToolEvent::Up { .. })
        && let Some(p) = app.guidance_pending.take()
    {
        let result = app.run("guidance.stroke.add", json!({"layer":p.layer,"stroke":p.stroke}));
        if let Ok(r) = &result {
            app.ui.guidance.stroke = r["stroke"].as_u64().and_then(|v| u32::try_from(v).ok());
        }
        report(app, result);
    }
    true
}
pub fn pointer(app: &mut PhotocraftApp, ui: &egui::Ui, xf: &ViewXform, response: &egui::Response, _idx: usize) {
    use crate::canvas::ToolEvent;
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let down = ui.input(|i| i.pointer.primary_down());
    let released = ui.input(|i| i.pointer.primary_released());
    let Some(pos) = ui.input(|i| i.pointer.interact_pos()) else { return };
    let [x, y] = xf.to_doc(pos);
    let pressure = app.stylus.pressure();
    if pressed && response.is_pointer_button_down_on() {
        tool_event(app, ToolEvent::Down { x, y, pressure });
    } else if released {
        tool_event(app, ToolEvent::Up { x, y });
    } else if down {
        tool_event(app, ToolEvent::Move { x, y, pressure });
    }
    if app.guidance_pending.is_some() {
        ui.ctx().request_repaint();
    }
}
fn color(s: Semantic) -> Color32 {
    match s {
        Semantic::Highlight => Color32::from_rgb(255, 219, 92),
        Semantic::Shadow => Color32::from_rgb(160, 141, 247),
        Semantic::FoldDirection => Color32::from_rgb(83, 224, 241),
        Semantic::MaterialBoundary => Color32::from_rgb(255, 146, 81),
        Semantic::EdgePreservation => Color32::from_rgb(154, 238, 144),
        Semantic::TextureDirection => Color32::from_rgb(226, 146, 232),
        Semantic::DepthCue => Color32::from_rgb(143, 191, 255),
        Semantic::Reflection => Color32::from_rgb(244, 242, 228),
        Semantic::AreaModify => Color32::from_rgb(60, 218, 166),
        Semantic::AreaProtect => Color32::from_rgb(255, 99, 125),
    }
}
fn draw(painter: &egui::Painter, xf: &ViewXform, s: &Stroke, opacity: f32, selected: bool) {
    if s.points.is_empty() {
        return;
    }
    let scale = (xf.to_screen(1.0, 0.0) - xf.to_screen(0.0, 0.0)).length();
    let col = color(s.semantic).gamma_multiply(s.opacity * opacity);
    let clip = painter.clip_rect();
    for i in 0..s.points.len().saturating_sub(1).max(1) {
        let a = s.points[i];
        let b = s.points[(i + 1).min(s.points.len() - 1)];
        let p = xf.to_screen(a.x, a.y);
        let q = xf.to_screen(b.x, b.y);
        let width = (s.width * scale * if s.pressure_size { ((a.pressure + b.pressure) * 0.5).max(0.01) } else { 1.0 }).max(1.0);
        if !egui::Rect::from_two_pos(p, q).expand(width).intersects(clip) {
            continue;
        }
        if selected {
            painter.line_segment([p, q], egui::Stroke::new(width + 2.0, crate::theme::Tokens::get(painter.ctx()).accent.gamma_multiply(0.6)));
        }
        if p.distance(q) < 0.1 {
            painter.circle_filled(p, width * 0.5, col);
        } else {
            painter.line_segment([p, q], egui::Stroke::new(width, col));
        }
    }
    if s.arrow && s.points.len() > 1 {
        let a = s.points[s.points.len() - 2];
        let b = s.points[s.points.len() - 1];
        let end = xf.to_screen(b.x, b.y);
        let start = xf.to_screen(a.x, a.y);
        let vector = end - start;
        if vector.length() > 0.1 {
            painter.arrow(end - vector.normalized() * 18.0, vector.normalized() * 18.0, egui::Stroke::new(2.0, col));
        }
    }
    if let Some(p) = s.points.first() {
        let pos = xf.to_screen(p.x, p.y);
        if clip.contains(pos) {
            let label = if s.annotation.is_empty() { tl!(s.semantic.label()).to_string() } else { format!("{} · {}", tl!(s.semantic.label()), s.annotation) };
            painter.text(pos + egui::vec2(5.0, -8.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(12.0), col);
        }
    }
}
pub fn paint(app: &PhotocraftApp, painter: &egui::Painter, xf: &ViewXform, idx: usize) {
    if !app.ui.guidance.open || !app.ui.guidance.show || app.ui.ai_results.enabled {
        return;
    }
    let Some(st) = app.session.documents().get(idx) else { return };
    for layer in st.doc.guidance.layers.iter().filter(|l| l.visible) {
        for stroke in layer.strokes.iter() {
            draw(painter, xf, stroke, layer.opacity, app.ui.guidance.layer == Some(layer.id) && app.ui.guidance.stroke == Some(stroke.id));
        }
    }
    if let Some(p) = &app.guidance_pending {
        draw(painter, xf, &p.stroke, 1.0, false);
    }
}
fn semantic_picker(ui: &mut egui::Ui, id: &str, semantic: &mut Semantic) {
    egui::ComboBox::from_id_salt(id).selected_text(tl!(semantic.label())).show_ui(ui, |ui| {
        for s in Semantic::ALL {
            ui.selectable_value(semantic, s, tl!(s.label()));
        }
    });
}
pub fn window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if !app.ui.guidance.open {
        return;
    }
    let mut state = std::mem::take(&mut app.ui.guidance);
    let mut open = state.open;
    let guidance = app.session.active().map(|s| s.doc.guidance.clone()).unwrap_or_default();
    if state.layer.is_none_or(|id| !guidance.layers.iter().any(|l| l.id == id)) {
        state.layer = guidance.layers.first().map(|l| l.id);
        state.stroke = None;
    }
    egui::Window::new(tl!("AI Guidance")).id(egui::Id::new("ai-guidance")).open(&mut open).default_pos(pos2(90.0,110.0)).default_width(320.0).default_height(800.0).vscroll(true).max_height((ctx.content_rect().height()-150.0).max(100.0)).show(ctx,|ui|{
        ui.weak(tl!("Guidance is separate from artwork pixels"));
        ui.checkbox(&mut state.show,tl!("Show guidance"));
        if ui.button(tl!("New guidance layer")).clicked(){let r=app.run("guidance.layer.add",json!({"name":tl!("Guidance")}));if let Ok(v)=&r{state.layer=v["layer"].as_u64().and_then(|v|u32::try_from(v).ok());}report(app,r);}
        egui::ComboBox::from_label(tl!("Guidance layer")).selected_text(guidance.layers.iter().find(|l|Some(l.id)==state.layer).map_or("—",|l|l.name.as_str())).show_ui(ui,|ui|{for l in guidance.layers.iter(){if ui.selectable_value(&mut state.layer,Some(l.id),&l.name).changed(){state.stroke=None;}}});
        if let Some(l)=guidance.layers.iter().find(|l|Some(l.id)==state.layer){
            let mut name=l.name.clone();let mut visible=l.visible;let mut locked=l.locked;let mut opacity=l.opacity;
            if ui.checkbox(&mut locked,tl!("Locked")).changed(){let r=app.run("guidance.layer.set",json!({"layer":l.id,"locked":locked}));report(app,r);}
            ui.add_enabled_ui(!locked,|ui|{
                if ui.text_edit_singleline(&mut name).changed()&&!name.is_empty()&&name!=l.name{let r=app.run("guidance.layer.set",json!({"layer":l.id,"name":name}));report(app,r);}
                if ui.checkbox(&mut visible,tl!("Visible")).changed(){let r=app.run("guidance.layer.set",json!({"layer":l.id,"visible":visible}));report(app,r);}
                let slider=ui.add(egui::Slider::new(&mut opacity,0.0..=1.0).text(tl!("Opacity")));if slider.changed(){let r=app.run("guidance.layer.set",json!({"layer":l.id,"opacity":opacity}));report(app,r);}
                if ui.button(tl!("Delete guidance layer")).clicked(){let r=app.run("guidance.layer.remove",json!({"layer":l.id}));report(app,r);state.drawing=false;}
            });
            ui.separator();
            ui.add_enabled_ui(!locked&&visible&&!app.ui.ai_results.enabled&&app.drag.is_none()&&app.ui.transform.is_none(),|ui|{ui.checkbox(&mut state.drawing,tl!("Draw guidance"));});
            semantic_picker(ui,"guidance-brush-semantic",&mut state.semantic);
            ui.add(egui::Slider::new(&mut state.width,1.0..=512.0).text(tl!("Width")));
            ui.add(egui::Slider::new(&mut state.hardness,0.0..=1.0).text(tl!("Hardness")));
            ui.add(egui::Slider::new(&mut state.opacity,0.0..=1.0).text(tl!("Opacity")));
            ui.checkbox(&mut state.pressure_size,tl!("Pressure controls width"));ui.checkbox(&mut state.arrow,tl!("Direction arrow"));ui.label(tl!("Annotation"));ui.text_edit_singleline(&mut state.annotation);
            ui.separator();
            egui::ComboBox::from_label(tl!("Edit guidance stroke")).selected_text(state.stroke.map_or_else(||"—".into(),|id|format!("#{id}"))).show_ui(ui,|ui|{for s in l.strokes.iter(){ui.selectable_value(&mut state.stroke,Some(s.id),format!("#{} {}",s.id,tl!(s.semantic.label())));}});
            if let Some(stroke)=l.strokes.iter().find(|s|Some(s.id)==state.stroke){ui.add_enabled_ui(!locked,|ui|{
                let mut semantic=stroke.semantic;semantic_picker(ui,"guidance-edit-semantic",&mut semantic);if semantic!=stroke.semantic{let r=app.run("guidance.stroke.set",json!({"layer":l.id,"stroke":stroke.id,"semantic":semantic}));report(app,r);}
                let mut annotation=stroke.annotation.clone();if ui.text_edit_singleline(&mut annotation).changed()&&annotation!=stroke.annotation{let r=app.run("guidance.stroke.set",json!({"layer":l.id,"stroke":stroke.id,"annotation":annotation}));report(app,r);}
                if ui.button(tl!("Apply brush settings to stroke")).clicked(){let r=app.run("guidance.stroke.set",json!({"layer":l.id,"stroke":stroke.id,"width":state.width,"hardness":state.hardness,"opacity":state.opacity,"pressureSize":state.pressure_size,"arrow":state.arrow}));report(app,r);}
                ui.horizontal(|ui|{for(label,delta)in [("←",[-1,0]),("→",[1,0]),("↑",[0,-1]),("↓",[0,1])]{if ui.button(label).on_hover_text(tl!("Move stroke one document pixel")).clicked(){let r=app.run("guidance.stroke.set",json!({"layer":l.id,"stroke":stroke.id,"translate":delta}));report(app,r);}}});
                if ui.button(tl!("Delete guidance stroke")).clicked(){let r=app.run("guidance.stroke.remove",json!({"layer":l.id,"stroke":stroke.id}));report(app,r);state.stroke=None;}
            });}
        }
    });
    state.open = open;
    app.ui.guidance = state;
}
