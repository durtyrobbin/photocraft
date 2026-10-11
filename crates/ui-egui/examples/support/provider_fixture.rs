//! Snapshot-only service fixture. No transport, credential store or image model exists here.
use photocraft_genai::{Capabilities, ImageProvider, ProviderInfo};
use std::sync::Arc;
pub struct Fixture;
impl ImageProvider for Fixture {
    fn info(&self) -> ProviderInfo {
        ProviderInfo { id: "snapshot-fixture", name: "Synthetic provider · no network", model: "UI fixture only".into(), host: "example.invalid" }
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { edit: true, ..Default::default() }
    }
    fn supports_guidance(&self) -> bool {
        true
    }
    // Default edit methods return Unsupported. Even an accidental Generate sends nothing.
}
pub fn services() -> photocraft_ui_egui::GenAiServices {
    photocraft_ui_egui::GenAiServices {
        load_key: Box::new(|_| Ok(None)),
        save_key: Box::new(|_, _| Err("snapshot fixtures cannot store credentials".into())),
        delete_key: Box::new(|_| Err("snapshot fixtures cannot modify credentials".into())),
        provider: Box::new(|_, _, _, _| Ok(Arc::new(Fixture))),
        key_store_name: "unavailable in snapshot fixture",
    }
}
