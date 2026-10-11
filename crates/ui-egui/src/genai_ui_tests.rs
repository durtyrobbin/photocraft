use super::*;
use photocraft_genai::{Capabilities, GeneratedImage, GuidedEditRequest, ImageProvider, ProviderInfo};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Fake(Arc<AtomicUsize>);
impl ImageProvider for Fake {
    fn info(&self) -> ProviderInfo {
        ProviderInfo { id: "fixture", name: "Synthetic remote", model: "no-model".into(), host: "example.invalid" }
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { edit: true, ..Default::default() }
    }
    fn edit_guided(&self, r: &GuidedEditRequest) -> photocraft_genai::Result<GeneratedImage> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(GeneratedImage { bytes: r.edit.image.clone(), mime: "image/png".into() })
    }
}

#[test]
fn control_confirmation_is_rejected_even_without_a_shell_authorization_hook() {
    let (mut app, calls) = app();
    let fields = reviewed(&mut app);
    let id = app.ui.dialogs.last().unwrap().id;
    assert!(app.services.automation_authorize.is_none());
    assert!(!app.automation_input);
    let (request, _) = crate::control::ControlRequest::new("ui.dialog.confirm", json!({"dialog":id}));
    let outcome = crate::control::handle(&mut app, &egui::Context::default(), &request);
    match outcome {
        crate::control::Outcome::Done(v) => assert_eq!(v["ok"], false),
        _ => panic!("confirmation should fail synchronously"),
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let params = request_params(&mut app, &fields).unwrap();
    assert!(app.session.execute(FILL, params).is_err());
}
fn app() -> (PhotocraftApp, Arc<AtomicUsize>) {
    let mut s = photocraft_engine::Session::new();
    s.execute("file.new", json!({"width":64,"height":64})).unwrap();
    let mut app = PhotocraftApp::new(s, Default::default());
    let calls = Arc::new(AtomicUsize::new(0));
    app.session.set_image_provider(Some(Arc::new(Fake(calls.clone()))));
    (app, calls)
}
fn reviewed(app: &mut PhotocraftApp) -> Map<String, Value> {
    let id = open_prompt(app, false);
    let fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
    let p = request_params(app, &fields).unwrap();
    let review = app.session.review_image_edit(&p).unwrap();
    app.genai.review = Some((p, review));
    fields
}
#[test]
fn automated_confirmation_cannot_create_a_remote_grant() {
    let (mut app, calls) = app();
    let fields = reviewed(&mut app);
    app.automation_input = true;
    assert!(confirm(&mut app, &fields).unwrap_err().contains("native confirmation"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    app.automation_input = false;
    let p = request_params(&mut app, &fields).unwrap();
    assert!(app.session.execute(FILL, p).is_err());
    let fields = reviewed(&mut app);
    app.session.authorize = Some(|_, _| Ok(()));
    assert!(confirm(&mut app, &fields).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[test]
fn changing_reviewed_fields_or_cancelling_discards_confirmation() {
    let (mut app, calls) = app();
    let mut fields = reviewed(&mut app);
    fields.insert("prompt".into(), json!("changed"));
    assert!(confirm(&mut app, &fields).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let fields = reviewed(&mut app);
    cancelled(&mut app);
    assert!(confirm(&mut app, &fields).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn request_region_follows_selection_and_document_revisions() {
    let (mut app, _) = app();
    let fields = reviewed(&mut app);
    assert_eq!(request_params(&mut app, &fields).unwrap()["recipe"]["outputSize"], json!([64, 64]));
    app.session.execute("select.rect", json!({"x":8,"y":10,"width":16,"height":20})).unwrap();
    let next = request_params(&mut app, &fields).unwrap();
    assert_eq!(next["recipe"]["region"], json!({"x0":8,"y0":10,"x1":24,"y1":30}));
    app.session.execute("file.new", json!({"width":32,"height":48})).unwrap();
    assert_eq!(request_params(&mut app, &fields).unwrap()["recipe"]["outputSize"], json!([32, 48]));
}
#[test]
fn direct_native_confirmation_starts_the_reviewed_synthetic_job() {
    use std::time::{Duration, Instant};
    let (mut app, calls) = app();
    let fields = reviewed(&mut app);
    let before = app.session.active().unwrap().doc.layers.len();
    confirm(&mut app, &fields).unwrap();
    let start = Instant::now();
    while app.session.active().unwrap().doc.layers.len() == before {
        let events = app.session.poll_jobs();
        for e in events {
            assert!(matches!(e.outcome, photocraft_engine::jobs::JobOutcome::Done(_)), "{e:?}");
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(app.session.active().unwrap().doc.layers.len(), before + 1);
    assert!(confirm(&mut app, &fields).is_err());
}
