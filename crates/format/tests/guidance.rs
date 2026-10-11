//! Hostile native guidance is rejected before it reaches the editor.
use photocraft_doc::{
    ColorMode, Document, SampleType, Size,
    guidance::{Guidance, Layer, Point, Semantic, Stroke},
};
use photocraft_format::{load_from_bytes, read_manifest, save_to_bytes, zip};
use serde_json::{Value, json};
use std::sync::Arc;
fn fixture() -> Vec<u8> {
    let mut doc = Document::new("Guidance", Size::new(64, 64), ColorMode::Rgb, SampleType::U8);
    doc.guidance = Guidance {
        layers: Arc::new(vec![Layer {
            id: 1,
            name: "Intent".into(),
            visible: true,
            locked: false,
            opacity: 1.0,
            strokes: Arc::new(vec![Stroke {
                id: 1,
                semantic: Semantic::AreaProtect,
                points: Arc::new(vec![Point { x: 20.0, y: 30.0, pressure: 1.0 }]),
                width: 12.0,
                hardness: 0.8,
                opacity: 1.0,
                pressure_size: true,
                arrow: false,
                annotation: "Protect the seam".into(),
            }]),
        }]),
    };
    save_to_bytes(&doc, &Default::default()).unwrap()
}
fn modified(bytes: &[u8], change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let m = read_manifest(bytes).unwrap();
    let mut value = serde_json::to_value(m).unwrap();
    change(&mut value);
    let mut writer = zip::ZipWriter::new();
    writer.add("manifest.json", &serde_json::to_vec(&value).unwrap()).unwrap();
    let reader = zip::ZipReader::new(bytes).unwrap();
    for entry in reader.entries.iter().filter(|e| e.name != "manifest.json") {
        writer.add(&entry.name, &reader.read(entry, usize::MAX).unwrap()).unwrap();
    }
    writer.finish().unwrap()
}
#[test]
fn guidance_manifest_absent_defaults_empty_and_roundtrip_keeps_typed_semantics() {
    let bytes = fixture();
    let doc = load_from_bytes(&bytes).unwrap();
    assert_eq!(doc.guidance.layers[0].strokes[0].semantic, Semantic::AreaProtect);
    let legacy = modified(&bytes, |v| {
        v["document"].as_object_mut().unwrap().remove("guidance");
    });
    assert!(load_from_bytes(&legacy).unwrap().guidance.is_empty());
}
#[test]
fn guidance_hostile_manifest_layers_strokes_and_aggregate_limits_fail() {
    let bytes = fixture();
    for (field, value) in [
        ("width", json!(0)),
        ("hardness", json!(2)),
        ("opacity", json!(-1)),
        ("semantic", json!("red")),
        ("annotation", json!("界".repeat(342))),
        ("points", json!([])),
        ("points", json!([{"x":1e30,"y":0,"pressure":1}])),
        ("points", json!(vec![json!({"x":1,"y":2,"pressure":1}); 4097])),
    ] {
        let mutated = modified(&bytes, |v| v["document"]["guidance"]["layers"][0]["strokes"][0][field] = value);
        assert!(load_from_bytes(&mutated).is_err(), "{field}");
    }
    let duplicate = modified(&bytes, |v| {
        let layer = v["document"]["guidance"]["layers"][0].clone();
        v["document"]["guidance"]["layers"].as_array_mut().unwrap().push(layer);
    });
    assert!(load_from_bytes(&duplicate).is_err());
    let many = modified(&bytes, |v| {
        let mut stroke = v["document"]["guidance"]["layers"][0]["strokes"][0].clone();
        stroke["points"] = json!(vec![json!({"x":1,"y":2,"pressure":1}); 4096]);
        let strokes: Vec<_> = (1..=17)
            .map(|id| {
                let mut s = stroke.clone();
                s["id"] = json!(id);
                s
            })
            .collect();
        v["document"]["guidance"]["layers"][0]["strokes"] = json!(strokes);
    });
    assert!(load_from_bytes(&many).is_err());
}
