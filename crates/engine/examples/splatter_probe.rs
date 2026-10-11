//! 24 MP CPU command benchmark; no display/GPU/tablet performance claim.
use photocraft_engine::{
    Session,
    paint::{GrayTile, splatter::Settings},
};
use photocraft_geom::Rect;
use serde_json::json;
use std::time::Instant;

fn stats(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    json!({"samples":n,"mean_ms":values.iter().sum::<f64>()/n as f64,
        "p95_ms":values.get((n as f64*0.95).ceil() as usize-1),
        "p99_ms":values.get((n as f64*0.99).ceil() as usize-1),"max_ms":values.last()})
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut s = Session::new();
    s.execute("file.new", json!({"width":6000,"height":4000,"background":"#808080"}))?;
    s.execute("layer.new.layer", json!({"name":"Dense synthetic source"}))?;
    s.edit("Fixture", |doc, id| {
        let surface =
            id.and_then(|id| doc.layer_mut(id)).and_then(|l| l.surface_mut()).ok_or_else(|| photocraft_engine::EngineError::Other("no source".into()))?;
        for y in (0..4000).step_by(256) {
            for x in (0..6000).step_by(256) {
                surface.fill_rect(Rect::new(x, y, (x + 256).min(6000), (y + 256).min(4000)), &[(x % 251) as f32 / 250.0, (y % 239) as f32 / 238.0, 0.6, 0.8]);
            }
        }
        Ok(())
    })?;
    s.execute("layer.new.layer", json!({"name":"Splatter"}))?;
    let before = s.active().ok_or("missing document")?.doc.clone();
    let mut results = Vec::new();
    for count in [160, 512, 2048] {
        for custom in [false, true] {
            let settings = Settings {
                count,
                max_size: if count == 160 { 10.0 } else { 32.0 },
                distance: 512.0,
                distribution: custom.then(|| GrayTile::from_fn(128, 128, |x, y| if ((x / 16) + (y / 16)) % 2 == 0 { 1.0 } else { 0.0 })),
                ..Default::default()
            };
            let mut paint = Vec::new();
            let mut undo = Vec::new();
            let mut redo = Vec::new();
            for seed in 0..32 {
                let mut settings = settings.clone();
                settings.seed = seed;
                let now = Instant::now();
                s.execute("paint.splatter", json!({"center":[3000,2000],"color":[0.2,0.6,0.9,1.0],"settings":settings}))?;
                paint.push(now.elapsed().as_secs_f64() * 1000.0);
                let after = s.active().ok_or("missing document")?.doc.clone();
                let now = Instant::now();
                s.execute("edit.undo", json!({}))?;
                undo.push(now.elapsed().as_secs_f64() * 1000.0);
                if s.active().ok_or("missing document")?.doc.layers != before.layers {
                    return Err("undo mismatch".into());
                }
                let now = Instant::now();
                s.execute("edit.redo", json!({}))?;
                redo.push(now.elapsed().as_secs_f64() * 1000.0);
                if s.active().ok_or("missing document")?.doc.layers != after.layers {
                    return Err("redo mismatch".into());
                }
                s.execute("edit.undo", json!({}))?;
            }
            results.push(json!({"count":count,"max_diameter":settings.max_size,"custom_mask":custom,"paint":stats(paint),"undo":stats(undo),"redo":stats(redo),"exact_undo_redo":true}));
        }
    }
    println!(
        "{}",
        json!({"canvas":[6000,4000],"source":"dense tiled RGBA8 synthetic, background plus source plus empty paint layer","results":results,
        "scope":"CPU command timings, fixture and exact comparisons excluded; no pointer/display/GPU measurement","pointer_to_visible_ms":null,"gpu_peak_percent":null})
    );
    Ok(())
}
