//! Bounded synthetic PDF export qualification. No personal documents or file writes.
use photocraft_doc::{ColorMode, Document, Layer, PixelFormat, Rect, SampleType, Size};
use photocraft_testkit::perf::RssSampler;
use std::time::{Duration, Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args().nth(1).ok_or("report path required")?;
    let mut rows = Vec::new();
    let rss = RssSampler::start(Duration::from_millis(5));
    for (w, h) in [(2048, 2048), (4096, 4096), (6000, 4000), (8192, 8192)] {
        for layers in [1, 10, 50, 100] {
            let mut doc = Document::new("Synthetic bounded layers", Size::new(w, h), ColorMode::Rgb, SampleType::U8);
            doc.resolution_dpi = 144.;
            for i in 0..layers {
                doc.layers.push(Layer::raster(format!("Layer {i}"), PixelFormat::new(ColorMode::Rgb, SampleType::U8, true)));
            }
            let bounded = u64::from(w) * u64::from(h) > 16_000_000 || u64::from(w) * u64::from(h) * layers * 2 > 64_000_000;
            if !bounded {
                let mut seed = 0x12345678u32;
                for layer in &mut doc.layers {
                    for y in (0..h).step_by(64) {
                        let rows = 64.min(h - y);
                        let mut pixels = Vec::with_capacity((w * rows * 4) as usize);
                        for _ in 0..w * rows {
                            for _ in 0..3 {
                                seed ^= seed << 13;
                                seed ^= seed >> 17;
                                seed ^= seed << 5;
                                pixels.push(seed as u8);
                            }
                            pixels.push(200);
                        }
                        layer.surface_mut().ok_or("missing surface")?.write_interleaved(Rect::new(0, y as i32, w as i32, (y + rows) as i32), &pixels);
                    }
                }
            }
            rss.reset();
            let start = Instant::now();
            let result = photocraft_io::pdf_layers::export_ocg(&[&doc], &mut |_| Ok(()));
            let (status, bytes, error) = match result {
                Ok(r) => ("EXPORTED", r.bytes.len(), None),
                Err(e) => ("BOUNDED REJECTION", 0, Some(e.to_string())),
            };
            if bounded != error.is_some() {
                return Err("unexpected safety-bound result".into());
            }
            rows.push(serde_json::json!({"width":w,"height":h,"layers":layers,"input":if bounded {"sparse preflight only; no large pixel allocation"} else {"high-entropy RGBA"},"status":status,"bytes":bytes,"ms":start.elapsed().as_secs_f64()*1000.,"sampledPeakRssBytes":rss.peak(),"error":error}));
            if !bounded {
                let start = Instant::now();
                let result = photocraft_io::pdf_layers::export_ocg(&[&doc], &mut |_| Err("cancelled by qualification".into()));
                if result.is_ok() {
                    return Err("cancellation ignored".into());
                }
                rows.push(serde_json::json!({"operation":"cancel at first progress callback","ms":start.elapsed().as_secs_f64()*1000.}));
                let start = Instant::now();
                let mut requested = None;
                let result = photocraft_io::pdf_layers::export_ocg(&[&doc], &mut |fraction| {
                    if fraction >= 0.25 {
                        requested = Some(Instant::now());
                        Err("cancel quarter export".into())
                    } else {
                        Ok(())
                    }
                });
                if result.is_ok() || requested.is_none() {
                    return Err("mid-export cancellation was not observed".into());
                }
                rows.push(serde_json::json!({"operation":"cancel after quarter export","totalMs":start.elapsed().as_secs_f64()*1000.,"requestToReturnMs":requested.map(|t|t.elapsed().as_secs_f64()*1000.)}));
            }
        }
    }
    std::fs::write(output, serde_json::to_vec_pretty(&rows)?)?;
    Ok(())
}
