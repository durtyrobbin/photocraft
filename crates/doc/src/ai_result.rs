//! Native result provenance. Generated pixels remain ordinary editable raster layers.
use photocraft_geom::Rect;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResultOrigin {
    #[default]
    Imported,
    LocalProvider,
    RemoteProvider,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResultStatus {
    #[default]
    Proposed,
    Accepted,
}

/// Intent recorded for one completed request. Later regeneration creates another
/// result with its own record rather than rewriting the old provider attribution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditRecipe {
    pub prompt: String,
    #[serde(default)]
    pub material: String,
    #[serde(default)]
    pub lighting: String,
    pub region: Rect,
    #[serde(default)]
    pub context_pixels: u32,
    pub output_size: [u32; 2],
}
impl EditRecipe {
    pub fn validate(&self) -> Result<(), String> {
        if self.prompt.len() > 8000 || self.material.len() > 2000 || self.lighting.len() > 2000 {
            return Err("AI intent text exceeds its limit".into());
        }
        if self.region.is_empty() || self.region.x0 < 0 || self.region.y0 < 0 || u64::from(self.region.width()) * u64::from(self.region.height()) > 16_777_216 {
            return Err("AI result region must contain 1–16777216 canvas pixels".into());
        }
        if self.context_pixels > 1024
            || self.output_size.iter().any(|v| !(1..=4096).contains(v))
            || u64::from(self.output_size[0]) * u64::from(self.output_size[1]) > 16_777_216
        {
            return Err("AI context or output dimensions exceed their bounds".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AiResult {
    pub version: u32,
    pub origin: ResultOrigin,
    pub status: ResultStatus,
    /// Imported provenance is a user-supplied claim; provider operations populate it directly.
    pub provider: String,
    pub model: Option<String>,
    pub endpoint: Option<String>,
    pub created_unix_seconds: u64,
    pub input_digest: String,
    pub result_digest: String,
    pub recipe: EditRecipe,
}
impl AiResult {
    pub fn validate(&self) -> Result<(), String> {
        let digest = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
        if self.version != 1
            || self.provider.is_empty()
            || self.provider.len() > 128
            || self.model.as_ref().is_some_and(|s| s.len() > 256)
            || self.endpoint.as_ref().is_some_and(|s| s.len() > 512)
            || !digest(&self.input_digest)
            || !digest(&self.result_digest)
        {
            return Err("Invalid or oversized AI result provenance".into());
        }
        if self
            .endpoint
            .as_ref()
            .is_some_and(|s| !(s.starts_with("https://") || s.starts_with("http://")) || s.chars().any(|c| c.is_control()) || s.contains(['@', '?', '#']))
        {
            return Err("AI endpoint provenance must not contain credentials, queries or fragments".into());
        }
        self.recipe.validate()
    }
}
