#[path = "support/layered_interop.rs"]
mod fixture;
use photocraft_doc::{Layer, LayerContent, SampleType};
use photocraft_io::{ExportOptions, pdf, pdf_layers};
use photocraft_raster::Interrupt;
fn names(layers: &[Layer]) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    for l in layers {
        out.push((l.name.clone(), l.visible));
        if let Some(c) = l.children() {
            out.extend(names(c));
        }
    }
    out
}
#[test]
fn ocg_structure_roundtrip_preserves_independent_layers_and_composite() {
    for depth in [SampleType::U8, SampleType::U16, SampleType::F32] {
        let doc = fixture::document(depth);
        let out = pdf_layers::export_ocg(&[&doc], &mut |_| Ok(())).unwrap();
        let entries = photocraft_io::pdf_ocg_import::inspect(&out.bytes).unwrap();
        assert_eq!(entries.len(), 6);
        assert_eq!(entries.iter().filter(|e| !e.visible).count(), 1);
        assert!(out.bytes.starts_with(b"%PDF-1.5"));
        let result =
            pdf::import_pdf_pages_with("test.pdf", &out.bytes, None, pdf::ImportOptions { resolution: 72., ..Default::default() }, &Interrupt::default())
                .unwrap();
        let children = result.document.layers[0].children().unwrap();
        assert_eq!(names(&doc.layers), names(children));
        let before = photocraft_compose::render(&doc, doc.bounds());
        let after = photocraft_compose::render(&result.document, result.document.bounds());
        for (a, b) in before.px.iter().zip(after.px) {
            for c in 0..4 {
                assert!((a[c] - b[c]).abs() < 0.015, "{a:?} {b:?}");
            }
        }
        assert!(matches!(children[0].content, LayerContent::Raster(_)));
        let id = entries.iter().find(|e| e.name == "Color noise").unwrap().id;
        let selected = pdf::import_pdf_pages_with(
            "one.pdf",
            &out.bytes,
            None,
            pdf::ImportOptions { resolution: 72., ocg_selection: Some(vec![id]), ..Default::default() },
            &Interrupt::default(),
        )
        .unwrap();
        assert_eq!(names(selected.document.layers[0].children().unwrap()), [("Color noise".into(), true)]);
    }
}
#[test]
fn paired_outputs_use_real_layered_psd_and_psb() {
    let doc = fixture::document(SampleType::U16);
    for psb in [false, true] {
        let pdf = pdf_layers::export_ocg(&[&doc], &mut |_| Ok(())).unwrap();
        let package = pdf_layers::paired_package(&[&doc], pdf, psb, &mut |_| Ok(())).unwrap();
        let zip = photocraft_format::zip::ZipReader::new(&package.bytes).unwrap();
        let filename = if psb { "document-001.psb" } else { "document-001.psd" };
        let bytes = zip.read_by_name(filename, 10_000_000).unwrap();
        let restored = photocraft_io::import(filename, &bytes).unwrap().document;
        assert_eq!(names(&doc.layers), names(&restored.layers));
        assert_eq!(restored.depth, doc.depth);
        assert!(restored.layers[2].children().unwrap()[0].mask.is_some());
        assert!(zip.find("binder.pdf").is_some());
        assert!(zip.find("manifest.json").is_some());
    }
}
#[test]
fn optional_content_cancellation_limits_and_backdrop_dependencies_fail() {
    let mut doc = fixture::document(SampleType::U8);
    assert!(pdf_layers::export_ocg(&[&doc], &mut |_| Err("cancelled".into())).is_err());
    doc.layers[1].clipped = true;
    assert!(pdf_layers::export_ocg(&[&doc], &mut |_| Ok(())).unwrap_err().to_string().contains("backdrop"));
    doc.layers[1].clipped = false;
    doc.size.width = 16385;
    assert!(pdf_layers::export_ocg(&[&doc], &mut |_| Ok(())).is_err());
}
#[test]
fn flat_and_layers_as_pages_remain_distinct() {
    let doc = fixture::document(SampleType::U8);
    let flat = pdf::export_pdf_canvases(&[&doc], &ExportOptions::default(), &mut |_| Ok(())).unwrap();
    assert_eq!(pdf::page_sizes(&flat.bytes).unwrap().len(), 1);
    assert!(photocraft_io::pdf_ocg_import::inspect(&flat.bytes).unwrap().is_empty());
    let pages = pdf_layers::export_layers_as_pages(&[&doc], &ExportOptions::default(), &mut |_| Ok(())).unwrap();
    assert_eq!(pdf::page_sizes(&pages.bytes).unwrap().len(), 5);
}

#[test]
fn ocg_preserves_transparent_canvas_and_canvas_artboard_background() {
    for board in [false, true] {
        let mut doc = fixture::document(SampleType::U8);
        doc.layers.remove(0);
        if board {
            let bounds = doc.bounds();
            doc.layers = vec![Layer::new(
                "Canvas board",
                LayerContent::Group(photocraft_doc::Group { children: doc.layers, expanded: true, artboard: Some(photocraft_doc::Artboard::new(bounds)) }),
            )];
        }
        let out = pdf_layers::export_ocg(&[&doc], &mut |_| Ok(())).unwrap();
        let restored =
            pdf::import_pdf_pages_with("test.pdf", &out.bytes, None, pdf::ImportOptions { resolution: 72., ..Default::default() }, &Interrupt::default())
                .unwrap()
                .document;
        let a = photocraft_compose::render(&doc, doc.bounds());
        let b = photocraft_compose::render(&restored, restored.bounds());
        for (a, b) in a.px.iter().zip(b.px) {
            for c in 0..4 {
                assert!((a[c] - b[c]).abs() < 0.02, "{a:?} {b:?}");
            }
        }
    }
}

#[test]
fn complex_psd_keeps_native_type_vector_adjustment_and_nested_groups() {
    let doc = fixture::complex_document(SampleType::U16);
    assert!(pdf_layers::export_ocg(&[&doc], &mut |_| Ok(())).is_err(), "Backdrop adjustment cannot be represented as an independent OCG");
    for ext in ["psd", "psb"] {
        let bytes = photocraft_io::export(&doc, ext, &Default::default()).unwrap().bytes;
        let restored = photocraft_io::import(ext, &bytes).unwrap().document;
        assert_eq!(names(&doc.layers), names(&restored.layers));
        let nested = restored.layers[2].children().unwrap()[2].children().unwrap();
        assert!(matches!(nested[0].content, LayerContent::Shape(_)));
        assert!(matches!(nested[1].content, LayerContent::Text(_)));
        assert!(matches!(restored.layers[4].content, LayerContent::Adjustment(_)));
        assert!(!nested[0].effects.items.is_empty());
    }
}

#[test]
fn empty_groups_count_toward_the_ocg_resource_limit() {
    let mut doc = fixture::document(SampleType::U8);
    doc.layers = (0..4097).map(|i| Layer::group(format!("Group {i}"), Vec::new())).collect();
    assert!(pdf_layers::export_ocg(&[&doc], &mut |_| Ok(())).unwrap_err().to_string().contains("4096"));
}

#[test]
fn ocg_artboard_background_uses_the_document_icc_conversion() {
    use photocraft_cms::Builtin;
    use photocraft_doc::{Artboard, ArtboardBackground, Color, ColorMode, Document, Group, Size};
    for profile in [Builtin::Srgb, Builtin::DisplayP3, Builtin::AdobeRgbCompat, Builtin::ProPhotoCompat, Builtin::LinearSrgb, Builtin::Rec2020] {
        let mut doc = Document::new("Color-managed board", Size::new(8, 8), ColorMode::Rgb, SampleType::U8);
        doc.resolution_dpi = 72.0;
        doc.icc_profile = Some(profile.profile().to_bytes());
        doc.layers.push(Layer::new(
            "Board",
            LayerContent::Group(Group {
                children: vec![],
                expanded: true,
                artboard: Some(Artboard { background: ArtboardBackground::Custom(Color::rgb(0.5, 0.4, 0.3)), ..Artboard::new(doc.bounds()) }),
            }),
        ));
        let flat = pdf::export_pdf_canvases(&[&doc], &Default::default(), &mut |_| Ok(())).unwrap();
        let ocg = pdf_layers::export_ocg(&[&doc], &mut |_| Ok(())).unwrap();
        let render = |bytes: &[u8]| {
            let d = pdf::import_pdf_pages_with("color.pdf", bytes, None, pdf::ImportOptions { resolution: 72., ..Default::default() }, &Interrupt::default())
                .unwrap()
                .document;
            photocraft_compose::render(&d, d.bounds())
        };
        let a = render(&flat.bytes);
        let b = render(&ocg.bytes);
        for (a, b) in a.px.iter().zip(b.px) {
            for c in 0..4 {
                assert!((a[c] - b[c]).abs() < 0.005, "{profile:?}: {a:?} != {b:?}");
            }
        }
    }
}
