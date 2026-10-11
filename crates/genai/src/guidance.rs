//! Explicit semantic references carried through the existing image-provider interface.
use crate::{EditRequest, GenError, Result};
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_GUIDANCE_TEXT: usize = 16_384;
pub const SEMANTICS: [&str; 10] =
    ["highlight", "shadow", "foldDirection", "materialBoundary", "edgePreservation", "textureDirection", "depthCue", "reflection", "areaModify", "areaProtect"];
#[derive(Clone)]
pub struct GuidanceImage {
    /// Explicit semantic name; never inferred from an RGB color.
    pub semantic: String,
    /// PNG coverage in input-image coordinates; white marks this semantic's support.
    pub mask: Vec<u8>,
    pub annotations: Vec<String>,
}
#[derive(Clone)]
pub struct GuidedEditRequest {
    pub edit: EditRequest,
    pub guidance: Vec<GuidanceImage>,
}
pub fn check_edit(r: &EditRequest) -> Result<(u32, u32)> {
    crate::check_prompt(&r.prompt, false)?;
    if r.image.len().saturating_add(r.mask.len()) > MAX_INPUT_BYTES {
        return Err(GenError::Invalid("image and mask exceed the input byte limit".into()));
    }
    let size = crate::png_size(&r.image)
        .filter(|&(w, h)| w > 0 && h > 0 && w <= 2048 && h <= 2048)
        .ok_or_else(|| GenError::Invalid("input must be a PNG of at most 2048 pixels per side".into()))?;
    if crate::png_size(&r.mask) != Some(size) {
        return Err(GenError::Invalid("edit mask dimensions must match the image".into()));
    }
    Ok(size)
}
pub fn check(r: &GuidedEditRequest) -> Result<(u32, u32)> {
    let size = check_edit(&r.edit)?;
    let mut bytes = r.edit.image.len() + r.edit.mask.len();
    let mut text = 0usize;
    if r.guidance.len() > 10 {
        return Err(GenError::Invalid("at most ten explicit guidance masks".into()));
    }
    let mut seen = std::collections::BTreeSet::new();
    for g in &r.guidance {
        if !SEMANTICS.contains(&g.semantic.as_str()) || !seen.insert(&g.semantic) {
            return Err(GenError::Invalid("unknown or duplicated guidance semantic".into()));
        }
        if crate::png_size(&g.mask) != Some(size) {
            return Err(GenError::Invalid("guidance mask dimensions must match the image".into()));
        }
        bytes = bytes.saturating_add(g.mask.len());
        if bytes > MAX_INPUT_BYTES {
            return Err(GenError::Invalid("guidance exceeds the input byte limit".into()));
        }
        if g.annotations.len() > 256 {
            return Err(GenError::Invalid("too many guidance annotations".into()));
        }
        for a in &g.annotations {
            text = text.saturating_add(a.len());
            if a.len() > 1024 || text > MAX_GUIDANCE_TEXT {
                return Err(GenError::Invalid("guidance annotations exceed the text limit".into()));
            }
        }
    }
    Ok(size)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut p = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        p.extend_from_slice(&w.to_be_bytes());
        p.extend_from_slice(&h.to_be_bytes());
        p
    }
    fn request() -> GuidedEditRequest {
        GuidedEditRequest {
            edit: EditRequest { image: png(64, 64), mask: png(64, 64), prompt: "Synthetic wire-contract test".into(), size: crate::ImageSize::Small },
            guidance: vec![GuidanceImage { semantic: "foldDirection".into(), mask: png(64, 64), annotations: vec!["Follow the marked fold".into()] }],
        }
    }
    #[test]
    fn guidance_is_typed_bounded_and_dimension_matched() {
        assert!(check(&request()).is_ok());
        let mut r = request();
        r.guidance[0].semantic = "red".into();
        assert!(check(&r).is_err());
        let mut r = request();
        r.guidance[0].mask = png(63, 64);
        assert!(check(&r).is_err());
        let mut r = request();
        r.guidance.push(r.guidance[0].clone());
        assert!(check(&r).is_err());
        let mut r = request();
        r.guidance[0].annotations = vec!["界".repeat(342)];
        assert!(check(&r).is_err());
        let mut r = request();
        r.edit.image = png(4096, 4096);
        assert!(check(&r).is_err());
    }
}
