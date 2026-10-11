//! The same import path serves File > Open and dropped PDF files. Save As must write a
//! readable PDF without acquiring an overwrite path to the original during import.
use photocraft_doc::{Color, ColorMode, Document, SampleType, Size};
use photocraft_io::{ExportOptions, export, import};
use photocraft_ui_egui::{PhotocraftApp, Services};
use std::{cell::RefCell, rc::Rc};

#[test]
fn combined_pdf_dialog_cancel_failure_and_close_are_safe() {
    use photocraft_ui_egui::{FileDialogAnswer, export_dialog};
    for (cancel, fail, close) in [(true, false, true), (false, true, true), (false, false, false), (false, false, true)] {
        let services = Services {
            file_dialog: Some(Box::new(move |_, _, reply| reply.send(if cancel { None } else { Some(FileDialogAnswer::SaveTo("combined.pdf".into())) }))),
            write: Some(Box::new(move |_, bytes| {
                assert!(bytes.starts_with(b"%PDF-"));
                if fail { Err("disk full".into()) } else { Ok(()) }
            })),
            ..Default::default()
        };
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), services);
        app.session.add_document(Document::with_background("edited", Size::new(24, 12), ColorMode::Rgb, SampleType::U8, Color::WHITE), None);
        app.session.active_mut().unwrap().revision += 1;
        let before = app.session.active().unwrap().doc.clone();
        let id = export_dialog::open_all_pdf(&mut app).unwrap();
        let mut fields = app.ui.dialogs.iter().find(|d| d.id == id).unwrap().fields.clone();
        assert_eq!(fields["closeAfter"], false);
        fields.insert("closeAfter".into(), serde_json::json!(close));
        export_dialog::confirm(&mut app, &fields).unwrap();
        assert_eq!(app.session.documents().len(), 1, "never close before a file is written");
        app.poll_file_dialog(&egui::Context::default(), None);
        if !cancel && !fail && close {
            assert!(app.session.documents().is_empty());
        } else {
            assert_eq!(app.session.active().unwrap().doc, before);
            assert!(app.session.active().unwrap().is_dirty());
        }
        if fail {
            assert!(app.ui.status.contains("disk full"));
        }
    }
}

fn binder() -> Vec<u8> {
    use photocraft_doc::{Artboard, Group, Layer, LayerContent, Rect};
    let mut doc = Document::new("binder", Size::new(72, 180), ColorMode::Rgb, SampleType::U8);
    for i in (0..3).rev() {
        doc.layers.push(Layer::new(
            format!("Page {}", i + 1),
            LayerContent::Group(Group { children: vec![], expanded: true, artboard: Some(Artboard::new(Rect::new(0, i * 60, 72, i * 60 + 36))) }),
        ));
    }
    export(&doc, "pdf", &ExportOptions::default()).unwrap().bytes
}

#[test]
fn picker_places_in_original_document_and_retains_state_on_failure() {
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
    app.session.execute("file.new", serde_json::json!({"width": 100, "height": 80})).unwrap();
    let target = app.session.active().unwrap().doc.id;
    let before = app.session.active().unwrap().doc.clone();
    app.open_file("binder.pdf", &binder()).unwrap();
    app.set_pdf_import_as_tabs(false).unwrap();
    app.session.execute("file.new", serde_json::json!({"width": 20, "height": 20})).unwrap();
    let other = app.session.active().unwrap().doc.clone();
    assert!(app.confirm_pdf_pages(&[3]).is_err());
    assert_eq!(app.pending_pdf_pages(), Some(3));
    app.confirm_pdf_pages(&[2, 0]).unwrap();
    assert_eq!(app.session.documents().len(), 2);
    assert_eq!(app.session.active().unwrap().doc.id, target);
    assert_eq!(app.session.documents()[1].doc, other);
    assert_eq!(app.session.active().unwrap().doc.layers.len(), before.layers.len() + 2);
    assert!(app.session.undo());
    assert_eq!(app.session.active().unwrap().doc, before);
    app.open_file("binder.pdf", &binder()).unwrap();
    app.set_pdf_import_as_tabs(false).unwrap();
    app.session.execute("file.close", serde_json::json!({})).unwrap();
    assert!(app.confirm_pdf_pages(&[0]).is_err());
    assert_eq!(app.pending_pdf_pages(), Some(3));
    assert_eq!(app.session.active().unwrap().doc, other);
    app.cancel_pdf_import();
}

#[test]
fn place_embedded_pdf_dialog_waits_for_page_selection() {
    use photocraft_ui_egui::FileDialogAnswer;
    let bytes = binder();
    let services = Services {
        file_dialog: Some(Box::new(move |_, _, reply| reply.send(Some(FileDialogAnswer::Contents("binder.pdf".into(), bytes.clone()))))),
        ..Default::default()
    };
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), services);
    app.session.execute("file.new", serde_json::json!({"width": 100, "height": 80})).unwrap();
    let before = app.session.active().unwrap().doc.clone();
    photocraft_ui_egui::menus::invoke(&mut app, &egui::Context::default(), "file.placeEmbedded", serde_json::json!({})).unwrap();
    app.poll_file_dialog(&egui::Context::default(), None);
    app.poll_file_dialog(&egui::Context::default(), None);
    assert_eq!(app.pending_pdf_pages(), Some(3));
    assert_eq!(app.session.active().unwrap().doc, before);
    app.confirm_pdf_pages(&[1]).unwrap();
    assert_eq!(app.session.documents().len(), 1);
    assert_eq!(app.session.active().unwrap().doc.layers.len(), before.layers.len() + 1);
    assert!(app.session.active().unwrap().doc.layers.iter().any(|l| l.name == "binder - Page 2"));
}

#[test]
fn picker_cancel_selected_and_all_pages_create_only_independent_tabs() {
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
    let bytes = binder();
    app.open_file("binder.pdf", &bytes).unwrap();
    assert_eq!(app.pending_pdf_pages(), Some(3));
    assert!(app.session.documents().is_empty());
    app.cancel_pdf_import();
    assert!(app.session.documents().is_empty());
    app.open_file("binder.pdf", &bytes).unwrap();
    assert!(app.open_pdf_pages(&[]).is_err());
    assert_eq!(app.pending_pdf_pages(), Some(3));
    app.open_pdf_pages(&[2, 1]).unwrap();
    assert_eq!(app.session.documents().len(), 2);
    assert_eq!(app.session.documents()[0].doc.name, "binder - Page 2.pdf");
    assert_eq!(app.session.documents()[1].doc.name, "binder - Page 3.pdf");
    for st in app.session.documents() {
        assert_eq!(st.doc.size, Size::new(144, 72));
        assert_eq!(st.doc.artboards()[0].2.rect.x0, 0);
        assert_eq!(st.doc.artboards()[0].2.rect.y0, 0);
        assert!(st.path.is_none() && st.source_read_only);
    }
    assert_ne!(app.session.documents()[0].doc.id, app.session.documents()[1].doc.id);
    app.open_file("binder.pdf", &bytes).unwrap();
    app.open_pdf_pages(&[0, 1, 2]).unwrap();
    assert_eq!(app.session.documents().len(), 5);
}

#[test]
fn background_selected_import_creates_separate_tabs() {
    use photocraft_engine::jobs::{OpenSource, Started};
    let mut session = photocraft_engine::Session::new();
    let started =
        session.start_open("binder.pdf", OpenSource::PdfPages { bytes: std::sync::Arc::new(binder()), pages: vec![0, 2], resolution: 300.0 }).unwrap();
    if let Started::Job(job) = started {
        session.wait_job(job).unwrap();
    }
    assert_eq!(session.documents().len(), 2);
    assert_eq!(session.documents()[0].doc.name, "binder - Page 1.pdf");
    assert_eq!(session.documents()[1].doc.name, "binder - Page 3.pdf");
    assert_eq!(session.documents()[0].doc.resolution_dpi, 300.0);
    assert_eq!(session.documents()[0].doc.size, Size::new(300, 150));
}

#[test]
fn picker_resolution_changes_pixels_but_not_physical_size() {
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
    app.open_file("binder.pdf", &binder()).unwrap();
    assert!(app.set_pdf_import_resolution(f32::NAN).is_err());
    app.set_pdf_import_resolution(72.0).unwrap();
    app.open_pdf_pages(&[0]).unwrap();
    let doc = &app.session.active().unwrap().doc;
    assert_eq!(doc.size, Size::new(72, 36));
    assert_eq!(doc.resolution_dpi, 72.0);
    let bytes = export(doc, "pdf", &ExportOptions::default()).unwrap().bytes;
    assert_eq!(photocraft_io::pdf::page_sizes(&bytes).unwrap(), [(72.0, 36.0)]);
}

#[test]
fn pdf_open_and_save_as_use_shared_codec_and_protect_original() {
    let source = Document::with_background("test", Size::new(72, 36), ColorMode::Rgb, SampleType::U8, Color::WHITE);
    let bytes = export(&source, "pdf", &ExportOptions::default()).unwrap().bytes;
    let writes = Rc::new(RefCell::new(Vec::new()));
    let captured = writes.clone();
    let services = Services {
        import: Some(Box::new(|name, bytes, depth| {
            photocraft_io::import_with_svg_group_depth(name, bytes, depth).map(|r| (r.document, r.warnings)).map_err(|e| e.to_string())
        })),
        export: Some(Box::new(|doc, path, _| export(doc, path, &ExportOptions::default()).map(|r| (r.bytes, r.warnings)).map_err(|e| e.to_string()))),
        write: Some(Box::new(move |path, bytes| {
            captured.borrow_mut().push((path.to_string(), bytes.to_vec()));
            Ok(())
        })),
        ..Default::default()
    };
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), services);
    app.open_file("original.pdf", &bytes).unwrap();
    assert!(app.session.documents().is_empty());
    assert_eq!(app.pending_pdf_pages(), Some(1));
    app.open_pdf_pages(&[0]).unwrap();
    let st = app.session.active().unwrap();
    assert!(st.source_read_only);
    assert!(st.path.is_none());
    assert_eq!(st.doc.artboards().len(), 1);
    app.save_as(Some("edited.pdf".into())).unwrap();
    let written = writes.borrow();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].0, "edited.pdf");
    let reopened = import("edited.pdf", &written[0].1).unwrap();
    assert_eq!(reopened.document.artboards().len(), 1);
    assert_eq!(reopened.document.size, Size::new(144, 72));
}

#[test]
fn failed_page_import_preserves_picker_and_existing_work_for_retry() {
    let source = Document::with_background("wide", Size::new(600, 1), ColorMode::Rgb, SampleType::U8, Color::WHITE);
    let bytes = export(&source, "pdf", &ExportOptions::default()).unwrap().bytes;
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
    app.session.add_document(Document::new("existing", Size::new(20, 10), ColorMode::Rgb, SampleType::U16), None);
    app.session.active_mut().unwrap().revision += 1;
    let before = app.session.active().unwrap().doc.clone();
    app.open_file("wide.pdf", &bytes).unwrap();
    app.set_pdf_import_resolution(2400.0).unwrap();
    assert!(app.open_pdf_pages(&[0]).unwrap_err().contains("16,384"));
    assert_eq!(app.session.documents().len(), 1);
    assert_eq!(app.session.active().unwrap().doc, before);
    assert!(app.session.active().unwrap().is_dirty());
    assert_eq!(app.pending_pdf_pages(), Some(1), "failed rendering must allow correcting the resolution");
    app.set_pdf_import_resolution(72.0).unwrap();
    app.open_pdf_pages(&[0]).unwrap();
    assert_eq!(app.session.documents().len(), 2);
    assert_eq!(app.pending_pdf_pages(), None);
}

#[test]
fn binder_selection_is_stable_across_refresh_reorder_and_new_documents() {
    use photocraft_ui_egui::export_dialog;
    use serde_json::json;
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
    for _ in 0..3 {
        app.session.add_document(Document::new("same", Size::new(8, 8), ColorMode::Rgb, SampleType::U8), None);
    }
    let ids: Vec<_> = app.session.documents().iter().map(|st| st.doc.id.0).collect();
    let dialog = export_dialog::open_all_pdf(&mut app).unwrap();
    let mut fields = app.ui.dialogs.iter().find(|d| d.id == dialog).unwrap().fields.clone();
    fields.insert("documentIds".into(), json!([ids[0], ids[2]]));
    app.session.execute("document.move", json!({"document":2,"to":0})).unwrap();
    app.session.add_document(Document::new("new", Size::new(8, 8), ColorMode::Rgb, SampleType::U8), None);
    let ctx = egui::Context::default();
    let _ = ctx.run_ui(Default::default(), |ui| export_dialog::body(&mut app, ui, &mut fields));
    assert_eq!(fields["documentIds"], json!([ids[2], ids[0]]));
    assert!(export_dialog::can_confirm(&app, &fields));
    app.session.close(0);
    let _ = ctx.run_ui(Default::default(), |ui| export_dialog::body(&mut app, ui, &mut fields));
    assert_eq!(fields["documentIds"], json!([ids[0]]));
    fields.insert("documentIds".into(), json!([]));
    assert!(!export_dialog::can_confirm(&app, &fields));
    assert!(export_dialog::confirm(&mut app, &fields).is_err());
}

#[test]
fn opening_with_active_document_defaults_to_separate_tabs_even_after_switch() {
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
    app.session.execute("file.new", serde_json::json!({"width":20,"height":20})).unwrap();
    let original = app.session.active().unwrap().doc.clone();
    app.open_file("binder.pdf", &binder()).unwrap();
    app.session.execute("file.new", serde_json::json!({"width":30,"height":30})).unwrap();
    let other = app.session.active().unwrap().doc.clone();
    app.confirm_pdf_pages(&[0, 2]).unwrap();
    assert_eq!(app.session.documents().len(), 4);
    assert_eq!(app.session.documents()[0].doc, original);
    assert_eq!(app.session.documents()[1].doc, other);
    assert_eq!(app.session.documents()[2].doc.name, "binder - Page 1.pdf");
    assert_eq!(app.session.documents()[3].doc.name, "binder - Page 3.pdf");
}

#[test]
fn explicit_layers_retains_source_stack_while_tabs_composite_and_unsupported_layers_refuse() {
    let mut source = Document::with_background("OCG source", Size::new(12, 8), ColorMode::Rgb, SampleType::U8, Color::WHITE);
    source.layers.push(photocraft_doc::Layer::raster("Hidden ink", photocraft_doc::PixelFormat::new(ColorMode::Rgb, SampleType::U8, true)));
    source.layers.last_mut().unwrap().visible = false;
    let bytes = photocraft_io::pdf_layers::export_ocg(&[&source], &mut |_| Ok(())).unwrap().bytes;
    let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
    app.open_file("source.pdf", &bytes).unwrap();
    app.confirm_pdf_pages(&[0]).unwrap();
    let flat = app.session.active().unwrap().doc.clone();
    assert_eq!(flat.layers[0].children().unwrap().len(), 1);
    app.open_file("source.pdf", &bytes).unwrap();
    app.set_pdf_import_as_layers().unwrap();
    app.confirm_pdf_pages(&[0]).unwrap();
    let layers = app.session.active().unwrap().doc.layers[0].children().unwrap();
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[1].name, "Hidden ink");
    assert!(!layers[1].visible);
    assert_eq!(app.session.documents()[0].doc, flat);
    let unsupported: Vec<u8> = bytes
        .windows(b"/PhotoCraftLayered 1".len())
        .position(|w| w == b"/PhotoCraftLayered 1")
        .map(|offset| {
            let mut modified = bytes.clone();
            modified[offset + b"/PhotoCraftLayered ".len()] = b'0';
            modified
        })
        .unwrap();
    app.open_file("other-producer.pdf", &unsupported).unwrap();
    assert!(app.set_pdf_import_as_layers().unwrap_err().contains("not supported"));
    assert_eq!(app.session.documents().len(), 2);
    assert_eq!(app.pending_pdf_pages(), Some(1));
    app.cancel_pdf_import();
    app.open_file("ordinary.pdf", &binder()).unwrap();
    assert!(app.set_pdf_import_as_layers().unwrap_err().contains("no source OCG layers"));
    assert_eq!(app.session.documents().len(), 2);
}
