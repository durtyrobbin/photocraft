//! Native flick controls. Pixels are committed by the engine, independently of feedback animation.
use crate::{PhotocraftApp, state::Tool, theme::Tokens};
use photocraft_engine::paint::{TipShape, splatter::Settings};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub open: bool,
    pub settings: Settings,
    pub gesture: bool,
    pub animate: bool,
    pub reduced_motion: bool,
    #[serde(skip)]
    flicked: bool,
    #[serde(skip)]
    animation_start: Option<f64>,
    #[serde(skip)]
    pub context: Option<(photocraft_engine::doc::DocId, u64)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            open: true,
            settings: Settings::default(),
            gesture: false,
            animate: true,
            reduced_motion: false,
            flicked: false,
            animation_start: None,
            context: None,
        }
    }
}

pub fn flick(app: &mut PhotocraftApp, center: [f64; 2], end: Option<[f64; 2]>, pressure: f32) {
    if end.is_some() && (app.ui.tool != Tool::Splatter || app.ui.splatter.context.take() != app.session.active().map(|st| (st.doc.id, st.revision))) {
        return;
    }
    let mut settings = app.ui.splatter.settings.clone();
    if let Some(end) = end {
        let (dx, dy) = (end[0] - center[0], end[1] - center[1]);
        let distance = dx.hypot(dy);
        if distance > 1.0 {
            settings.angle = dy.atan2(dx).to_degrees() as f32;
            settings.distance = (distance as f32).clamp(1.0, 1024.0);
            settings.paint_load *= (distance as f32 / 128.0).clamp(0.1, 1.0);
        }
    }
    let target = crate::canvas::paint_target(app);
    if app.run("paint.splatter", json!({"center":center,"pressure":pressure,"settings":settings,"target":target})).is_ok() {
        app.ui.splatter.flicked = true;
    }
}

pub fn cancel_on_context_change(app: &mut PhotocraftApp, ctx: &egui::Context) -> bool {
    if !app.drag.as_ref().is_some_and(|d| d.tool == Tool::Splatter) {
        return false;
    }
    if app.ui.tool != Tool::Splatter
        || app.ui.splatter.context != app.session.active().map(|st| (st.doc.id, st.revision))
        || ctx.input(|i| !i.focused)
        || ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        app.drag = None;
        app.ui.splatter.context = None;
        return true;
    }
    false
}

pub fn window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if app.ui.tool != Tool::Splatter || !app.ui.splatter.open {
        return;
    }
    let mut state = std::mem::take(&mut app.ui.splatter);
    // Saved view state can predate validation; never construct inverted slider ranges.
    state.settings.min_size = if state.settings.min_size.is_finite() { state.settings.min_size.clamp(0.5, 128.0) } else { 1.0 };
    state.settings.max_size =
        if state.settings.max_size.is_finite() { state.settings.max_size.clamp(state.settings.min_size, 128.0) } else { state.settings.min_size };
    let mut open = state.open;
    let tokens = Tokens::get(ctx);
    let now = ctx.input(|i| i.time);
    if std::mem::take(&mut state.flicked) {
        state.animation_start = Some(now);
    }
    egui::Window::new(tl!("Flick / Splatter"))
        .id(egui::Id::new("splatter-settings"))
        .open(&mut open)
        .default_pos(egui::pos2(70.0, 95.0))
        .default_width(330.0)
        .show(ctx, |ui| {
            ui.label(tl!("Click to flick; enable gesture to drag direction"));
            ui.checkbox(&mut state.gesture, tl!("Gesture direction and intensity"));
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.animate, tl!("Animate flick"));
                ui.checkbox(&mut state.reduced_motion, tl!("Reduced motion"));
            });
            // Original geometric hand/brush illustration; the animation never drives painting.
            let (r, _) = ui.allocate_exact_size(egui::vec2(300.0, 55.0), egui::Sense::hover());
            let elapsed = state.animation_start.map(|t| now - t).unwrap_or(1.0);
            let motion = if state.animate && !state.reduced_motion && (0.0..0.3).contains(&elapsed) {
                ctx.request_repaint();
                (1.0 - elapsed as f32 / 0.3) * 14.0
            } else {
                0.0
            };
            let origin = r.min + egui::vec2(24.0, 30.0);
            ui.painter().line_segment([origin, origin + egui::vec2(80.0, 0.0)], egui::Stroke::new(8.0, tokens.text));
            ui.painter().rect_filled(egui::Rect::from_min_size(origin + egui::vec2(80.0, -9.0 + motion), egui::vec2(30.0, 18.0)), 3.0, tokens.accent);
            ui.painter().line_segment([origin + egui::vec2(98.0, 22.0), origin + egui::vec2(98.0, 8.0 + motion)], egui::Stroke::new(7.0, tokens.text));
            ui.painter().circle_filled(origin + egui::vec2(98.0, 23.0), 8.0, tokens.text);
            for i in 0..6 {
                ui.painter().circle_filled(origin + egui::vec2(130.0 + i as f32 * 14.0, ((i * 7) % 13) as f32 - 6.0), 2.0 + (i % 3) as f32, tokens.accent);
            }
            let s = &mut state.settings;
            ui.add(egui::Slider::new(&mut s.count, 0..=2048).text(tl!("Droplet count")));
            ui.add(egui::Slider::new(&mut s.min_size, 0.5..=s.max_size.max(0.5)).text(tl!("Minimum size")));
            ui.add(egui::Slider::new(&mut s.max_size, s.min_size..=128.0).text(tl!("Maximum size")));
            ui.add(egui::Slider::new(&mut s.size_variation, 0.0..=1.0).text(tl!("Size variation")));
            ui.add(egui::Slider::new(&mut s.distance, 0.0..=1024.0).text(tl!("Distance")));
            ui.add(egui::Slider::new(&mut s.angle, -180.0..=180.0).text(tl!("Angle")));
            ui.add_enabled_ui(s.distribution.is_none(), |ui| {
                ui.add(egui::Slider::new(&mut s.spread, 0.0..=360.0).text(tl!("Spread")));
                ui.add(egui::Slider::new(&mut s.directional_bias, 0.0..=1.0).text(tl!("Directional bias")));
            });
            for (label, value) in [
                (tl!("Density"), &mut s.density),
                (tl!("Clustering"), &mut s.clustering),
                (tl!("Opacity variation"), &mut s.opacity_variation),
                (tl!("Color variation"), &mut s.color_variation),
                (tl!("Paint Load"), &mut s.paint_load),
                (tl!("Edge breakup"), &mut s.edge_breakup),
                (tl!("Secondary droplets"), &mut s.secondary),
            ] {
                ui.add(egui::Slider::new(value, 0.0..=1.0).text(label));
            }
            ui.horizontal(|ui| {
                ui.label(tl!("Random Seed"));
                ui.add(egui::DragValue::new(&mut s.seed));
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button(tl!("Design Tip…")).clicked() {
                    let mut brush = app.session.tools.brush.clone();
                    if let Some(mask) = &s.distribution {
                        brush.tip = TipShape::Sampled(mask.clone());
                    }
                    app.ui.tip_designer = crate::tip_designer::State::from_brush(&brush);
                }
                let mask = app.session.tools.brush.tip.bitmap().filter(|m| m.is_valid() && m.width <= 512 && m.height <= 512);
                if ui.add_enabled(mask.is_some(), egui::Button::new(tl!("Use tip distribution"))).clicked() {
                    s.distribution = mask.cloned();
                }
                if ui.button(tl!("Reset distribution")).clicked() {
                    s.distribution = None;
                }
            });
            if s.distribution.is_some() {
                ui.label(tl!("Custom distribution replaces spread and bias"));
            }
            if let Err(error) = s.validate() {
                ui.colored_label(tokens.accent, error);
            }
        });
    state.open = open;
    app.ui.splatter = state;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{ToolEvent, tool_event};
    #[test]
    fn splatter_gesture_cancels_on_focus_tool_document_or_escape() {
        for mode in 0..4 {
            let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
            app.run("file.new", json!({"width":64,"height":64})).unwrap();
            app.ui.tool = Tool::Splatter;
            app.ui.splatter.gesture = true;
            tool_event(&mut app, ToolEvent::Down { x: 12.0, y: 12.0, pressure: 1.0 }, egui::Modifiers::NONE);
            if mode == 1 {
                app.ui.tool = Tool::Brush;
            }
            if mode == 2 {
                app.run("file.new", json!({"width":64,"height":64})).unwrap();
            }
            let before = app.session.active().unwrap().doc.clone();
            let ctx = egui::Context::default();
            let mut raw = egui::RawInput { focused: mode != 0, ..Default::default() };
            if mode == 3 {
                raw.events.push(egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            ctx.begin_pass(raw);
            assert!(cancel_on_context_change(&mut app, &ctx));
            ctx.end_pass().textures_delta.clear(); // This input-only test has no renderer.
            tool_event(&mut app, ToolEvent::Up { x: 40.0, y: 40.0 }, egui::Modifiers::NONE);
            assert_eq!(*app.session.active().unwrap().doc, *before);
        }
    }

    #[test]
    fn splatter_native_controls_accept_a_drawn_distribution_and_persist() {
        use egui_kittest::{Harness, kittest::Queryable};
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.ui.tool = Tool::Splatter;
        let tile = photocraft_engine::paint::GrayTile::from_fn(32, 32, |x, y| if x < 8 && y < 8 { 1.0 } else { 0.0 });
        app.run("tools.setBrush", json!({"brush":{"tip":TipShape::Sampled(tile.clone())}})).unwrap();
        let mut h = Harness::builder().with_size(egui::vec2(1000.0, 900.0)).build_ui_state(
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
        h.get_by_label("Use tip distribution").click();
        h.run_steps(2);
        assert_eq!(h.state().ui.splatter.settings.distribution, Some(tile));
        h.get_by_label("Reduced motion").click();
        h.run_steps(2);
        let mut stored = h.state().ui.splatter.clone();
        stored.context = None;
        assert_eq!(serde_json::from_str::<State>(&serde_json::to_string(&stored).unwrap()).unwrap(), stored);
        h.get_by_label("Reset distribution").click();
        h.run_steps(2);
        assert!(h.state().ui.splatter.settings.distribution.is_none());
    }
    #[test]
    fn splatter_click_paints_on_down_once_and_gesture_uses_direction() {
        for gesture in [false, true] {
            let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
            app.run("file.new", json!({"width":256,"height":256})).unwrap();
            app.run("layer.new.layer", json!({"name":"Paint"})).unwrap();
            app.ui.tool = Tool::Splatter;
            app.ui.splatter.gesture = gesture;
            let before = app.session.active().unwrap().doc.clone();
            let steps = app.session.active().unwrap().history.past_len();
            let m = egui::Modifiers::NONE;
            tool_event(&mut app, ToolEvent::Down { x: 128.0, y: 128.0, pressure: 0.7 }, m);
            assert_eq!(app.session.active().unwrap().history.past_len(), steps + usize::from(!gesture));
            tool_event(&mut app, ToolEvent::Move { x: 128.0, y: 200.0, pressure: 0.8 }, m);
            tool_event(&mut app, ToolEvent::Up { x: 128.0, y: 224.0 }, m);
            assert_eq!(app.session.active().unwrap().history.past_len(), steps + 1);
            let p = &app.session.journal.last().unwrap().1;
            if gesture {
                assert_eq!(p["settings"]["angle"], 90.0);
                assert_eq!(p["settings"]["distance"], 96.0);
            }
            let encoded = serde_json::to_string(&app.ui.splatter).unwrap();
            assert_eq!(serde_json::from_str::<State>(&encoded).unwrap().settings, app.ui.splatter.settings);
            app.run("edit.undo", json!({})).unwrap();
            assert_eq!(*app.session.active().unwrap().doc, *before);
        }
    }
}
