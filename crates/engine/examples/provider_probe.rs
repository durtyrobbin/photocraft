//! Regional request/placement costs on dense 24MP artwork; synthetic provider, no model/socket.
use photocraft_genai::{Capabilities, GeneratedImage, GuidedEditRequest, ImageProvider, ProviderInfo};
use photocraft_geom::Rect;
use serde_json::json;
use std::{
    sync::{Arc, Mutex},
    time::Instant,
};
#[derive(Default)]
struct Mock {
    start: Mutex<Option<Instant>>,
    samples: Mutex<Vec<(f64, usize, usize)>>,
}
impl ImageProvider for Mock {
    fn info(&self) -> ProviderInfo {
        ProviderInfo { id: "synthetic", name: "Benchmark fixture", model: "no-model".into(), host: "none.invalid" }
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { edit: true, ..Default::default() }
    }
    fn local_only(&self) -> bool {
        true
    }
    fn supports_guidance(&self) -> bool {
        true
    }
    fn endpoint(&self) -> String {
        "http://127.0.0.1:1/no-network-fixture".into()
    }
    fn edit_guided(&self, r: &GuidedEditRequest) -> photocraft_genai::Result<GeneratedImage> {
        let elapsed = self
            .start
            .lock()
            .map_err(|_| photocraft_genai::GenError::Invalid("benchmark lock".into()))?
            .as_ref()
            .map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
        let bytes = r.edit.image.len() + r.edit.mask.len() + r.guidance.iter().map(|g| g.mask.len()).sum::<usize>();
        self.samples.lock().map_err(|_| photocraft_genai::GenError::Invalid("benchmark lock".into()))?.push((elapsed, bytes, r.guidance.len()));
        Ok(GeneratedImage { bytes: r.edit.image.clone(), mime: "image/png".into() })
    }
}
fn stats(mut a: Vec<f64>) -> serde_json::Value {
    a.sort_by(f64::total_cmp);
    let n = a.len();
    json!({"samples":n,"mean_ms":a.iter().sum::<f64>()/n as f64,"p95_ms":a.get((n as f64*0.95).ceil()as usize-1),"p99_ms":a.get((n as f64*0.99).ceil()as usize-1)})
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
    s.execute("guidance.layer.add", json!({"name":"Synthetic intent"}))?;
    for (i, semantic) in ["foldDirection", "highlight", "areaProtect"].iter().enumerate() {
        let x = 2960 + i * 30;
        s.execute("guidance.stroke.add",json!({"layer":1,"stroke":{"semantic":semantic,"points":[{"x":x,"y":1960,"pressure":0.5},{"x":x+20,"y":2040,"pressure":1}],"width":12,"hardness":0.8,"opacity":1,"pressureSize":true,"arrow":true,"annotation":"Synthetic contract fixture"}}))?;
    }
    let source = s.active().ok_or("no source")?.doc.clone();
    let provider = Arc::new(Mock::default());
    s.set_image_provider(Some(provider.clone()));
    let mut cases = vec![];
    for side in [512, 1024, 1920] {
        let mut total = vec![];
        let mut prep = vec![];
        let mut bytes = 0;
        let mut semantics = 0;
        for _ in 0..12 {
            *provider.start.lock().map_err(|_| "benchmark lock")? = Some(Instant::now());
            let now = Instant::now();
            let result=s.execute("edit.generativeFill",json!({"recipe":{"prompt":"Synthetic pass-through request; not model quality","region":{"x0":3000-side/2,"y0":2000-side/2,"x1":3000+side/2,"y1":2000+side/2},"outputSize":[side,side],"contextPixels":64}}))?;
            total.push(now.elapsed().as_secs_f64() * 1000.0);
            let sample = provider.samples.lock().map_err(|_| "benchmark lock")?.last().copied().ok_or("no sample")?;
            prep.push(sample.0);
            bytes = sample.1;
            semantics = sample.2;
            let doc = &s.active().ok_or("no document")?.doc;
            let layer = doc.layer(photocraft_doc::LayerId(result["layer"].as_u64().ok_or("missing result")?)).ok_or("missing layer")?;
            if !source.layers.iter().zip(&doc.layers).all(|(a, b)| a == b) || layer.ai_result.is_none() {
                return Err("source or provenance changed unexpectedly".into());
            }
            s.execute("edit.undo", json!({}))?;
            if *s.active().ok_or("missing undo")?.doc != *source {
                return Err("Undo did not restore source exactly".into());
            }
        }
        cases.push(json!({"region":[side,side],"input":[side+128,side+128],"through_provider_entry_cpu":stats(prep),"complete_mock_pipeline_cpu":stats(total),"input_encoded_bytes":bytes,"semantic_references":semantics,"source_undo_exact":true}));
    }
    println!(
        "{}",
        json!({"document":[6000,4000],"fixture":"dense synthetic artwork plus three semantic strokes","cases":cases,"scope":"CPU regional rendering, masks, encoding, bounded decode/CMS and result placement. Provider returns its input immediately; no network, installed model, quality, GPU or presentation claim."})
    );
    Ok(())
}
