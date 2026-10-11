//! Reproducible PDF page-picker screenshots using only synthetic documents.
//! cargo run -p photocraft-ui-egui --example pdf_import_demo -- <output-directory>
#[path = "../../io/tests/support/layered_interop.rs"]
mod fixture;
use egui_kittest::Harness;
use photocraft_doc::{Artboard, ColorMode, Document, Group, Layer, LayerContent, PixelFormat, Rect, SampleType, Size, Surface};
use photocraft_ui_egui::control::{ControlRequest, Outcome, handle};
use photocraft_ui_egui::{PhotocraftApp, Services, theme::ThemeKind};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).unwrap_or_else(|| "plan/evidence".into());
    std::fs::create_dir_all(&output)?;
    let mut doc = Document::new("Synthetic pages", Size::new(144, 2688), ColorMode::Rgb, SampleType::U8);
    doc.resolution_dpi = 144.0;
    for i in 0..12 {
        let color = [[0.2, 0.5, 0.9, 1.0], [0.9, 0.4, 0.2, 1.0], [0.2, 0.7, 0.4, 1.0]][i % 3];
        let y = i as i32 * 224;
        let rect = Rect::new(0, y, 144, y + 208);
        let mut surface = Surface::new(PixelFormat::new(ColorMode::Rgb, SampleType::U8, true));
        surface.write_region(rect, &[1.0, 1.0, 1.0, 1.0].repeat(144 * 208));
        surface.write_region(Rect::new(18, y + 22, 126, y + 28), &color.repeat(108 * 6));
        for line in 0..18 {
            let width = if line % 6 == 5 { 74 } else { 108 };
            let top = y + 45 + line * 7;
            surface.write_region(Rect::new(18, top, 18 + width, top + 2), &[0.2, 0.2, 0.2, 1.0].repeat(width as usize * 2));
        }
        doc.layers.push(Layer::new(
            format!("Page {}", i + 1),
            LayerContent::Group(Group {
                children: vec![Layer::new("Rectangle", LayerContent::Raster(surface))],
                expanded: true,
                artboard: Some(Artboard::new(rect)),
            }),
        ));
    }
    doc.layers.reverse();
    let bytes = photocraft_io::export(&doc, "pdf", &photocraft_io::ExportOptions::default())?.bytes;
    std::fs::write(format!("{output}/synthetic-pages.pdf"), &bytes)?;
    let mut measurements = Vec::new();
    let rss = photocraft_testkit::perf::RssSampler::start(std::time::Duration::from_millis(5));
    rss.reset();
    let start = std::time::Instant::now();
    photocraft_io::pdf::page_sizes(&bytes)?;
    measurements.push(serde_json::json!({"operation":"inspect 12 pages", "ms":start.elapsed().as_secs_f64()*1000.0,"sampledPeakRssBytes":rss.peak()}));
    rss.reset();
    let start = std::time::Instant::now();
    photocraft_io::pdf::page_preview(std::sync::Arc::new(bytes.clone()), 0)?;
    measurements.push(serde_json::json!({"operation":"one bounded thumbnail", "ms":start.elapsed().as_secs_f64()*1000.0,"sampledPeakRssBytes":rss.peak()}));
    for ppi in [72.0, 144.0, 300.0] {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), Services::default());
        rss.reset();
        let start = std::time::Instant::now();
        app.open_file("Synthetic pages.pdf", &bytes).map_err(std::io::Error::other)?;
        measurements
            .push(serde_json::json!({"operation":"queue picker", "ppi":ppi,"ms":start.elapsed().as_secs_f64()*1000.0,"sampledPeakRssBytes":rss.peak()}));
        app.set_pdf_import_resolution(ppi).map_err(std::io::Error::other)?;
        rss.reset();
        let start = std::time::Instant::now();
        app.open_pdf_pages(&[0, 2, 3, 6]).map_err(std::io::Error::other)?;
        measurements.push(
            serde_json::json!({"operation":"import four selected tabs", "ppi":ppi,"ms":start.elapsed().as_secs_f64()*1000.0,"sampledPeakRssBytes":rss.peak()}),
        );
    }

    let layered = fixture::document(SampleType::U8);
    let ocg = photocraft_io::pdf_layers::export_ocg(&[&layered], &mut |_| Ok(()))?.bytes;
    // Exercise complex PSD fixture construction too; native type/shape data are tested separately.
    let _complex = fixture::complex_document(SampleType::U16);
    for (w, h) in [(1000.0, 750.0), (800.0, 600.0)] {
        for scale in [1.0, 2.0] {
            let mut harness = Harness::builder().with_size(egui::vec2(w, h)).with_pixels_per_point(scale).with_max_steps(64).wgpu().build_eframe(|cc| {
                PhotocraftApp::setup_context(&cc.egui_ctx, ThemeKind::default());
                PhotocraftApp::new(photocraft_engine::Session::new(), Services::default())
            });
            harness.state_mut().session.execute("file.new", serde_json::json!({"width": 600, "height": 400}))?;
            rss.reset();
            let first_frame = std::time::Instant::now();
            harness.state_mut().open_file("Synthetic pages.pdf", &bytes).map_err(std::io::Error::other)?;
            harness.run_steps(4);
            let _ = harness.render()?;
            measurements.push(serde_json::json!({"operation":"queue and first offscreen picker frame","width":w,"height":h,"scale":scale,"ms":first_frame.elapsed().as_secs_f64()*1000.,"sampledPeakRssBytes":rss.peak()}));
            for theme in ThemeKind::ALL {
                let ctx = harness.ctx.clone();
                let (req, _rx) = ControlRequest::new("ui.set", serde_json::json!({"theme": theme.id()}));
                match handle(harness.state_mut(), &ctx, &req) {
                    Outcome::Done(result) if result["ok"] == true => {}
                    _ => return Err(std::io::Error::other("could not apply screenshot theme").into()),
                }
                for _ in 0..12 {
                    harness.step();
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                harness.render()?.save(format!("{output}/pdf-picker-{}-{w:.0}x{h:.0}-{scale:.0}x.png", theme.id()))?;
            }
            harness.state_mut().cancel_pdf_import();
            harness.state_mut().open_file("Layered sample.pdf", &ocg).map_err(std::io::Error::other)?;
            harness.state_mut().set_pdf_import_as_layers().map_err(std::io::Error::other)?;
            harness.run_steps(12);
            harness.render()?.save(format!("{output}/ocg-picker-{w:.0}x{h:.0}-{scale:.0}x.png"))?;
            harness.state_mut().cancel_pdf_import();
            for (index, name) in ["Blue study", "Orange study", "Green study"].into_iter().enumerate() {
                let mut sample = fixture::complex_document(SampleType::U16);
                sample.name = name.into();
                if let Some(layer) = sample.layers.get_mut(0) {
                    layer.name = format!("Background {index}");
                    let color = [[0.1, 0.3, 0.8, 1.0], [0.9, 0.4, 0.1, 1.0], [0.1, 0.7, 0.3, 1.0]][index];
                    if let Some(surface) = layer.surface_mut() {
                        surface.write_region(Rect::new(0, 0, 64, 48), &color.repeat(64 * 48));
                    }
                }
                if w == 1000.0 && scale == 1.0 {
                    std::fs::write(format!("{output}/study-{index}.pcraft"), photocraft_io::export(&sample, "pcraft", &Default::default())?.bytes)?;
                }
                harness.state_mut().session.add_document(sample, None);
            }
            photocraft_ui_egui::export_dialog::open_all_pdf(harness.state_mut()).map_err(std::io::Error::other)?;
            if let Some(dialog) = harness.state_mut().ui.dialogs.last_mut() {
                dialog.fields.insert("pairedPhotoshop".into(), serde_json::json!(true));
                dialog.fields.insert("includeHiddenLayers".into(), serde_json::json!(true));
            }
            harness.run_steps(12);
            harness.render()?.save(format!("{output}/pdf-binder-{w:.0}x{h:.0}-{scale:.0}x.png"))?;
        }
    }
    std::fs::write(format!("{output}/timings.json"), serde_json::to_vec_pretty(&measurements)?)?;
    Ok(())
}
