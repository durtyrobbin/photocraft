//! Standalone local preset experiment using the existing PhotoCraft engine.
use photocraft_engine::paint::{GrayTile, StrokePoint, TipShape};
use photocraft_engine::{BrushSettings, Session, brush_cmds::mixer_live::LiveMixer};
use serde_json::json;
use std::time::Instant;

fn brush(size: f32, loaded: bool) -> BrushSettings {
    let mut b = BrushSettings {
        size,
        hardness: 1.0,
        roundness: 0.7,
        spacing: if loaded { 0.3 } else { 0.12 },
        pressure_size: false,
        pressure_opacity: false,
        seed: 42,
        ..Default::default()
    };
    b.tip = TipShape::Sampled(GrayTile::from_fn(64, 64, |x, y| {
        let u = (x as f32 + 0.5) / 32.0 - 1.0;
        let v = (y as f32 + 0.5) / 32.0 - 1.0;
        if u * u + v * v <= 1.0 { 0.25 + 0.75 * photocraft_engine::paint::rng::hash2(x as i32 / 2, 0, 91) } else { 0.0 }
    }));
    b.mixer.wet = if loaded { 0.0 } else { 0.65 };
    b.mixer.mix = if loaded { 0.0 } else { 0.7 };
    b.mixer.load = if loaded { 1.0 } else { 0.5 };
    b.mixer.sample_all_layers = !loaded;
    b.mixer.clean_after_stroke = false;
    b.mixer.reload_after_stroke = false;
    b
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let out = std::path::PathBuf::from(args.get(1).ok_or("expected output directory")?);
    std::fs::create_dir_all(&out)?;
    let mut presets = Session::new();
    let warnings = presets.attach_preset_store(photocraft_engine::preset_store::open_dir(out.join("Presets")));
    if !warnings.is_empty() {
        return Err(format!("preset store: {warnings:?}").into());
    }
    let fast = brush(64.0, true);
    // This isolated store substitutes the rack's bristle slot, and adds a clearly named picker entry.
    for name in ["Studio / Hog Bristle", "Fast / Loaded bristle"] {
        presets.execute("brush.presets.save", json!({"name":name,"brush":fast}))?;
    }
    presets.sync_preset_store();
    let mut restored = Session::new();
    let warnings = restored.attach_preset_store(photocraft_engine::preset_store::open_dir(out.join("Presets")));
    if !warnings.is_empty() {
        return Err(format!("preset reload: {warnings:?}").into());
    }
    restored.execute("tools.setBrush", json!({"preset":"Fast / Loaded bristle"}))?;
    if restored.tools.brush.mixer.wet != 0.0 || restored.tools.brush.mixer.sample_all_layers || restored.tools.brush.spacing != 0.3 {
        return Err("loaded preset did not survive native store roundtrip".into());
    }
    std::fs::write(out.join("loaded-bristle.json"), serde_json::to_vec_pretty(&json!({"name":"Fast / Loaded bristle","brush":fast}))?)?;
    if args.iter().any(|a| a == "--install-only") {
        return Ok(());
    }

    let mut base = Session::new();
    base.execute("file.new", json!({"width":6000,"height":4000,"background":"#808080"}))?;
    base.execute("layer.new.layer", json!({"name":"Dense synthetic source"}))?;
    base.edit("fixture", |doc, id| {
        let bounds = doc.bounds();
        if let Some(surface) = id.and_then(|id| doc.layer_mut(id)).and_then(|l| l.surface_mut()) {
            surface.fill_rect(bounds, &[0.1, 0.5, 0.8, 1.0]);
            let mut right = bounds;
            right.x0 = 3000;
            surface.fill_rect(right, &[0.8, 0.6, 0.1, 1.0]);
        }
        Ok(())
    })?;
    base.execute("layer.new.layer", json!({"name":"Paint"}))?;
    // Use actual palette deposit/pickup commands, so the carried multicolor load is not a mock.
    base.execute("palette.deposit", json!({"points":[[116,96]],"color":[1.0,0.1,0.1,1.0],"size":48}))?;
    base.execute("palette.deposit", json!({"points":[[140,96]],"color":[0.1,0.2,1.0,1.0],"size":48}))?;
    base.execute("palette.pickup", json!({"x":128,"y":96,"size":40}))?;
    let original_load = base.tools.mixer.clone();
    if original_load.spatial.is_none() {
        return Err("palette pickup did not produce a spatial load".into());
    }
    let source = base.active().ok_or("no document")?.doc.clone();
    let mut rows = Vec::new();
    for size in [64.0, 256.0, 512.0] {
        for loaded in [false, true] {
            for repeat in 0..3 {
                base.tools.mixer = original_load.clone();
                let b = brush(size, loaded);
                let start = Instant::now();
                let mut live = LiveMixer::begin(&base, &json!({"points":[[2600,2000]],"brush":b,"seed":42}))?;
                let begin_ms = start.elapsed().as_secs_f64() * 1000.0;
                let mut times = Vec::new();
                for i in 1..=96 {
                    let phase = (i as f64 * 48.0) % 1600.0;
                    let x = if phase < 800.0 { phase } else { 1600.0 - phase };
                    let p = StrokePoint::new(2600.0 + x, 2000.0 + (i as f64 * 0.17).sin() * 100.0, 1.0);
                    let start = Instant::now();
                    live.push(&[p])?;
                    times.push(start.elapsed().as_secs_f64() * 1000.0);
                }
                let shown = live.doc.clone();
                let p = live.prepare_commit(&mut base)?;
                base.execute("paint.mixerBrush", p)?;
                let painted = base.active().ok_or("no document")?.doc.clone();
                let preview_exact = *shown == *painted;
                let source_preserved = source.layers.iter().take(2).eq(painted.layers.iter().take(2));
                let changed = *painted != *source;
                base.execute("edit.undo", json!({}))?;
                let undo_exact = *base.active().ok_or("no document")?.doc == *source;
                if !preview_exact || !source_preserved || !undo_exact || !changed {
                    return Err("paint transaction check failed".into());
                }
                times.sort_by(f64::total_cmp);
                rows.push(json!({"size":size,"loaded_paint":loaded,"repeat":repeat,"begin_ms":begin_ms,
                    "mean_ms":times.iter().sum::<f64>()/times.len() as f64,"p95_ms":times.get(91),"max_ms":times.last(),
                    "samples":times.len(),"preview_exact":preview_exact,"source_preserved":source_preserved,"undo_exact":undo_exact,"changed":changed}));
            }
        }
    }
    let result = json!({"scope":"24MP, 3 layers; CPU engine only. No frame/pointer latency claim. Actual palette pickup. Preset approximation changes spacing and wet mixing.","results":rows});
    std::fs::write(out.join("benchmark.json"), serde_json::to_vec_pretty(&result)?)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
