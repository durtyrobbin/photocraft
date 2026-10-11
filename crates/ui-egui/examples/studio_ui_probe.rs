//! Native egui CPU update costs; excludes GPU presentation, monitor latency and disk settings I/O.
use egui_kittest::Harness;
use photocraft_ui_egui::{PhotocraftApp, state::Tool};
use serde_json::json;
use std::time::Instant;

fn stats(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    json!({"mean_ms":values.iter().sum::<f64>()/n as f64,"p95_ms":values.get((n as f64*0.95).ceil() as usize-1),"p99_ms":values.get((n as f64*0.99).ceil() as usize-1),"samples":n})
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut s = photocraft_engine::Session::new();
    s.execute("file.new", json!({"width":6000,"height":4000,"background":"#408090"}))?;
    s.execute("palette.deposit", json!({"points":[[128,128]],"size":70,"color":[0.9,0.1,0.2,1]}))?;
    let image = photocraft_engine::palette_cmds::capture_image(&s)?;
    let mut h = Harness::builder().with_size(egui::vec2(1600.0, 1100.0)).build_eframe(move |cc| {
        PhotocraftApp::setup_context(&cc.egui_ctx, Default::default());
        let mut app = PhotocraftApp::new(s, Default::default());
        app.ui.tool = Tool::MixerBrush;
        app.ui.artist_palette.position = [60.0, 420.0];
        app.ui.brush_rack.offset = [0.0, -300.0];
        app
    });
    h.run_steps(8);
    let mut cases = Vec::new();
    for custom in [false, true] {
        if custom {
            for slot in &mut h.state_mut().ui.brush_rack.slots {
                slot.artwork = Some(image.clone());
                slot.aspect = 0.4;
            }
        }
        h.run_steps(4);
        let mut idle = Vec::new();
        let mut editing = Vec::new();
        for i in 0..128 {
            let now = Instant::now();
            h.run_steps(1);
            idle.push(now.elapsed().as_secs_f64() * 1000.0);
            h.state_mut().ui.artist_palette.ryb = i % 2 == 0;
            let now = Instant::now();
            h.run_steps(1);
            editing.push(now.elapsed().as_secs_f64() * 1000.0);
        }
        cases.push(json!({"custom_artwork_slots":if custom {8}else{0},"cached_ui_update":stats(idle),"settings_change_and_serialization":stats(editing)}));
    }
    println!(
        "{}",
        json!({"document":[6000,4000],"fixture":"uniform background; visible palette and rack; warmed display caches","cases":cases,"scope":"CPU full native egui app update only; no GPU raster/presentation or filesystem preference write; no FPS claim"})
    );
    Ok(())
}
