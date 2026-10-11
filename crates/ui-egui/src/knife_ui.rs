//! Blade options; the canvas uses the existing live mixer preview and commit transaction.
use crate::{PhotocraftApp, state::Tool};
use photocraft_engine::knife_cmds::Settings;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub open: bool,
    pub settings: Settings,
}
impl Default for State {
    fn default() -> Self {
        Self { open: true, settings: Settings::default() }
    }
}

pub fn window(app: &mut PhotocraftApp, ctx: &egui::Context) {
    if app.ui.tool != Tool::PaletteKnife || !app.ui.knife.open {
        return;
    }
    let mut state = std::mem::take(&mut app.ui.knife);
    let mut open = state.open;
    egui::Window::new(tl!("Palette Knife"))
        .id(egui::Id::new("palette-knife"))
        .open(&mut open)
        .vscroll(true)
        .max_height((ctx.content_rect().height() - 150.0).max(64.0))
        .default_pos(egui::pos2(70.0, 95.0))
        .default_width(300.0)
        .show(ctx, |ui| {
            let s = &mut state.settings;
            ui.label(tl!("Alt-click the canvas to load the brush tip"));
            ui.add(egui::Slider::new(&mut s.width, 1.0..=1024.0).logarithmic(true).text(tl!("Width")));
            ui.add(egui::Slider::new(&mut s.angle, -180.0..=180.0).text(tl!("Angle")));
            ui.add(egui::Slider::new(&mut s.contact, 0.05..=1.0).text(tl!("Blade contact")));
            ui.add(egui::Slider::new(&mut s.softness, 0.0..=1.0).text(tl!("Softness")));
            ui.add(egui::Slider::new(&mut s.breakup, 0.0..=1.0).text(tl!("Edge breakup")));
            ui.checkbox(&mut s.follow_direction, tl!("Follow direction"));
            ui.checkbox(&mut s.pressure_contact, tl!("Pressure contact"));
            for (label, value) in [(tl!("Wet"), &mut s.wet), (tl!("Mix"), &mut s.mix), (tl!("Flow"), &mut s.flow), (tl!("Load"), &mut s.load)] {
                ui.add(egui::Slider::new(value, 0.0..=1.0).text(label));
            }
            ui.checkbox(&mut s.sample_all_layers, tl!("Sample All Layers"));
            ui.checkbox(&mut s.clean_after_stroke, tl!("Clean pickup after stroke"));
            ui.checkbox(&mut s.reload_after_stroke, tl!("Reload after stroke"));
            ui.horizontal(|ui| {
                if ui.button(tl!("Load Brush")).clicked() {
                    let _ = app.run("brush.mixer.load", json!({}));
                }
                if ui.button(tl!("Clean Brush")).clicked() {
                    let _ = app.run("brush.mixer.clean", json!({}));
                    s.reload_after_stroke = false;
                }
            });
            if let Err(error) = s.validate() {
                ui.label(error);
            }
        });
    state.open = open;
    app.ui.knife = state;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{ToolEvent, tool_event};
    #[test]
    fn knife_native_drag_shows_real_paint_before_release_and_undoes_once() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width":160,"height":120,"background":"#3080c0"})).unwrap();
        app.run("layer.new.layer", json!({})).unwrap();
        app.ui.tool = Tool::PaletteKnife;
        app.ui.knife.settings.width = 32.0;
        let before = app.session.active().unwrap().doc.clone();
        let steps = app.session.active().unwrap().history.past_len();
        let m = egui::Modifiers::NONE;
        tool_event(&mut app, ToolEvent::Down { x: 20.0, y: 55.0, pressure: 1.0 }, m);
        tool_event(&mut app, ToolEvent::Move { x: 80.0, y: 55.0, pressure: 0.5 }, m);
        assert!(app.live_stroke.is_some());
        assert_eq!(app.session.active().unwrap().history.past_len(), steps);
        tool_event(&mut app, ToolEvent::Up { x: 130.0, y: 55.0 }, m);
        assert_eq!(app.session.active().unwrap().history.past_len(), steps + 1);
        assert_eq!(app.session.journal.last().unwrap().0, "paint.paletteKnife");
        let after = app.session.active().unwrap().doc.clone();
        assert_eq!(after.layers[0], before.layers[0]);
        assert_ne!(after.layers[1], before.layers[1]);
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(*app.session.active().unwrap().doc, *before);
    }
}
