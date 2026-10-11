use photocraft_doc::{Color, ColorMode, Document, Rect, SampleType, Size};
use photocraft_engine::{
    Session,
    jobs::pdf_page_documents,
    pdf_export_cmds::{BinderExportPlan, Snapshot},
};
use photocraft_io::{ExportOptions, pdf};
use photocraft_raster::Interrupt;
use serde_json::json;
fn source() -> Vec<u8> {
    let docs: Vec<_> = (0..12)
        .map(|i| {
            let mut doc =
                Document::with_background(format!("Source {i}"), Size::new(72, 36), ColorMode::Rgb, SampleType::U8, Color::rgba(i as f32 / 12., 0.3, 0.7, 1.));
            doc.resolution_dpi = 72.;
            doc
        })
        .collect();
    pdf::export_pdf_canvases(&docs.iter().collect::<Vec<_>>(), &ExportOptions::default(), &mut |_| Ok(())).unwrap().bytes
}
#[test]
fn twelve_page_selection_deduplicates_in_source_order_at_three_resolutions() {
    let bytes = source();
    for dpi in [72., 144., 300.] {
        let imported = pdf::import_pdf_pages_with(
            "Portfolio.pdf",
            &bytes,
            Some(&[6, 0, 3, 2, 2]),
            pdf::ImportOptions { resolution: dpi, ..Default::default() },
            &Interrupt::default(),
        )
        .unwrap();
        assert!(imported.source_read_only);
        let docs = pdf_page_documents(&imported.document).unwrap();
        assert_eq!(docs.len(), 4);
        let ids: std::collections::HashSet<_> = docs.iter().map(|d| d.id).collect();
        assert_eq!(ids.len(), 4);
        for (doc, page) in docs.iter().zip([1, 3, 4, 7]) {
            assert_eq!(doc.name, format!("Portfolio - Page {page}.pdf"));
            assert_eq!(doc.size, Size::new(dpi as u32, (dpi / 2.) as u32));
            assert_eq!(doc.resolution_dpi, dpi);
        }
    }
    assert!(
        pdf::import_pdf_pages_with(
            "large.pdf",
            &bytes,
            Some(&[0]),
            pdf::ImportOptions { resolution: f32::INFINITY, ..Default::default() },
            &Interrupt::default()
        )
        .is_err()
    );
    assert!(pdf::import_pdf_pages("none.pdf", &bytes, Some(&[]), &Interrupt::default()).is_err());
    assert_eq!(pdf::page_sizes(&bytes).unwrap().len(), 12);
}
#[test]
fn imported_tabs_crop_paint_undo_redo_and_binder_remain_isolated() {
    let bytes = source();
    let original = bytes.clone();
    let imported = pdf::import_pdf_pages_with(
        "Portfolio.pdf",
        &bytes,
        Some(&[0, 2, 3, 6]),
        pdf::ImportOptions { resolution: 72., ..Default::default() },
        &Interrupt::default(),
    )
    .unwrap();
    let mut s = Session::new();
    for doc in pdf_page_documents(&imported.document).unwrap() {
        s.open_document(doc, None);
        s.active_mut().unwrap().source_read_only = true;
    }
    let before: Vec<_> = s.documents().iter().map(|d| d.doc.clone()).collect();
    s.set_active(0);
    s.execute("image.crop", json!({"x":4,"y":3,"width":40,"height":24,"deleteCroppedPixels":false})).unwrap();
    assert_eq!(s.active().unwrap().doc.size, Size::new(40, 24));
    assert!(s.undo());
    assert_eq!(s.active().unwrap().doc, before[0]);
    assert!(s.redo());
    assert_eq!(s.active().unwrap().doc.size, Size::new(40, 24));
    s.edit("Synthetic paint", |doc, _| {
        let mut layer = photocraft_doc::Layer::raster("Local paint", photocraft_doc::PixelFormat::new(ColorMode::Rgb, SampleType::U8, true));
        layer
            .surface_mut()
            .ok_or_else(|| photocraft_engine::EngineError::Other("missing surface".into()))?
            .write_region(Rect::new(0, 0, 4, 4), &[1., 0., 0., 1.].repeat(16));
        doc.layers.push(layer);
        Ok(())
    })
    .unwrap();
    for (state, prior) in s.documents().iter().zip(&before).skip(1) {
        assert_eq!(&state.doc, prior);
    }
    assert_eq!(bytes, original);
    let ids: Vec<_> = s.documents().iter().map(|d| d.doc.id.0).collect();
    let plan = BinderExportPlan::from_params(&s, &json!({"documentIds":[ids[0],ids[3]]})).unwrap();
    let out = Snapshot::capture_plan(&s, &plan).unwrap().render(&mut |_| Ok(())).unwrap();
    assert_eq!(pdf::page_sizes(&out.bytes).unwrap(), [(40., 24.), (72., 36.)]);
    let reopened =
        pdf::import_pdf_pages_with("binder.pdf", &out.bytes, None, pdf::ImportOptions { resolution: 72., ..Default::default() }, &Interrupt::default())
            .unwrap();
    let pages = pdf_page_documents(&reopened.document).unwrap();
    let painted = photocraft_compose::render(&pages[0], Rect::new(0, 0, 1, 1)).px[0];
    assert!(painted[0] > 0.95 && painted[1] < 0.05);
    s.close(0);
    assert_eq!(s.documents().len(), 3);
}
