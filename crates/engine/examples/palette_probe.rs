//! Palette tool timings on a 24 MP document; no display/frame-time claim.
use photocraft_engine::Session;
use serde_json::json;
use std::time::Instant;
fn stats(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    json!({"mean_ms":values.iter().sum::<f64>()/n as f64,"p95_ms":values.get((n as f64*0.95).ceil() as usize-1),"p99_ms":values.get((n as f64*0.99).ceil() as usize-1),"samples":n})
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut s = Session::new();
    s.execute("file.new", json!({"width":6000,"height":4000,"background":"#3680ad"}))?;
    s.execute("layer.new.layer", json!({"name":"Untouched artwork"}))?;
    let original = s.active().ok_or("no document")?.doc.clone();
    let mut deposit = Vec::new();
    let mut mix = Vec::new();
    let mut pickup = Vec::new();
    for i in 0..128 {
        let x = 80.0 + (i % 4) as f32 * 30.0;
        let y = 80.0 + (i % 3) as f32 * 30.0;
        let now = Instant::now();
        s.execute("palette.deposit", json!({"points":[[x,y]],"size":32,"color":[(i%3) as f32/2.0,0.2,1.0-(i%3) as f32/2.0,1]}))?;
        deposit.push(now.elapsed().as_secs_f64() * 1000.0);
        let now = Instant::now();
        s.execute("palette.mix", json!({"points":[[x,y],[x+24.0,y+15.0]],"size":32}))?;
        mix.push(now.elapsed().as_secs_f64() * 1000.0);
        let now = Instant::now();
        s.execute("palette.pickup", json!({"x":x,"y":y,"size":24}))?;
        pickup.push(now.elapsed().as_secs_f64() * 1000.0);
    }
    let now = Instant::now();
    let snapshot = s.execute("palette.get", json!({}))?;
    let export_ms = now.elapsed().as_secs_f64() * 1000.0;
    let bytes = serde_json::to_vec(&snapshot)?.len();
    let expected = s.tools.artist_palette.paint.clone();
    let mut restored = Session::new();
    let now = Instant::now();
    restored.execute("palette.restore", snapshot)?;
    let restore_ms = now.elapsed().as_secs_f64() * 1000.0;
    let exact = expected == restored.tools.artist_palette.paint;
    let untouched = *s.active().ok_or("no document")?.doc == *original;
    println!(
        "{}",
        json!({"document":[6000,4000],"fixture":"uniform background + empty source layer; palette workload is document-size independent","palette":[256,256],"deposit":stats(deposit),"mix":stats(mix),"pickup":stats(pickup),"snapshot_bytes":bytes,"snapshot_ms":export_ms,"restore_ms":restore_ms,"restore_exact":exact,"source_preserved":untouched,"scope":"CPU command costs; display and physical pointer latency not measured"})
    );
    if !exact || !untouched {
        return Err("palette preservation check failed".into());
    }
    Ok(())
}
