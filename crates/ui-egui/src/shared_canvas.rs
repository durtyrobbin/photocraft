//! Camera handoff at the native canvas seam; never touches pixels or GPU caches.
use crate::PhotocraftApp;
use artcraft_canvas::Camera2d;

/// Host handoff waits for pending native tool previews to be committed or
/// cancelled by their own controls; hiding a preview must never discard it.
pub fn can_handoff(app: &PhotocraftApp) -> bool {
    app.drag.is_none()
        && app.live_stroke.is_none()
        && app.ui.text_edit.is_none()
        && app.ui.type_transform.is_none()
        && app.ui.transform.is_none()
        && app.ui.crop_rect.is_none()
        && app.ui.dialogs.is_empty()
        && !app.file_dialog_open()
        && crate::jobs_ui::job_in_view(app).is_none()
}

pub(crate) fn enter(app: &mut PhotocraftApp) {
    if app.shared_canvas.is_none() {
        return;
    }
    app.sync_views();
    let Some(id) = app.session.active().map(|d| d.doc.id.0) else { return };
    let Some(index) = app.session.active_index() else { return };
    let Some(camera) = app.shared_canvas.as_mut().and_then(|b| b.initial_camera(id)) else { return };
    if let Some(view) = app.ui.views.get_mut(index) {
        view.center = camera.center.map(|v| v as f32);
        view.zoom = camera.zoom as f32;
        view.rotation = camera.rotation_degrees as f32;
        view.fit_pending = false;
        view.fill_pending = false;
    }
}

pub(crate) fn leave(app: &PhotocraftApp) {
    let Some(binding) = &app.shared_canvas else { return };
    let Some(id) = app.session.active().map(|d| d.doc.id.0) else { return };
    let Some(index) = app.session.active_index() else { return };
    if let Some(view) = app.ui.views.get(index) {
        binding.publish(id, Camera2d { center: view.center.map(f64::from), zoom: f64::from(view.zoom), rotation_degrees: f64::from(view.rotation) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_camera_preserves_native_commands_and_pending_edits() {
        let mut session = photocraft_engine::Session::new();
        let doc =
            photocraft_doc::Document::new("test", photocraft_geom::Size::new(100, 80), photocraft_color::ColorMode::Rgb, photocraft_color::SampleType::U8);
        let id = doc.id.0;
        session.add_document(doc, None);
        let mut app = PhotocraftApp::new(session, Default::default());
        let shared = artcraft_canvas::SharedView::new(id, 1);
        let camera = Camera2d { center: [30.0, 25.0], zoom: 2.0, rotation_degrees: 17.0 };
        assert!(shared.bind(999).publish(999, camera));
        app.shared_canvas = Some(shared.bind(id));
        enter(&mut app);
        assert_eq!(app.ui.views[0].zoom, 2.0);
        app.ui.views[0].zoom = 3.0;
        enter(&mut app);
        assert_eq!(app.ui.views[0].zoom, 3.0);
        leave(&app);
        assert_eq!(shared.camera().unwrap().zoom, 3.0);
        assert!(can_handoff(&app));
        app.ui.crop_rect = Some([0.0, 0.0, 20.0, 20.0]);
        assert!(!can_handoff(&app));
    }
}
