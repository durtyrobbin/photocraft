//! Synthetic fidelity comparison. `pigment_quality <output-directory>`; no model or private artwork.
use photocraft_engine::{Session, brush_cmds::mixer_live::LiveMixer};
use photocraft_geom::Rect;
use photocraft_paint::StrokePoint;
use serde_json::json;
use std::{path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = PathBuf::from(std::env::args().nth(1).ok_or("output directory required")?);
    std::fs::create_dir_all(&dir)?;
    let mut results = Vec::new();
    let mut pixels = Vec::new();
    for grid in [0, 16, 32, 64] {
        let mut s = Session::new();
        s.execute("file.new", json!({"width":720,"height":256,"background":"#f0ede6"}))?;
        s.execute("layer.new.layer", json!({"name":"Synthetic pigment bands"}))?;
        s.edit("Fixture", |doc, id| {
            if let Some(target) = id.and_then(|id| doc.layer_mut(id)).and_then(|l| l.surface_mut()) {
                let rect = Rect::new(0, 0, 720, 256);
                let mut data = Vec::with_capacity(720 * 256 * 4);
                for y in 0..256 {
                    for x in 0..720 {
                        let color = if y < 128 { [0.95, 0.10, 0.035, 1.0] } else { [0.035, 0.18, 0.95, 1.0] };
                        let fine = if y % 13 < 3 { 0.35 } else { 1.0 };
                        let wave = 0.75 + 0.25 * (x as f32 * 0.027).sin().abs();
                        data.extend_from_slice(&[color[0] * fine * wave, color[1] * fine, color[2] * fine * wave, 1.0]);
                    }
                }
                target.write_region(rect, &data);
            }
            Ok(())
        })?;
        s.execute("layer.new.layer", json!({"name":format!("Reservoir {grid}")}))?;
        let before = s.active().ok_or("no document")?.doc.clone();
        s.execute("brush.mixer.load", json!({"color":"#e3b047"}))?;
        let p = json!({"points":[[90,128,1]],"brush":{"size":112,"hardness":0.4,"pressureSize":false,"spacing":0.15,"smoothing":{"amount":0},"mixer":{"reservoirGrid":grid}},"wet":100,"mix":90,"load":20,"flow":85,"cleanAfterStroke":false,"loadAfterStroke":false});
        let start = Instant::now();
        let mut live = LiveMixer::begin(&s, &p)?;
        for i in 1..=64 {
            live.push(&[StrokePoint::new(90.0 + i as f64 * 8.0, 128.0 + (i as f64 * 0.15).sin() * 12.0, 1.0)])?;
        }
        let shown = live.doc.clone();
        let prepared = live.prepare_commit(&mut s)?;
        s.execute("paint.mixerBrush", prepared)?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        let doc = &s.active().ok_or("no document")?.doc;
        if **doc != *shown || !before.layers.iter().take(2).eq(doc.layers.iter().take(2)) {
            return Err("preview/source mismatch".into());
        }
        let target = doc.layers.last().and_then(|l| l.surface()).ok_or("no target")?;
        let data = target.read_region(Rect::new(0, 0, 720, 256));
        let display: Vec<f32> = data
            .chunks_exact(4)
            .flat_map(|p| [p[0] * p[3] + (1.0 - p[3]) * 0.94, p[1] * p[3] + (1.0 - p[3]) * 0.93, p[2] * p[3] + (1.0 - p[3]) * 0.90])
            .collect();
        let image = photocraft_codecs::Image::from_normalized(720, 256, photocraft_codecs::ChannelLayout::Rgb, photocraft_codecs::SampleType::U8, &display)?;
        let bytes = photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &Default::default())?;
        std::fs::write(dir.join(format!("pigment-quality-{grid}.png")), bytes)?;
        let top = target.rgba(350, 110);
        let bottom = target.rgba(350, 146);
        let depletion=s.tools.mixer.local.as_ref().map(|g|json!({"cells":g.cells.len(),"minimum_load":g.cells.iter().map(|c|c.amount).fold(1.0,f32::min),"maximum_load":g.cells.iter().map(|c|c.amount).fold(0.0,f32::max)}));
        results.push(json!({"grid":grid,"live_and_commit_ms":elapsed,"top_rgba":top,"bottom_rgba":bottom,"red_blue_separation":((top[0]-top[2])-(bottom[0]-bottom[2]))/2.0,"depletion":depletion,"preview_exact":true,"source_preserved":true}));
        pixels.push(data);
    }
    if let Some(reference) = pixels.last() {
        for (result, data) in results.iter_mut().zip(&pixels) {
            let error = data.iter().zip(reference).map(|(a, b)| (*a as f64 - *b as f64).powi(2)).sum::<f64>() / data.len().max(1) as f64;
            result["rgba_rmse_vs_64"] = json!(error.sqrt());
        }
    }
    println!(
        "{}",
        json!({"fixture":"720x256 synthetic red/blue bands with 13-pixel stripes; target-only PNG on a neutral display background","comparison":"64-cell grid is a numerical reference, not artistic ground truth; total timing is illustrative, use pigment_probe for performance gates","cases":results})
    );
    Ok(())
}
