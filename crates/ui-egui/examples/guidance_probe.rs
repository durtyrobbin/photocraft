//! Native CPU guidance editing costs, independent of any image model.
use egui_kittest::Harness;
use photocraft_doc::guidance::{Guidance, Layer, Point, Semantic, Stroke};
use photocraft_ui_egui::PhotocraftApp;
use serde_json::json;
use std::{sync::Arc, time::Instant};
fn stats(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    let n = samples.len();
    json!({"samples":n,"mean_ms":samples.iter().sum::<f64>()/n as f64,"p95_ms":samples.get((n as f64*0.95).ceil()as usize-1),"p99_ms":samples.get((n as f64*0.99).ceil()as usize-1)})
}
fn fixture(count: u32) -> Guidance {
    let strokes = (0..count)
        .map(|i| {
            let x = 300.0 + (i % 32) as f32 * 160.0;
            let y = 250.0 + (i / 32) as f32 * 100.0;
            Stroke {
                id: i + 1,
                semantic: Semantic::ALL[(i % 10) as usize],
                points: Arc::new(
                    (0..64).map(|j| Point { x: x + j as f32 * 2.0, y: y + (j as f32 * 0.1).sin() * 20.0, pressure: 0.3 + 0.7 * j as f32 / 63.0 }).collect(),
                ),
                width: 12.0,
                hardness: 0.8,
                opacity: 0.7,
                pressure_size: true,
                arrow: true,
                annotation: String::new(),
            }
        })
        .collect();
    Guidance {
        layers: Arc::new(vec![Layer {
            id: 1,
            name: "Synthetic semantic marks".into(),
            visible: true,
            locked: false,
            opacity: 1.0,
            strokes: Arc::new(strokes),
        }]),
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut cases = Vec::new();
    for count in [64, 1024] {
        let mut s = photocraft_engine::Session::new();
        s.execute("file.new", json!({"width":6000,"height":4000,"background":"#3a526c"}))?;
        let g = fixture(count);
        g.validate()?;
        s.edit("Fixture", |d, _| {
            d.guidance = g;
            Ok(())
        })?;
        let original = s.active().ok_or("no document")?.doc.clone();
        let steps = s.active().ok_or("no document")?.history.past_len();
        let now = Instant::now();
        let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1100.0)).build_eframe(move |cc| {
            PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
            let mut app = PhotocraftApp::new(s, Default::default());
            app.ui.guidance.open = true;
            app.ui.artist_palette.open = false;
            app.ui.brush_rack.open = false;
            app
        });
        h.run_steps(4);
        let cold_ms = now.elapsed().as_secs_f64() * 1000.0;
        let mut times = Vec::new();
        for i in 0..128 {
            h.state_mut().ui.guidance.stroke = Some(i % count + 1);
            let now = Instant::now();
            h.run_steps(1);
            times.push(now.elapsed().as_secs_f64() * 1000.0);
        }
        let doc = &h.state().session.active().ok_or("no document")?.doc;
        let unchanged = Arc::ptr_eq(&original, doc) && steps == h.state().session.active().ok_or("no document")?.history.past_len();
        let mut masks = Vec::new();
        for semantic in Semantic::ALL {
            let now = Instant::now();
            let mask = photocraft_engine::guidance_cmds::render_mask(&doc.guidance, doc.bounds(), [1500, 1000], semantic)?;
            masks.push(json!({"semantic":semantic,"ms":now.elapsed().as_secs_f64()*1000.0,"nonzero_pixels":mask.iter().filter(|&&p|p>0).count()}));
        }
        cases.push(json!({"strokes":count,"points":count*64,"cold_first_frames_ms":cold_ms,"warm_cpu_app_update":stats(times),"masks":masks,"document_history_unchanged":unchanged}));
        if !unchanged {
            return Err("guidance view changed document/history".into());
        }
    }
    println!(
        "{}",
        json!({"document":[6000,4000],"fixture":"flat native document and synthetic semantic vector guidance; no model","cases":cases,"scope":"CPU native app updates and intent rasterization; no GPU/presentation or physical pointer latency"})
    );
    Ok(())
}
