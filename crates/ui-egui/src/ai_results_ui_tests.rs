use super::*;
use egui_kittest::{Harness, kittest::Queryable};
fn app() -> PhotocraftApp {
    let mut s = photocraft_engine::Session::new();
    s.execute("file.new", json!({"width":160,"height":120,"background":"#304060"})).unwrap();
    let image = photocraft_codecs::Image::from_u8(40, 30, photocraft_codecs::ChannelLayout::Rgba, [220, 130, 30, 255].repeat(1200)).unwrap();
    let bytes = photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &Default::default()).unwrap();
    s.execute("ai.result.import",json!({"data":photocraft_paint::tile::b64_encode(&bytes),"recipe":{"prompt":"Synthetic comparison fixture; no model","region":{"x0":60,"y0":45,"x1":100,"y1":75},"outputSize":[40,30]}})).unwrap();
    let mut app = PhotocraftApp::new(s, Default::default());
    app.ui.ai_results.enabled = true;
    app.ui.tool = crate::state::Tool::Brush;
    app.ui.artist_palette.open = false;
    app.ui.brush_rack.open = false;
    app
}
#[test]
fn result_comparison_native_divider_keys_and_navigation_never_paint_or_recompose() {
    let app = app();
    let doc = app.session.active().unwrap().doc.clone();
    let steps = app.session.active().unwrap().history.past_len();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 900.0)).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
        app
    });
    h.run_steps(5);
    let ctx = h.ctx.clone();
    let old = textures(h.state_mut(), &ctx, 0, None).unwrap();
    let rect = h.state().last_canvas_rect;
    let start = rect.center();
    let end = pos2(rect.left() + rect.width() * 0.8, rect.center().y);
    h.hover_at(start);
    h.run_steps(1);
    h.drag_at(start);
    h.run_steps(1);
    h.hover_at(end);
    h.run_steps(2);
    h.drop_at(end);
    h.run_steps(2);
    assert!(h.state().ui.ai_results.position > 0.75);
    assert!(h.state().drag.is_none());
    assert!(h.state().live_stroke.is_none());
    assert_eq!(h.state().session.active().unwrap().history.past_len(), steps);
    assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &doc));
    assert_eq!(textures(h.state_mut(), &ctx, 0, None).unwrap(), old);
    h.key_press(egui::Key::B);
    h.run_steps(2);
    assert_eq!(h.state().ui.ai_results.mode, Mode::Before);
    h.key_press(egui::Key::A);
    h.run_steps(2);
    assert_eq!(h.state().ui.ai_results.mode, Mode::After);
    h.key_press(egui::Key::S);
    h.run_steps(2);
    assert_eq!(h.state().ui.ai_results.mode, Mode::Vertical);
    assert_eq!(h.state().ui.tool, crate::state::Tool::Brush);
    h.state_mut().ui.ai_results.mode = Mode::Horizontal;
    h.run_steps(1);
    let end = pos2(rect.center().x, rect.top() + rect.height() * 0.7);
    h.drag_at(start);
    h.run_steps(1);
    h.hover_at(end);
    h.run_steps(2);
    h.drop_at(end);
    h.run_steps(2);
    assert!(h.state().ui.ai_results.position > 0.65);
    h.state_mut().ui.views[0].zoom = 2.0;
    h.state_mut().ui.views[0].center = [72.0, 51.0];
    h.state_mut().ui.views[0].rotation = 23.0;
    h.run_steps(2);
    assert_eq!(textures(h.state_mut(), &ctx, 0, None).unwrap(), old, "pan/zoom/rotation must reuse comparison textures");
    h.drag_at(start);
    h.run_steps(1);
    h.key_press(egui::Key::Escape);
    h.run_steps(1);
    h.drop_at(start);
    h.run_steps(2);
    assert!(!h.state().ui.ai_results.enabled);
    assert!(Arc::ptr_eq(&h.state().session.active().unwrap().doc, &doc));
}
#[test]
fn result_comparison_cache_invalidates_after_document_edit_and_panel_accept_reject_undo() {
    let mut app = app();
    app.ui.ai_results.open = true;
    let source = app.session.active().unwrap().doc.layers[0].clone();
    let id = app.session.active().unwrap().active_layer.unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 1000.0)).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
        app
    });
    h.run_steps(6);
    let first_source = h.state().ai_result_caches[0].source.clone();
    h.get_by_label("Accept").click();
    h.run_steps(3);
    assert_eq!(
        h.state().session.active().unwrap().doc.layer(id).unwrap().ai_result.as_ref().unwrap().status,
        photocraft_doc::ai_result::ResultStatus::Accepted
    );
    assert!(!Arc::ptr_eq(&h.state().ai_result_caches[0].source, &first_source));
    h.get_by_label("Reject").click();
    h.run_steps(3);
    assert!(h.state().session.active().unwrap().doc.layer(id).is_none());
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.state().session.active().unwrap().doc.layer(id).is_some());
    assert_eq!(h.state().session.active().unwrap().doc.layers[0], source);
}
#[test]
fn result_comparison_controls_reject_hostile_values_before_applying() {
    for p in [json!({"position":-1}), json!({"position":2}), json!({"overlay":9}), json!({"prompt":"x".repeat(8001)})] {
        let state: State = serde_json::from_value(p).unwrap();
        assert!(state.validate().is_err());
    }
}

#[test]
fn result_comparison_document_coordinate_tool_events_cannot_paint() {
    let mut app = app();
    let before = app.session.active().unwrap().doc.clone();
    let n = app.session.active().unwrap().history.past_len();
    for event in [
        crate::canvas::ToolEvent::Down { x: 50.0, y: 50.0, pressure: 1.0 },
        crate::canvas::ToolEvent::Move { x: 100.0, y: 70.0, pressure: 1.0 },
        crate::canvas::ToolEvent::Up { x: 100.0, y: 70.0 },
    ] {
        crate::canvas::tool_event(&mut app, event, egui::Modifiers::NONE);
    }
    assert!(Arc::ptr_eq(&before, &app.session.active().unwrap().doc));
    assert_eq!(n, app.session.active().unwrap().history.past_len());
    assert!(app.drag.is_none());
    assert!(app.live_stroke.is_none());
}

#[test]
fn result_comparison_invalid_import_text_never_releases_canvas_to_paint() {
    let mut app = app();
    app.ui.ai_results.provider.clear();
    app.ui.ai_results.prompt = "x".repeat(8001);
    let before = app.session.active().unwrap().doc.clone();
    assert!(active(&app, 0));
    for event in [crate::canvas::ToolEvent::Down { x: 50.0, y: 50.0, pressure: 1.0 }, crate::canvas::ToolEvent::Up { x: 50.0, y: 50.0 }] {
        crate::canvas::tool_event(&mut app, event, egui::Modifiers::NONE);
    }
    assert!(Arc::ptr_eq(&before, &app.session.active().unwrap().doc));
    assert!(app.drag.is_none());
}
