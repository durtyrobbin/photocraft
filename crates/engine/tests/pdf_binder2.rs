use photocraft_doc::{ColorMode, Document, Group, Layer, LayerContent, LayerMask, PixelFormat, Rect, SampleType, Size, Surface};
use photocraft_engine::{
    Session,
    pdf_export_cmds::{BinderExportPlan, Snapshot},
};
use photocraft_io::pdf;
use serde_json::{Value, json};
use std::sync::Arc;

fn layer(name: &str, rgba: [f32; 4]) -> Layer {
    let mut surface = Surface::new(PixelFormat::new(ColorMode::Rgb, SampleType::U8, true));
    surface.write_region(Rect::new(0, 0, 16, 12), &rgba.repeat(16 * 12));
    Layer::new(name, LayerContent::Raster(surface))
}
fn fixture() -> Session {
    let mut session = Session::new();
    for color in [[1., 0., 0., 1.], [0., 1., 0., 1.], [0., 0., 1., 1.]] {
        let mut doc = Document::new("duplicate title", Size::new(16, 12), ColorMode::Rgb, SampleType::U8);
        doc.resolution_dpi = 72.;
        doc.layers.push(layer("base", color));
        let mut hidden =
            Layer::new("hidden parent", LayerContent::Group(Group { children: vec![layer("hidden white", [1.; 4])], expanded: true, artboard: None }));
        hidden.visible = false;
        doc.layers.push(hidden);
        session.add_document(doc, None);
    }
    session
}
fn capture(session: &Session, params: Value) -> Snapshot {
    Snapshot::capture_plan(session, &BinderExportPlan::from_params(session, &params).unwrap()).unwrap()
}
fn page(bytes: &[u8], index: usize) -> Document {
    pdf::import_pdf_pages_with("test.pdf", bytes, Some(&[index]), pdf::ImportOptions { resolution: 72. }, &photocraft_raster::Interrupt::default())
        .unwrap()
        .document
}
fn pixels(doc: &Document) -> Vec<[f32; 4]> {
    photocraft_compose::render(doc, doc.bounds()).px
}
fn compare(expected: &Document, actual: &Document) {
    assert_eq!(expected.size, actual.size);
    for (a, b) in pixels(expected).iter().zip(pixels(actual)) {
        // PDF rasterization composites onto white. Compare the same white-backed appearance.
        for c in 0..3 {
            assert!((a[c] * a[3] + 1. - a[3] - (b[c] * b[3] + 1. - b[3])).abs() < 0.035, "{a:?} != {b:?}");
        }
    }
}
#[test]
fn mandatory_two_selected_tabs_are_exact_composites_and_sources_are_unchanged() {
    let session = fixture();
    let before: Vec<_> = session.documents().iter().map(|s| s.doc.clone()).collect();
    let snapshot = capture(&session, json!({"tabs":[true,true,false],"mode":"flatten","includeAllLayers":true,"includeHiddenLayers":false}));
    let out = snapshot.render(&mut |_| Ok(())).unwrap();
    assert_eq!(pdf::page_sizes(&out.bytes).unwrap(), [(16., 12.), (16., 12.)]);
    for (i, expected) in before.iter().take(2).enumerate() {
        compare(expected, &page(&out.bytes, i));
    }
    for (st, expected) in session.documents().iter().zip(before) {
        assert!(Arc::ptr_eq(&st.doc, &expected));
        assert_eq!(st.doc.layers.len(), 2);
    }
}
#[test]
fn hidden_override_changes_only_snapshot() {
    let session = fixture();
    let out = capture(&session, json!({"tabs":[true,false,false],"includeHiddenLayers":true})).render(&mut |_| Ok(())).unwrap();
    assert!(pixels(&page(&out.bytes, 0)).iter().all(|p| p[0] > 0.97 && p[1] > 0.97 && p[2] > 0.97));
    assert!(!session.documents()[0].doc.layers[1].visible);
}
#[test]
fn invalid_selection_and_unsupported_modes_fail_explicitly() {
    let session = fixture();
    let id = session.documents()[0].doc.id.0;
    for params in [
        json!({"documentIds":[]}),
        json!({"documentIds":[id,id]}),
        json!({"documentIds":[u64::MAX]}),
        json!({"mode":"layersAsPages"}),
        json!({"mode":"preserveLayersOcg"}),
        json!({"includeAllLayers":false}),
    ] {
        assert!(Snapshot::capture_plan(&session, &BinderExportPlan::from_params(&session, &params).unwrap()).is_err());
    }
    for params in [json!({"tabs":[true]}), json!({"tabs":[1,2,3]}), json!({"documentIds":"bad"}), json!({"includeHiddenLayers":"yes"}), json!({"mode":"bogus"})]
    {
        assert!(BinderExportPlan::from_params(&session, &params).is_err());
    }
}
#[test]
fn stable_ids_survive_reorder_switch_close_and_new_tabs() {
    let mut session = fixture();
    let ids: Vec<_> = session.documents().iter().map(|s| s.doc.id.0).collect();
    let snapshot = capture(&session, json!({"documentIds":[ids[2],ids[0]]}));
    let expected = [session.documents()[2].doc.clone(), session.documents()[0].doc.clone()];
    session.execute("document.move", json!({"document":2,"to":0})).unwrap();
    session.set_active(1);
    session.close(0);
    session.add_document(Document::new("new", Size::new(4, 4), ColorMode::Rgb, SampleType::U8), None);
    let out = snapshot.render(&mut |_| Ok(())).unwrap();
    for (i, d) in expected.iter().enumerate() {
        compare(d, &page(&out.bytes, i));
    }
    let result = snapshot.finish(&mut session, "test.pdf", true, out.bytes.len(), out.warnings);
    assert_eq!(result["closed"], 1);
    assert_eq!(session.documents().len(), 2);
}
#[test]
fn cancelled_render_never_calls_writer_or_changes_documents() {
    let session = fixture();
    let mut calls = 0;
    let result = capture(&session, json!({})).render(&mut |_| {
        calls += 1;
        if calls > 2 { Err("cancelled".into()) } else { Ok(()) }
    });
    assert!(result.unwrap_err().to_string().contains("cancelled"));
    assert_eq!(session.documents().len(), 3);
}
#[test]
fn masked_nested_opacity_and_smart_layers_match_document_composite() {
    let mut session = fixture();
    session
        .edit("complex", |doc, _| {
            let mut overlay = layer("overlay", [0.2, 0.3, 0.8, 0.6]);
            overlay.opacity = 0.4;
            overlay.blend = photocraft_doc::BlendMode::Multiply;
            overlay.mask = Some(LayerMask::reveal_all());
            let surface = match &overlay.content {
                LayerContent::Raster(s) => s.clone(),
                _ => unreachable!(),
            };
            let smart = photocraft_doc::SmartObject::new(
                photocraft_doc::SmartSource::Linked { path: "missing-but-cached.png".into() },
                photocraft_geom::Affine::IDENTITY,
                Some(surface),
            );
            doc.layers.push(Layer::new(
                "nested",
                LayerContent::Group(Group { children: vec![overlay, Layer::new("smart", LayerContent::Smart(smart))], expanded: true, artboard: None }),
            ));
            Ok(())
        })
        .unwrap();
    let expected = session.active().unwrap().doc.clone();
    let out = capture(&session, json!({"documentIds":[expected.id.0]})).render(&mut |_| Ok(())).unwrap();
    compare(&expected, &page(&out.bytes, 0));
}
#[test]
fn bounded_many_tab_stress() {
    let mut session = Session::new();
    for index in 0..32 {
        let mut doc = Document::new(format!("Stress {index}"), Size::new(16, 12), ColorMode::Rgb, SampleType::U8);
        for n in 0..8 {
            doc.layers.push(layer("layer", [n as f32 / 8., 0.4, 0.2, 0.5]));
        }
        session.add_document(doc, None);
    }
    let out = capture(&session, json!({})).render(&mut |_| Ok(())).unwrap();
    assert_eq!(pdf::page_sizes(&out.bytes).unwrap().len(), 32);
    assert_eq!(session.documents().len(), 32);
}

#[test]
fn asynchronous_cancel_preserves_existing_target_and_modified_tabs() {
    let root = std::env::temp_dir().join(format!("photocraft-binder2-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("existing.pdf");
    std::fs::write(&path, b"previous destination").unwrap();
    let mut session = fixture();
    let snapshot = capture(&session, json!({}));
    let job = match snapshot.start(&mut session, path.to_string_lossy().into_owned(), true).unwrap() {
        photocraft_engine::jobs::Started::Job(id) => id,
        _ => panic!("native worker expected"),
    };
    session.cancel_job(job);
    session.join_cancelled_jobs();
    session.poll_jobs();
    assert_eq!(std::fs::read(&path).unwrap(), b"previous destination");
    assert_eq!(session.documents().len(), 3);
    let expected = session.active().unwrap().doc.clone();
    let snapshot = capture(&session, json!({}));
    let job = match snapshot.start(&mut session, path.to_string_lossy().into_owned(), true).unwrap() {
        photocraft_engine::jobs::Started::Job(id) => id,
        _ => panic!("native worker expected"),
    };
    session
        .edit("Edit during export", |doc, _| {
            doc.layers.clear();
            Ok(())
        })
        .unwrap();
    let result = session.wait_job(job).unwrap();
    assert_eq!(result["closed"], 2);
    assert_eq!(session.documents().len(), 1);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(pdf::page_sizes(&bytes).unwrap().len(), 3);
    compare(&expected, &page(&bytes, 2));
    assert!(session.documents()[0].doc.layers.is_empty());
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1, "temporary output removed");
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn artboards_stay_one_canvas_page_in_binder_but_split_in_save_as() {
    let mut session = fixture();
    session
        .edit("boards", |doc, _| {
            doc.layers.clear();
            for x in [0, 8] {
                doc.layers.push(Layer::new(
                    "board",
                    LayerContent::Group(Group {
                        children: vec![layer("pixels", [0.2, 0.4, 0.8, 1.])],
                        expanded: true,
                        artboard: Some(photocraft_doc::Artboard::new(Rect::new(x, 0, x + 8, 12))),
                    }),
                ));
            }
            Ok(())
        })
        .unwrap();
    let doc = session.active().unwrap().doc.clone();
    let out = capture(&session, json!({"documentIds":[doc.id.0]})).render(&mut |_| Ok(())).unwrap();
    assert_eq!(pdf::page_sizes(&out.bytes).unwrap(), [(16., 12.)]);
    compare(&doc, &page(&out.bytes, 0));
    let split = pdf::export_pdf(&doc, &Default::default()).unwrap();
    assert_eq!(pdf::page_sizes(&split.bytes).unwrap(), [(8., 12.), (8., 12.)]);
}

#[test]
fn missing_smart_appearance_is_rejected_only_when_included() {
    let mut session = fixture();
    session
        .edit("missing smart", |doc, _| {
            let mut layer = Layer::new(
                "unavailable",
                LayerContent::Smart(photocraft_doc::SmartObject::new(
                    photocraft_doc::SmartSource::Linked { path: "unavailable.psd".into() },
                    photocraft_geom::Affine::IDENTITY,
                    None,
                )),
            );
            layer.visible = false;
            doc.layers.push(layer);
            Ok(())
        })
        .unwrap();
    assert!(Snapshot::capture(&session).is_ok());
    let plan = BinderExportPlan::from_params(&session, &json!({"includeHiddenLayers":true})).unwrap();
    assert!(Snapshot::capture_plan(&session, &plan).is_err());
    let excluded = BinderExportPlan::from_params(&session, &json!({"tabs":[true,true,false],"includeHiddenLayers":true})).unwrap();
    assert!(Snapshot::capture_plan(&session, &excluded).is_ok());
}

#[test]
fn transparent_canvas_and_empty_canvas_export_without_mutating_layers() {
    let mut session = Session::new();
    let mut doc = Document::new("transparent", Size::new(16, 12), ColorMode::Rgb, SampleType::U16);
    doc.resolution_dpi = 72.;
    doc.layers.push(layer("alpha", [0.2, 0.4, 0.8, 0.5]));
    let expected = doc.clone();
    session.add_document(doc, None);
    session.add_document(Document::new("empty", Size::new(16, 12), ColorMode::Rgb, SampleType::U8), None);
    let out = Snapshot::capture(&session).unwrap().render(&mut |_| Ok(())).unwrap();
    compare(&expected, &page(&out.bytes, 0));
    assert_eq!(pdf::page_sizes(&out.bytes).unwrap().len(), 2);
    assert!(session.documents()[1].doc.layers.is_empty());
}

#[test]
#[ignore = "24 MP export benchmark; run explicitly in release"]
fn binder_24mp_export_benchmark() {
    let doc = Document::with_background("24 MP", Size::new(6000, 4000), ColorMode::Rgb, SampleType::U8, photocraft_doc::Color::WHITE);
    let start = std::time::Instant::now();
    let baseline = pdf::export_pdf_documents(&[&doc], &Default::default()).unwrap();
    let baseline_ms = start.elapsed().as_millis();
    let mut session = Session::new();
    session.add_document(doc, None);
    let start = std::time::Instant::now();
    let out = Snapshot::capture(&session).unwrap().render(&mut |_| Ok(())).unwrap();
    let binder_ms = start.elapsed().as_millis();
    assert_eq!(pdf::page_sizes(&out.bytes).unwrap(), pdf::page_sizes(&baseline.bytes).unwrap());
    println!("24 MP solid white: existing export {baseline_ms} ms; binder snapshot and render {binder_ms} ms; {} bytes", out.bytes.len());
}

#[test]
fn close_after_export_keeps_documents_with_pending_edits() {
    let mut session = fixture();
    let snapshot = Snapshot::capture(&session).unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    let job = match session
        .start_job(
            "image.adjustments.invert",
            json!({}),
            "Pending edit",
            true,
            move |ctx| {
                wait.recv_timeout(std::time::Duration::from_secs(5)).map_err(|e| photocraft_engine::EngineError::Other(e.to_string()))?;
                ctx.check()?;
                Ok(())
            },
            |session, ()| {
                session.edit("Pending edit", |doc, _| {
                    doc.layers.clear();
                    Ok(())
                })?;
                Ok(json!({}))
            },
        )
        .unwrap()
    {
        photocraft_engine::jobs::Started::Job(job) => job,
        _ => panic!("native worker expected"),
    };
    let result = snapshot.write(&mut session, "output.pdf", true, |_, _| Ok(())).unwrap();
    assert_eq!(result["closed"], 2, "an unchanged revision does not mean pending work can be discarded");
    assert_eq!(session.documents().len(), 1);
    release.send(()).unwrap();
    session.wait_job(job).unwrap();
    assert!(session.documents()[0].doc.layers.is_empty(), "pending edit completed on the retained document");
}
