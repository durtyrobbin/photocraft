//! Generate public synthetic fixtures for independent PDF/PSD readers.
#[path = "../tests/support/layered_interop.rs"]
mod fixture;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args().nth(1).ok_or("output folder required")?;
    let root = std::path::Path::new(&root);
    std::fs::create_dir_all(root)?;
    for (depth, label) in [(photocraft_doc::SampleType::U8, "8"), (photocraft_doc::SampleType::U16, "16"), (photocraft_doc::SampleType::F32, "32")] {
        let doc = fixture::document(depth);
        for ext in ["psd", "psb", "pcraft", "png"] {
            let out = photocraft_io::export(&doc, ext, &Default::default())?;
            std::fs::write(root.join(format!("fixture-{label}.{ext}")), out.bytes)?;
        }
        if label == "8" {
            fn ids(layers: &[photocraft_doc::Layer], out: &mut Vec<(photocraft_doc::LayerId, String)>) {
                for layer in layers {
                    out.push((layer.id, layer.name.clone()));
                    if let Some(children) = layer.children() {
                        ids(children, out);
                    }
                }
            }
            let mut entries = Vec::new();
            ids(&doc.layers, &mut entries);
            let mut records = Vec::new();
            for (i, (id, name)) in entries.iter().enumerate() {
                let mut one = doc.clone();
                if let Some(layer) = one.layer_mut(*id) {
                    layer.visible = !layer.visible;
                }
                let filename = format!("toggle-{i}.png");
                let out = photocraft_io::export(&one, "png", &Default::default())?;
                std::fs::write(root.join(&filename), out.bytes)?;
                records.push(serde_json::json!({"name":name,"expected":filename}));
            }
            std::fs::write(root.join("toggles.json"), serde_json::to_vec_pretty(&records)?)?;
        }
        let out = photocraft_io::pdf_layers::export_ocg(&[&doc], &mut |_| Ok(()))?;
        std::fs::write(root.join(format!("fixture-{label}.pdf")), &out.bytes)?;
        let paired = photocraft_io::pdf_layers::paired_package(&[&doc], out, false, &mut |_| Ok(()))?;
        std::fs::write(root.join(format!("fixture-{label}.zip")), paired.bytes)?;
    }
    let complex = fixture::complex_document(photocraft_doc::SampleType::U16);
    for ext in ["psd", "psb", "pcraft", "png"] {
        std::fs::write(root.join(format!("complex.{ext}")), photocraft_io::export(&complex, ext, &Default::default())?.bytes)?;
    }
    let flat = photocraft_io::pdf::export_pdf_canvases(&[&complex], &Default::default(), &mut |_| Ok(()))?;
    let pair = photocraft_io::pdf_layers::paired_package(&[&complex], flat, false, &mut |_| Ok(()))?;
    std::fs::write(root.join("complex.zip"), pair.bytes)?;
    Ok(())
}
