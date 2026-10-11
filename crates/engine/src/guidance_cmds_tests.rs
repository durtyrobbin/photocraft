use super::*;
use photocraft_color::{ColorMode, SampleType};
fn session() -> Session {
    let mut s = Session::new();
    s.execute("file.new", json!({"width":128,"height":96,"background":"#496782"})).unwrap();
    s
}
fn stroke(semantic: Semantic) -> Value {
    json!({"semantic":semantic,"points":[{"x":20,"y":30,"pressure":0.3},{"x":60,"y":40,"pressure":1.0}],"width":16,"hardness":0.5,"opacity":0.8,"pressureSize":true,"arrow":true,"annotation":"Follow this fold"})
}
fn add(s: &mut Session, semantic: Semantic) -> (u32, u32) {
    let l = s.execute("guidance.layer.add", json!({"name":"Intent"})).unwrap()["layer"].as_u64().unwrap() as u32;
    let st = s.execute("guidance.stroke.add", json!({"layer":l,"stroke":stroke(semantic)})).unwrap()["stroke"].as_u64().unwrap() as u32;
    (l, st)
}
#[test]
fn guidance_all_semantics_native_roundtrip_and_no_artwork_pixel_changes() {
    for mode in [ColorMode::Rgb, ColorMode::Grayscale, ColorMode::Cmyk, ColorMode::Lab] {
        for depth in [SampleType::U8, SampleType::U16, SampleType::F32] {
            let mut s = session();
            s.edit("Format", |d, _| {
                d.mode = mode;
                d.depth = depth;
                for l in &mut d.layers {
                    if let Some(p) = l.surface_mut() {
                        *p = p.convert(photocraft_color::PixelFormat::new(mode, depth, true));
                    }
                }
                Ok(())
            })
            .unwrap();
            let original = s.active().unwrap().doc.clone();
            let pixels = photocraft_compose::render(&original, original.bounds());
            let layer = s.execute("guidance.layer.add", json!({})).unwrap()["layer"].as_u64().unwrap();
            for semantic in Semantic::ALL {
                s.execute("guidance.stroke.add", json!({"layer":layer,"stroke":stroke(semantic)})).unwrap();
            }
            let doc = &s.active().unwrap().doc;
            assert_eq!(original.layers, doc.layers);
            assert!(crate::layer_multi_cmds::same_pixels(&original, doc));
            assert_eq!(pixels.px, photocraft_compose::render(doc, doc.bounds()).px);
            let bytes = photocraft_format::save_to_bytes(doc, &Default::default()).unwrap();
            assert_eq!(photocraft_format::load_from_bytes(&bytes).unwrap().guidance, doc.guidance);
        }
    }
}
#[test]
fn guidance_edit_lock_remove_undo_and_bad_inputs_are_atomic() {
    let mut s = session();
    let (layer, st) = add(&mut s, Semantic::FoldDirection);
    let before = s.active().unwrap().doc.clone();
    let n = s.active().unwrap().history.past_len();
    s.execute("guidance.stroke.set", json!({"layer":layer,"stroke":st,"semantic":"areaProtect","translate":[10,-2],"annotation":"Preserve seam"})).unwrap();
    assert_eq!(s.active().unwrap().history.past_len(), n + 1);
    let changed = s.active().unwrap().doc.clone();
    assert_eq!(changed.guidance.layers[0].strokes[0].points[0].x, 30.0);
    s.execute("edit.undo", json!({})).unwrap();
    assert!(*before == *s.active().unwrap().doc);
    s.execute("edit.redo", json!({})).unwrap();
    assert!(*changed == *s.active().unwrap().doc);
    s.execute("guidance.layer.set", json!({"layer":layer,"locked":true})).unwrap();
    let locked = s.active().unwrap().doc.clone();
    for (cmd, p) in [
        ("guidance.stroke.remove", json!({"layer":layer,"stroke":st})),
        ("guidance.layer.remove", json!({"layer":layer})),
        ("guidance.layer.set", json!({"layer":layer,"name":"Locked rename"})),
        ("guidance.stroke.add", json!({"layer":layer,"stroke":stroke(Semantic::Shadow)})),
    ] {
        assert!(s.execute(cmd, p).is_err());
        assert!(Arc::ptr_eq(&locked, &s.active().unwrap().doc));
    }
    s.execute("guidance.layer.set", json!({"layer":layer,"locked":false})).unwrap();
    let before = s.active().unwrap().doc.clone();
    let n = s.active().unwrap().history.past_len();
    for p in [
        json!({"width":0}),
        json!({"width":513}),
        json!({"opacity":-1}),
        json!({"hardness":2}),
        json!({"points":[]}),
        json!({"points":vec![json!({"x":0,"y":0,"pressure":1});4097]}),
        json!({"points":[{"x":1e30,"y":0,"pressure":1}]}),
        json!({"semantic":"red"}),
        json!({"annotation":"x".repeat(1025)}),
        json!({"translate":[1e30,0]}),
        json!({"id":7}),
    ] {
        let mut p = p;
        p["layer"] = json!(layer);
        p["stroke"] = json!(st);
        assert!(s.execute("guidance.stroke.set", p).is_err());
        assert!(Arc::ptr_eq(&before, &s.active().unwrap().doc));
        assert_eq!(n, s.active().unwrap().history.past_len());
    }
    s.execute("guidance.stroke.remove", json!({"layer":layer,"stroke":st})).unwrap();
    assert!(s.active().unwrap().doc.guidance.layers[0].strokes.is_empty());
    s.execute("edit.undo", json!({})).unwrap();
    assert!(*before == *s.active().unwrap().doc);
}
#[test]
fn guidance_mask_explicit_semantics_pressure_softness_visibility_and_work_cap() {
    let mut s = session();
    let (l, st) = add(&mut s, Semantic::AreaProtect);
    let area = Rect::new(0, 0, 128, 96);
    let g = &s.active().unwrap().doc.guidance;
    let mask = render_mask(g, area, [128, 96], Semantic::AreaProtect).unwrap();
    assert!(mask.iter().any(|&p| p > 0));
    assert!(mask.iter().any(|&p| p > 0 && p < 204));
    assert_eq!(mask[40 * 128 + 60], 204);
    assert_eq!(mask[80 * 128 + 100], 0);
    assert!(render_mask(g, area, [128, 96], Semantic::Highlight).unwrap().iter().all(|&p| p == 0));
    s.execute("guidance.layer.set", json!({"layer":l,"visible":false})).unwrap();
    assert!(render_mask(&s.active().unwrap().doc.guidance, area, [128, 96], Semantic::AreaProtect).unwrap().iter().all(|&p| p == 0));
    s.execute("guidance.layer.set", json!({"layer":l,"visible":true})).unwrap();
    s.execute(
        "guidance.stroke.set",
        json!({"layer":l,"stroke":st,"width":512,"points":(0..4096).map(|i|json!({"x":if i%2==0{0}else{128},"y":48,"pressure":1})).collect::<Vec<_>>()}),
    )
    .unwrap();
    assert!(render_mask(&s.active().unwrap().doc.guidance, area, [128, 96], Semantic::AreaProtect).is_err());
    assert!(render_mask(&Guidance::default(), area, [4096, 4096], Semantic::Highlight).is_err());
}
#[test]
fn guidance_native_validation_legacy_default_and_export_notice() {
    let mut s = session();
    let doc = &s.active().unwrap().doc;
    let old = photocraft_format::save_to_bytes(doc, &Default::default()).unwrap();
    assert!(photocraft_format::load_from_bytes(&old).unwrap().guidance.is_empty());
    add(&mut s, Semantic::Highlight);
    let doc = &s.active().unwrap().doc;
    for ext in ["png", "psd", "pcraft"] {
        let saved = photocraft_io::export(doc, ext, &Default::default()).unwrap();
        assert_eq!(saved.warnings.iter().any(|w| w.contains("AI guidance")), ext != "pcraft");
    }
    let mut bad = (**doc).clone();
    Arc::make_mut(&mut bad.guidance.layers)[0].opacity = f32::NAN;
    assert!(photocraft_format::save_to_bytes(&bad, &Default::default()).is_err());
}
