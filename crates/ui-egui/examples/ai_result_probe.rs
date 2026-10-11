//! CPU native UI comparison costs on a dense 24MP document. No model is called.
use egui_kittest::Harness;
use photocraft_geom::Rect;
use photocraft_ui_egui::{PhotocraftApp, ai_results_ui::Mode};
use serde_json::json;
use std::{sync::Arc, time::Instant};
fn stats(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let n = samples.len();
    json!({"samples":n,"mean_ms":samples.iter().sum::<f64>()/n as f64,"p95_ms":samples.get((n as f64*0.95).ceil() as usize-1),"p99_ms":samples.get((n as f64*0.99).ceil() as usize-1),"max_ms":samples.last()})
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut s = photocraft_engine::Session::new();
    s.execute("file.new", json!({"width":6000,"height":4000,"background":"#303050"}))?;
    s.execute("layer.new.layer", json!({"name":"Dense synthetic source"}))?;
    s.edit("Fixture", |doc, id| {
        if let Some(surface) = id.and_then(|id| doc.layer_mut(id)).and_then(|l| l.surface_mut()) {
            let mut pixels = vec![0.0; 256 * 256 * 4];
            for ty in (0..4000).step_by(256) {
                for tx in (0..6000).step_by(256) {
                    for (i, p) in pixels.chunks_exact_mut(4).enumerate() {
                        let x = tx + (i % 256) as i32;
                        let y = ty + (i / 256) as i32;
                        p.copy_from_slice(&[(x % 257) as f32 / 256.0, (y % 251) as f32 / 250.0, ((x ^ y) % 239) as f32 / 238.0, 0.85]);
                    }
                    surface.write_region(Rect::new(tx, ty, tx + 256, ty + 256), &pixels);
                }
            }
        }
        Ok(())
    })?;
    let image = photocraft_codecs::Image::from_u8(512, 512, photocraft_codecs::ChannelLayout::Rgba, [240, 140, 30, 255].repeat(512 * 512))?;
    let bytes = photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &Default::default())?;
    let original = s.active().ok_or("no document")?.doc.clone();
    let now = Instant::now();
    s.execute("ai.result.import",json!({"data":photocraft_paint::tile::b64_encode(&bytes),"recipe":{"prompt":"Synthetic proposal: contract benchmark only","region":{"x0":2800,"y0":1700,"x1":3312,"y1":2212},"outputSize":[512,512]}}))?;
    let import_ms = now.elapsed().as_secs_f64() * 1000.0;
    let committed = s.active().ok_or("no document")?.doc.clone();
    let source_preserved = original.layers.iter().zip(&committed.layers).all(|(a, b)| a == b);
    let now = Instant::now();
    s.execute("edit.undo", json!({}))?;
    let undo_ms = now.elapsed().as_secs_f64() * 1000.0;
    let undo_exact = *s.active().ok_or("no document")?.doc == *original;
    let now = Instant::now();
    s.execute("edit.redo", json!({}))?;
    let redo_ms = now.elapsed().as_secs_f64() * 1000.0;
    let redo_exact = *s.active().ok_or("no document")?.doc == *committed;
    let source = s.active().ok_or("no document")?.doc.clone();
    let history = s.active().ok_or("no document")?.history.past_len();
    let now = Instant::now();
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1100.0)).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
        let mut app = PhotocraftApp::new(s, Default::default());
        app.ui.ai_results.enabled = true;
        app.ui.artist_palette.open = false;
        app.ui.brush_rack.open = false;
        app
    });
    h.run_steps(4);
    let cold_first_frames_ms = now.elapsed().as_secs_f64() * 1000.0;
    let mut cases = Vec::new();
    for mode in [Mode::Vertical, Mode::Horizontal, Mode::Overlay, Mode::Before, Mode::After] {
        h.state_mut().ui.ai_results.mode = mode;
        h.run_steps(3);
        let mut times = Vec::new();
        for i in 0..128 {
            h.state_mut().ui.ai_results.position = i as f32 / 127.0;
            h.state_mut().ui.ai_results.overlay = i as f32 / 127.0;
            let now = Instant::now();
            h.run_steps(1);
            times.push(now.elapsed().as_secs_f64() * 1000.0);
        }
        cases.push(json!({"mode":mode,"warm_cpu_ui_update":stats(times)}));
    }
    let document_unchanged = Arc::ptr_eq(&source, &h.state().session.active().ok_or("no document")?.doc);
    let history_unchanged = history == h.state().session.active().ok_or("no document")?.history.past_len();
    println!(
        "{}",
        json!({"document":[6000,4000],"fixture":"dense synthetic source plus a 512x512 imported synthetic proposal; no model", "source_preserved":source_preserved,"undo_exact":undo_exact,"redo_exact":redo_exact,"import_ms":import_ms,"undo_ms":undo_ms,"redo_ms":redo_ms,"cold_first_frames_ms":cold_first_frames_ms,"cases":cases,"document_unchanged_by_comparison":document_unchanged,"history_unchanged_by_comparison":history_unchanged,"scope":"CPU native egui app updates and CPU compositor, no GPU raster/presentation or physical pointer latency"})
    );
    if !(source_preserved && undo_exact && redo_exact && document_unchanged && history_unchanged) {
        return Err("result transaction changed source or comparison edited document".into());
    }
    Ok(())
}
