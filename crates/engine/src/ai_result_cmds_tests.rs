use super::*;
use photocraft_codecs::{ChannelLayout, Image};
use photocraft_doc::Size;

fn image() -> Vec<u8> {
    let image = Image::from_u16(4, 3, ChannelLayout::Rgba, &[50000, 10000, 3000, 65535].repeat(12))
        .unwrap()
        .with_icc(Some(photocraft_cms::Builtin::LinearSrgb.profile().to_bytes().as_ref().clone()));
    codecs::encode(&image, Format::Png, &Default::default()).unwrap()
}
fn params() -> Value {
    json!({"data":photocraft_paint::tile::b64_encode(&image()),"recipe":{"prompt":"Test result, not model output","region":{"x0":2,"y0":3,"x1":6,"y1":6},"outputSize":[4,3]},"provider":"Synthetic fixture","model":"none"})
}
fn session(mode: ColorMode, depth: SampleType) -> Session {
    let mut doc = Document::new("Result transaction", Size::new(12, 10), mode, depth);
    let mut source = Layer::raster("Untouched source", PixelFormat::new(mode, depth, true));
    let source_pixels = source.surface_mut().unwrap();
    source_pixels.write_region(Rect::new(0, 0, 12, 10), &vec![0.5; 120 * source_pixels.format().channels()]);
    doc.layers.push(source);
    let mut selection = Surface::new(PixelFormat::new(ColorMode::Grayscale, SampleType::F32, false));
    selection.write_region(Rect::new(2, 3, 6, 6), &[0.0, 0.25, 0.75, 1.0].repeat(3));
    doc.selection = Some(selection);
    let mut s = Session::new();
    s.add_document(doc, None);
    s
}
#[test]
fn result_import_is_masked_profile_converted_source_preserving_and_one_undo() {
    for mode in [ColorMode::Rgb, ColorMode::Grayscale, ColorMode::Cmyk, ColorMode::Lab] {
        for depth in [SampleType::U8, SampleType::U16, SampleType::F32] {
            let mut s = session(mode, depth);
            let before = s.active().unwrap().doc.clone();
            let n = s.active().unwrap().history.past_len();
            let r = s.execute("ai.result.import", params()).unwrap();
            let id = LayerId(r["layer"].as_u64().unwrap());
            let after = s.active().unwrap().doc.clone();
            let l = after.layer(id).unwrap();
            assert_eq!(after.layers[0], before.layers[0]);
            assert_eq!(s.active().unwrap().history.past_len(), n + 1);
            assert_eq!(l.surface().unwrap().format(), PixelFormat::new(mode, depth, true));
            let mask = &l.mask.as_ref().unwrap().surface;
            assert_eq!(mask.sample_channel(2, 3, 0), 0.0);
            assert_eq!(mask.sample_channel(4, 3, 0), 0.75);
            assert_eq!(mask.sample_channel(7, 3, 0), 0.0);
            let record = l.ai_result.as_ref().unwrap();
            assert_eq!(record.origin, ResultOrigin::Imported);
            assert_eq!(record.result_digest, blake3::hash(&image()).to_hex().to_string());
            assert_eq!(record.input_digest, input_digest(&before, record.recipe.region).unwrap());
            if mode == ColorMode::Rgb {
                assert!(l.surface().unwrap().rgba(3, 4)[0] > 0.87, "linear ICC must convert to sRGB");
            }
            s.execute("edit.undo", json!({})).unwrap();
            assert_eq!(*s.active().unwrap().doc, *before);
            s.execute("edit.redo", json!({})).unwrap();
            assert_eq!(*s.active().unwrap().doc, *after);
        }
    }
}
#[test]
fn result_accept_reject_and_replacement_keep_original_provenance_and_undo() {
    let mut s = session(ColorMode::Rgb, SampleType::U16);
    let id = s.execute("ai.result.import", params()).unwrap()["layer"].as_u64().unwrap();
    let first = s.active().unwrap().doc.clone();
    s.execute("ai.result.accept", json!({"layer":id})).unwrap();
    assert_eq!(s.active().unwrap().doc.layer(LayerId(id)).unwrap().ai_result.as_ref().unwrap().status, ResultStatus::Accepted);
    assert_eq!(first.layer(LayerId(id)).unwrap().ai_result.as_ref().unwrap().status, ResultStatus::Proposed);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*s.active().unwrap().doc, *first);
    let mut p = params();
    p["replace"] = json!(id);
    p["recipe"]["prompt"] = json!("Second proposal");
    let replacement = s.execute("ai.result.import", p).unwrap()["layer"].as_u64().unwrap();
    let next = s.active().unwrap().doc.clone();
    assert!(!next.layer(LayerId(id)).unwrap().visible);
    assert!(next.layer(LayerId(replacement)).unwrap().visible);
    assert_eq!(next.layer(LayerId(id)).unwrap().ai_result, first.layer(LayerId(id)).unwrap().ai_result);
    assert_eq!(next.layers[0], first.layers[0]);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*s.active().unwrap().doc, *first);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(*s.active().unwrap().doc, *next);
    s.execute("ai.result.reject", json!({"layer":replacement})).unwrap();
    assert!(s.active().unwrap().doc.layer(LayerId(replacement)).is_none());
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*s.active().unwrap().doc, *next);
}
#[test]
fn result_hostile_imports_fail_atomically_before_document_changes() {
    let mut s = session(ColorMode::Rgb, SampleType::U8);
    let before = s.active().unwrap().doc.clone();
    let count = s.active().unwrap().history.past_len();
    let mut cases = vec![Value::Null, json!({}), json!({"recipe":null})];
    for (key, value) in [("data", json!("garbage")), ("data", json!(false)), ("provider", json!("x".repeat(129))), ("model", json!(42)), ("replace", json!(1))]
    {
        let mut p = params();
        p[key] = value;
        cases.push(p);
    }
    for (key, value) in [
        ("region", json!({"x0":-1,"y0":0,"x1":3,"y1":3})),
        ("region", json!({"x0":0,"y0":0,"x1":i32::MAX,"y1":i32::MAX})),
        ("outputSize", json!([4097, 1])),
        ("contextPixels", json!(1025)),
        ("prompt", json!("x".repeat(8001))),
    ] {
        let mut p = params();
        p["recipe"][key] = value;
        cases.push(p);
    }
    for p in cases {
        assert!(s.execute("ai.result.import", p).is_err());
        assert_eq!(*s.active().unwrap().doc, *before);
        assert_eq!(s.active().unwrap().history.past_len(), count);
    }
}
#[test]
fn result_native_roundtrip_preserves_metadata_pixels_mask_and_legacy_default() {
    let mut s = session(ColorMode::Rgb, SampleType::F32);
    let bytes = photocraft_format::save_to_bytes(&s.active().unwrap().doc, &Default::default()).unwrap();
    assert!(photocraft_format::load_from_bytes(&bytes).unwrap().layers.iter().all(|l| l.ai_result.is_none()));
    s.execute("ai.result.import", params()).unwrap();
    let doc = &s.active().unwrap().doc;
    let bytes = photocraft_format::save_to_bytes(doc, &Default::default()).unwrap();
    assert_eq!(photocraft_format::load_from_bytes(&bytes).unwrap(), **doc);
    let mut invalid = (**doc).clone();
    let record = invalid.layers.last_mut().unwrap().ai_result.as_mut().unwrap();
    Arc::make_mut(record).version = 999;
    assert!(photocraft_format::save_to_bytes(&invalid, &Default::default()).is_err());
}

#[test]
fn result_placement_avoids_group_blending_and_export_discloses_native_metadata_loss() {
    let mut s = session(ColorMode::Rgb, SampleType::U8);
    s.edit("Nested source", |doc, active| {
        let child = doc.layers.remove(0);
        let id = child.id;
        let mut group = Layer::group("Selected group", vec![child]);
        group.opacity = 0.4;
        doc.layers.push(group);
        *active = Some(id);
        Ok(())
    })
    .unwrap();
    let before = s.active().unwrap().doc.clone();
    let id = s.execute("ai.result.import", params()).unwrap()["layer"].as_u64().unwrap();
    let doc = &s.active().unwrap().doc;
    assert_eq!(doc.layers.last().unwrap().id, LayerId(id));
    assert_eq!(doc.layers[0], before.layers[0]);
    for ext in ["psd", "psb", "png", "pcraft"] {
        let exported = photocraft_io::export(doc, ext, &Default::default()).unwrap();
        assert_eq!(exported.warnings.iter().any(|w| w.contains("AI result provenance")), ext != "pcraft");
        let back = photocraft_io::import(&format!("result.{ext}"), &exported.bytes).unwrap().document;
        if ext == "pcraft" {
            assert_eq!(back, **doc);
        }
        if ext == "psd" || ext == "psb" {
            assert_eq!(back.layers.len(), doc.layers.len());
            assert!(back.layers.iter().all(|l| l.ai_result.is_none()));
        }
    }
}
#[test]
fn result_cancelled_background_import_never_applies() {
    let mut s = session(ColorMode::Rgb, SampleType::U8);
    let before = s.active().unwrap().doc.clone();
    let n = s.active().unwrap().history.past_len();
    let crate::jobs::Started::Job(job) = s.start("ai.result.import", params()).unwrap() else { panic!("import should be a job") };
    assert!(s.cancel_job(job));
    assert!(matches!(s.wait_job(job), Err(EngineError::Cancelled)));
    assert_eq!(*s.active().unwrap().doc, *before);
    assert_eq!(s.active().unwrap().history.past_len(), n);
}

#[test]
fn result_special_color_modes_fail_without_changing_document_or_history() {
    for mode in [ColorMode::Indexed, ColorMode::Bitmap, ColorMode::Duotone, ColorMode::Multichannel] {
        let mut s = session(ColorMode::Rgb, SampleType::U8);
        s.edit("Synthetic mode", |doc, _| {
            doc.mode = mode;
            Ok(())
        })
        .unwrap();
        let before = s.active().unwrap().doc.clone();
        let n = s.active().unwrap().history.past_len();
        assert!(s.execute("ai.result.import", params()).is_err());
        assert!(*before == *s.active().unwrap().doc);
        assert_eq!(n, s.active().unwrap().history.past_len());
    }
}
