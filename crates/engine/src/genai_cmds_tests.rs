use super::*;
use photocraft_codecs::{self as codecs, ChannelLayout, Format, Image};
use photocraft_color::{ColorMode, SampleType};
use photocraft_doc::{Document, Size};
use photocraft_genai::{Capabilities, GeneratedImage, GuidedEditRequest, ProviderInfo};
use serde_json::json;
use std::sync::Mutex;
struct Fake {
    local: bool,
    guided: bool,
    broken: bool,
    sent: Mutex<Vec<GuidedEditRequest>>,
}
impl Fake {
    fn new(local: bool) -> Arc<Self> {
        Arc::new(Self { local, guided: true, broken: false, sent: Mutex::new(vec![]) })
    }
}
impl ImageProvider for Fake {
    fn info(&self) -> ProviderInfo {
        ProviderInfo { id: "synthetic", name: "Synthetic test provider", model: "no-model".into(), host: "example.invalid" }
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { edit: true, ..Default::default() }
    }
    fn local_only(&self) -> bool {
        self.local
    }
    fn supports_guidance(&self) -> bool {
        self.guided
    }
    fn endpoint(&self) -> String {
        if self.local { "http://127.0.0.1:1/synthetic".into() } else { "https://example.invalid/synthetic".into() }
    }
    fn edit_guided(&self, r: &GuidedEditRequest) -> photocraft_genai::Result<GeneratedImage> {
        photocraft_genai::guidance::check(r)?;
        self.sent.lock().unwrap().push(r.clone());
        if self.broken {
            return Ok(GeneratedImage { bytes: b"invalid image".to_vec(), mime: "image/png".into() });
        }
        let (w, h) = photocraft_genai::png_size(&r.edit.image).unwrap();
        let img = Image::from_u8(w, h, ChannelLayout::Rgba, [220, 50, 20, 255].repeat((w * h) as usize)).unwrap();
        Ok(GeneratedImage { bytes: codecs::encode(&img, Format::Png, &Default::default()).unwrap(), mime: "image/png".into() })
    }
}
fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"width":64,"height":64,"background":"#ffffff"})).unwrap();
    s
}
fn params() -> Value {
    json!({"recipe":{"prompt":"Synthetic contract fixture","material":"cloth","lighting":"soft","region":{"x0":16,"y0":16,"x1":48,"y1":48},"contextPixels":8,"outputSize":[32,32]}})
}
fn stroke(s: &mut Session, semantic: &str) {
    s.execute("guidance.layer.add", json!({"name":"Intent"})).unwrap();
    s.execute("guidance.stroke.add",json!({"layer":1,"stroke":{"semantic":semantic,"points":[{"x":30,"y":20,"pressure":1},{"x":30,"y":44,"pressure":1}],"width":8,"hardness":1,"opacity":1,"pressureSize":false,"arrow":true,"annotation":"Synthetic direction"}})).unwrap();
}
fn approve(s: &mut Session, p: &Value) {
    let r = s.review_image_edit(p).unwrap();
    s.approve_image_edit(r).unwrap();
}
#[test]
fn remote_grants_are_explicit_bound_and_one_use() {
    let mut s = session();
    let fake = Fake::new(false);
    s.set_image_provider(Some(fake.clone()));
    let p = params();
    let before = s.active().unwrap().doc.clone();
    assert!(s.execute(FILL, p.clone()).is_err());
    assert!(fake.sent.lock().unwrap().is_empty());
    assert!(s.execute("ai.edit.review", p.clone()).is_ok());
    assert!(s.execute(FILL, p.clone()).is_err());
    let mut forged = p.clone();
    forged["approved"] = json!(true);
    assert!(s.execute(FILL, forged).is_err());
    approve(&mut s, &p);
    let mut changed = p.clone();
    changed["recipe"]["prompt"] = json!("changed");
    assert!(s.execute(FILL, changed).is_err());
    assert!(s.execute(FILL, p.clone()).is_err());
    assert!(Arc::ptr_eq(&before, &s.active().unwrap().doc));
    approve(&mut s, &p);
    s.execute(FILL, p.clone()).unwrap();
    assert_eq!(fake.sent.lock().unwrap().len(), 1);
    assert!(s.execute(FILL, p).is_err());
}
#[test]
fn review_expires_when_source_or_provider_changes() {
    let mut s = session();
    s.set_image_provider(Some(Fake::new(false)));
    let p = params();
    let review = s.review_image_edit(&p).unwrap();
    s.execute("select.rect", json!({"x":1,"y":1,"width":40,"height":40})).unwrap();
    assert!(s.approve_image_edit(review).is_err());
    approve(&mut s, &p);
    s.set_image_provider(Some(Fake::new(false)));
    assert!(s.execute(FILL, p).is_err());
}
#[test]
fn local_result_enforces_protection_preserves_source_and_undoes_once() {
    let mut s = session();
    stroke(&mut s, "areaProtect");
    let fake = Fake::new(true);
    s.set_image_provider(Some(fake.clone()));
    let before = s.active().unwrap().doc.clone();
    let history = s.active().unwrap().history.past_len();
    let result = s.execute(FILL, params()).unwrap();
    let id = LayerId(result["layer"].as_u64().unwrap());
    let after = s.active().unwrap().doc.clone();
    let l = after.layer(id).unwrap();
    assert_eq!(before.layers[0], after.layers[0]);
    assert_eq!(before.guidance, after.guidance);
    assert_eq!(s.active().unwrap().history.past_len(), history + 1);
    let mask = &l.mask.as_ref().unwrap().surface;
    assert_eq!(mask.sample_channel(30, 30, 0), 0.0);
    assert_eq!(mask.sample_channel(20, 30, 0), 1.0);
    assert_eq!(mask.sample_channel(5, 5, 0), 0.0);
    let record = l.ai_result.as_ref().unwrap();
    record.validate().unwrap();
    assert_eq!(record.origin, ResultOrigin::LocalProvider);
    assert!(record.generation.is_some());
    assert!(fake.sent.lock().unwrap()[0].guidance.is_empty(), "protection is enforced in the edit mask, not silently sent as unsupported reference images");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*before, *s.active().unwrap().doc);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(*after, *s.active().unwrap().doc);
}
#[test]
fn semantic_metadata_is_sent_explicitly_and_unsupported_guidance_fails_before_call() {
    let mut s = session();
    stroke(&mut s, "foldDirection");
    let fake = Fake::new(true);
    s.set_image_provider(Some(fake.clone()));
    s.execute(FILL, params()).unwrap();
    let sent = fake.sent.lock().unwrap();
    assert_eq!(sent[0].guidance[0].semantic, "foldDirection");
    let a = &sent[0].guidance[0].annotations[0];
    assert!(a.contains("Synthetic direction") && a.contains("directionArrow") && a.contains("start") && a.contains("end"));
    drop(sent);
    let mut s = session();
    stroke(&mut s, "foldDirection");
    let fake = Arc::new(Fake { local: true, guided: false, broken: false, sent: Mutex::new(vec![]) });
    s.set_image_provider(Some(fake.clone()));
    let doc = s.active().unwrap().doc.clone();
    assert!(s.execute(FILL, params()).is_err());
    assert!(fake.sent.lock().unwrap().is_empty());
    assert_eq!(*doc, *s.active().unwrap().doc);
}
#[test]
fn bounded_review_and_invalid_results_are_atomic() {
    let mut s = Session::new();
    s.add_document(Document::new("Huge sparse", Size::new(100_000, 100_000), ColorMode::Rgb, SampleType::F32), None);
    let fake = Fake::new(true);
    s.set_image_provider(Some(fake.clone()));
    let mut p = params();
    p["recipe"]["region"] = json!({"x0":0,"y0":0,"x1":4000,"y1":4000});
    p["recipe"]["outputSize"] = json!([4000, 4000]);
    assert!(s.review_image_edit(&p).is_err());
    assert!(fake.sent.lock().unwrap().is_empty());
    let mut s = session();
    let fake = Arc::new(Fake { local: true, guided: true, broken: true, sent: Mutex::new(vec![]) });
    s.set_image_provider(Some(fake));
    let doc = s.active().unwrap().doc.clone();
    let n = s.active().unwrap().history.past_len();
    assert!(s.execute(FILL, params()).is_err());
    assert_eq!(*doc, *s.active().unwrap().doc);
    assert_eq!(n, s.active().unwrap().history.past_len());
}
#[test]
fn regeneration_hides_previous_result_preserves_old_record_and_uses_source_artwork() {
    let mut s = session();
    let fake = Fake::new(true);
    s.set_image_provider(Some(fake.clone()));
    let first = s.execute(FILL, params()).unwrap()["layer"].as_u64().unwrap();
    let before = s.active().unwrap().doc.clone();
    let mut p = params();
    p["replace"] = json!(first);
    s.execute(FILL, p).unwrap();
    let after = s.active().unwrap().doc.clone();
    assert!(!after.layer(LayerId(first)).unwrap().visible);
    assert_eq!(after.layer(LayerId(first)).unwrap().ai_result, before.layer(LayerId(first)).unwrap().ai_result);
    let sent = fake.sent.lock().unwrap();
    assert_eq!(sent[0].edit.image, sent[1].edit.image, "previous proposal must be excluded from regeneration input");
    drop(sent);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*before, *s.active().unwrap().doc);
}
#[test]
fn generated_layers_preserve_all_supported_native_modes_and_depths() {
    for mode in [ColorMode::Rgb, ColorMode::Grayscale, ColorMode::Cmyk, ColorMode::Lab] {
        for depth in [SampleType::U8, SampleType::U16, SampleType::F32] {
            let mut s = Session::new();
            s.add_document(Document::new("Native format", Size::new(64, 64), mode, depth), None);
            s.set_image_provider(Some(Fake::new(true)));
            let id = LayerId(s.execute(FILL, params()).unwrap()["layer"].as_u64().unwrap());
            let doc = &s.active().unwrap().doc;
            let fmt = doc.layer(id).unwrap().surface().unwrap().format();
            assert_eq!((fmt.mode, fmt.sample), (mode, depth));
        }
    }
}

#[test]
fn generated_request_provenance_roundtrips_and_remains_frozen_after_guidance_edits() {
    let mut s = session();
    stroke(&mut s, "foldDirection");
    s.set_image_provider(Some(Fake::new(true)));
    let id = LayerId(s.execute(FILL, params()).unwrap()["layer"].as_u64().unwrap());
    let doc = s.active().unwrap().doc.clone();
    let record = doc.layer(id).unwrap().ai_result.clone();
    let bytes = photocraft_format::save_to_bytes(&doc, &Default::default()).unwrap();
    assert_eq!(photocraft_format::load_from_bytes(&bytes).unwrap(), *doc);
    s.execute("guidance.stroke.set", json!({"layer":1,"stroke":1,"annotation":"Revised intent"})).unwrap();
    assert_eq!(s.active().unwrap().doc.layer(id).unwrap().ai_result, record);
    let mut invalid = (*doc).clone();
    Arc::make_mut(invalid.layer_mut(id).unwrap().ai_result.as_mut().unwrap()).generation.as_mut().unwrap().input_size = [0, 10];
    assert!(photocraft_format::save_to_bytes(&invalid, &Default::default()).is_err());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn cancelled_blocking_provider_keeps_inflight_limit_and_never_inserts_late_output() {
    use std::sync::Condvar;
    use std::time::{Duration, Instant};
    struct Blocked {
        fake: Arc<Fake>,
        entered: AtomicBool,
        gate: (Mutex<bool>, Condvar),
    }
    impl ImageProvider for Blocked {
        fn info(&self) -> ProviderInfo {
            self.fake.info()
        }
        fn capabilities(&self) -> Capabilities {
            self.fake.capabilities()
        }
        fn local_only(&self) -> bool {
            true
        }
        fn edit_guided(&self, r: &GuidedEditRequest) -> photocraft_genai::Result<GeneratedImage> {
            self.entered.store(true, Ordering::Release);
            let release = self.gate.0.lock().unwrap();
            let (release, timeout) = self.gate.1.wait_timeout_while(release, Duration::from_secs(10), |v| !*v).unwrap();
            assert!(!timeout.timed_out() && *release, "test transport was not released");
            self.fake.edit_guided(r)
        }
    }
    let provider = Arc::new(Blocked { fake: Fake::new(true), entered: AtomicBool::new(false), gate: (Mutex::new(false), Condvar::new()) });
    let mut s = session();
    s.set_image_provider(Some(provider.clone()));
    let before = s.active().unwrap().doc.clone();
    let history = s.active().unwrap().history.past_len();
    let id = match s.start(FILL, params()).unwrap() {
        crate::jobs::Started::Job(id) => id,
        _ => panic!("expected background job"),
    };
    let start = Instant::now();
    while !provider.entered.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(s.cancel_job(id));
    assert!(s.execute(FILL, params()).is_err());
    *provider.gate.0.lock().unwrap() = true;
    provider.gate.1.notify_one();
    let start = Instant::now();
    loop {
        if let Some(event) = s.poll_jobs().into_iter().find(|e| e.id == id) {
            assert_eq!(event.outcome, crate::jobs::JobOutcome::Cancelled);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(*before, *s.active().unwrap().doc);
    assert_eq!(history, s.active().unwrap().history.past_len());
}
