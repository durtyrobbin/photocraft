use super::*;
use egui_kittest::{Harness, kittest::Queryable};
fn app() -> PhotocraftApp {
    let mut s = photocraft_engine::Session::new();
    s.execute("file.new", json!({"width":640,"height":480,"background":"#405878"})).unwrap();
    let id = s.execute("guidance.layer.add", json!({"name":"Jacket folds"})).unwrap()["layer"].as_u64().unwrap() as u32;
    let mut app = PhotocraftApp::new(s, Default::default());
    app.ui.guidance = State { open: true, drawing: true, layer: Some(id), semantic: Semantic::FoldDirection, arrow: true, ..Default::default() };
    app.ui.artist_palette.open = false;
    app.ui.brush_rack.open = false;
    app.ui.tool = crate::state::Tool::Brush;
    app
}
#[test]
fn guidance_native_live_overlay_single_undo_and_source_isolation() {
    let app = app();
    let before = app.session.active().unwrap().doc.clone();
    let n = app.session.active().unwrap().history.past_len();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1100.0)).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
        app
    });
    h.run_steps(5);
    let center = h.state().last_canvas_rect.center();
    let end = center + egui::vec2(90.0, 60.0);
    h.hover_at(center);
    h.run_steps(1);
    h.drag_at(center);
    h.run_steps(1);
    h.hover_at(end);
    h.run_steps(2);
    assert!(h.state().guidance_pending.as_ref().is_some_and(|p| p.stroke.points.len() >= 2));
    assert!(Arc::ptr_eq(&before, &h.state().session.active().unwrap().doc));
    assert!(h.state().live_stroke.is_none());
    assert_eq!(n, h.state().session.active().unwrap().history.past_len());
    h.drop_at(end);
    h.run_steps(3);
    assert!(h.state().guidance_pending.is_none());
    let after = h.state().session.active().unwrap().doc.clone();
    assert_eq!(n + 1, h.state().session.active().unwrap().history.past_len());
    assert_eq!(before.layers, after.layers);
    assert_eq!(after.guidance.layers[0].strokes.len(), 1);
    assert_eq!(after.guidance.layers[0].strokes[0].semantic, Semantic::FoldDirection);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(2);
    assert!(*before == *h.state().session.active().unwrap().doc);
    h.state_mut().run("edit.redo", json!({})).unwrap();
    h.run_steps(2);
    assert!(*after == *h.state().session.active().unwrap().doc);
    h.get_by_label("Delete guidance stroke").click();
    h.run_steps(3);
    assert!(h.state().session.active().unwrap().doc.guidance.layers[0].strokes.is_empty());
    assert_eq!(before.layers, h.state().session.active().unwrap().doc.layers);
}
#[test]
fn guidance_native_escape_and_changed_document_cancel_pending_stroke() {
    let app = app();
    let before = app.session.active().unwrap().doc.clone();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1100.0)).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
        app
    });
    h.run_steps(4);
    let p = h.state().last_canvas_rect.center();
    h.hover_at(p);
    h.run_steps(1);
    h.drag_at(p);
    h.run_steps(1);
    assert!(h.state().guidance_pending.is_some());
    h.key_press(egui::Key::Escape);
    h.run_steps(1);
    h.drop_at(p);
    h.run_steps(2);
    assert!(h.state().guidance_pending.is_none());
    assert!(!h.state().ui.guidance.drawing);
    assert!(Arc::ptr_eq(&before, &h.state().session.active().unwrap().doc));
    h.state_mut().ui.guidance.drawing = true;
    h.run_steps(1);
    h.drag_at(p);
    h.run_steps(1);
    assert!(h.state().guidance_pending.is_some());
    h.state_mut().run("guidance.layer.add", json!({"name":"Another intent"})).unwrap();
    h.run_steps(1);
    h.drop_at(p);
    h.run_steps(2);
    assert!(h.state().guidance_pending.is_none());
    assert!(h.state().session.active().unwrap().doc.guidance.layers.iter().all(|l| l.strokes.is_empty()));
    assert_eq!(before.layers, h.state().session.active().unwrap().doc.layers);
}
#[test]
fn guidance_controls_reject_hostile_brush_settings() {
    for p in [json!({"width":0}), json!({"width":513}), json!({"opacity":-1}), json!({"hardness":2}), json!({"annotation":"x".repeat(1025)})] {
        let state: State = serde_json::from_value(p).unwrap();
        assert!(state.validate().is_err());
    }
}

#[test]
fn guidance_automation_routes_to_semantic_strokes_instead_of_paint() {
    let mut app = app();
    let before = app.session.active().unwrap().doc.clone();
    let n = app.session.active().unwrap().history.past_len();
    for event in [crate::canvas::ToolEvent::Down { x: 100.0, y: 100.0, pressure: 0.3 }, crate::canvas::ToolEvent::Move { x: 180.0, y: 150.0, pressure: 0.9 }] {
        crate::canvas::tool_event(&mut app, event, egui::Modifiers::NONE);
    }
    assert!(Arc::ptr_eq(&before, &app.session.active().unwrap().doc));
    assert!(app.guidance_pending.is_some());
    crate::canvas::tool_event(&mut app, crate::canvas::ToolEvent::Up { x: 180.0, y: 150.0 }, egui::Modifiers::NONE);
    assert_eq!(before.layers, app.session.active().unwrap().doc.layers);
    assert_eq!(n + 1, app.session.active().unwrap().history.past_len());
    assert_eq!(app.session.active().unwrap().doc.guidance.layers[0].strokes[0].points[1].pressure, 0.9);
    assert!(app.live_stroke.is_none());
    assert!(app.drag.is_none());
}

#[test]
fn guidance_invalid_brush_text_cannot_fall_back_to_ordinary_paint() {
    let mut app = app();
    app.ui.guidance.annotation = "界".repeat(342);
    let before = app.session.active().unwrap().doc.clone();
    assert!(active(&app, 0));
    crate::canvas::tool_event(&mut app, crate::canvas::ToolEvent::Down { x: 100.0, y: 100.0, pressure: 1.0 }, egui::Modifiers::NONE);
    crate::canvas::tool_event(&mut app, crate::canvas::ToolEvent::Up { x: 100.0, y: 100.0 }, egui::Modifiers::NONE);
    assert!(Arc::ptr_eq(&before, &app.session.active().unwrap().doc));
    assert!(app.ui.status_error);
    assert!(app.guidance_pending.is_none());
    assert!(app.drag.is_none());
}
