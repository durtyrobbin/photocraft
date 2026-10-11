//! Bounded request preparation layered on the upstream ImageProvider abstraction.
//! Rendering happens after request review, on the existing background-job path.
use crate::{EngineError, Result};
use photocraft_codecs::{self as codecs, ChannelLayout, Format, Image};
use photocraft_color::{ColorMode, PixelFormat, SampleType};
use photocraft_doc::{Document, LayerId, ai_result::EditRecipe, guidance::Semantic};
use photocraft_genai::{EditRequest, GuidanceImage, GuidedEditRequest, ImageProvider, ImageSize};
use photocraft_geom::Rect;
use photocraft_raster::Surface;
use serde_json::json;
use std::sync::Arc;

fn bad(e: impl std::fmt::Display) -> EngineError {
    EngineError::BadParams { cmd: "edit.generativeFill".into(), msg: e.to_string() }
}

/// Cheap review data. No pixels are rendered until the user starts the job.
#[derive(Clone)]
pub struct ReviewedEdit {
    pub(super) document: Arc<Document>,
    pub(super) provider: Arc<dyn ImageProvider>,
    pub recipe: EditRecipe,
    pub context: Rect,
    pub input_size: [u32; 2],
    pub replace: Option<LayerId>,
}
impl ReviewedEdit {
    pub fn disclosure(&self) -> serde_json::Value {
        let info = self.provider.info();
        json!({"provider":info.name,"model":info.model,"endpoint":self.provider.endpoint(),"localOnly":self.provider.local_only(),"recipe":self.recipe,"context":self.context,"inputSize":self.input_size,"guidanceIncluded":true,"sourceComposite":"current visible artwork; previous result hidden during regeneration","outputMapping":"provider image fitted to disclosed context, then cropped to region; no source pixels replaced"})
    }
}

pub(super) fn review(document: Arc<Document>, provider: Arc<dyn ImageProvider>, recipe: EditRecipe, replace: Option<LayerId>) -> Result<ReviewedEdit> {
    recipe.validate().map_err(bad)?;
    document.guidance.validate().map_err(bad)?;
    if !provider.capabilities().edit {
        return Err(bad("provider does not support image editing"));
    }
    if !matches!(document.mode, ColorMode::Rgb | ColorMode::Grayscale | ColorMode::Cmyk | ColorMode::Lab) {
        return Err(bad("convert a copy to RGB, Gray, CMYK or Lab before image editing"));
    }
    if recipe.region.intersect(&document.bounds()) != recipe.region {
        return Err(bad("edit region is outside the canvas"));
    }
    if recipe.output_size != [recipe.region.width(), recipe.region.height()] {
        return Err(bad("outputSize must match the edited region; resize the document separately"));
    }
    if let Some(id) = replace
        && (document.layer(id).is_none_or(|l| l.ai_result.is_none()) || document.effective_locks(id).all)
    {
        return Err(bad("previous result is missing or locked"));
    }
    let p = recipe.context_pixels as i32;
    let r = recipe.region;
    let mut context = Rect::new(r.x0.saturating_sub(p), r.y0.saturating_sub(p), r.x1.saturating_add(p), r.y1.saturating_add(p)).intersect(&document.bounds());
    // Preserve the provider's aspect without stretching the scene. Grow context only
    // inside the document, and disclose that exact rectangle before confirmation.
    let (a, b) = provider.edit_aspect(context.width(), context.height());
    if a == 0 || b == 0 {
        return Err(bad("provider returned an invalid aspect ratio"));
    }
    let (cw, ch) = (u64::from(context.width()), u64::from(context.height()));
    let (want_w, want_h) =
        if cw * u64::from(b) < ch * u64::from(a) { ((ch * u64::from(a)).div_ceil(u64::from(b)), ch) } else { (cw, (cw * u64::from(b)).div_ceil(u64::from(a))) };
    if want_w > 2048 || want_h > 2048 || want_w > u64::from(document.size.width) || want_h > u64::from(document.size.height) {
        return Err(bad(
            "context cannot match this provider's aspect within the canvas and 2048-pixel bound; reduce region/context or choose another provider",
        ));
    }
    let grow = |lo: i32, hi: i32, want: u64, limit: u32| {
        let want = want as i32;
        let limit = limit.min(i32::MAX as u32) as i32;
        let start = (lo - (want - (hi - lo)) / 2).clamp(0, limit - want);
        (start, start + want)
    };
    let (x0, x1) = grow(context.x0, context.x1, want_w, document.size.width);
    let (y0, y1) = grow(context.y0, context.y1, want_h, document.size.height);
    context = Rect::new(x0, y0, x1, y1);
    // Render bounds are checked BEFORE composition or allocation, including sparse huge docs.
    if context.width() > 2048 || context.height() > 2048 {
        return Err(bad("region plus context must fit within 2048 pixels per side; reduce the region or context"));
    }
    let input_size =
        if provider.local_only() { [context.width().div_ceil(8) * 8, context.height().div_ceil(8) * 8] } else { [context.width(), context.height()] };
    let prompt = format!("{}\nMaterial: {}\nLighting: {}", recipe.prompt, recipe.material, recipe.lighting);
    photocraft_genai::check_prompt(&prompt, false).map_err(bad)?;
    Ok(ReviewedEdit { document, provider, recipe, context, input_size, replace })
}

fn encode(s: &Surface, r: Rect) -> Result<Vec<u8>> {
    let rgba = s.convert(PixelFormat::new(ColorMode::Rgb, SampleType::U8, true));
    let img = Image::from_u8(r.width(), r.height(), ChannelLayout::Rgba, rgba.to_interleaved(r)).map_err(bad)?;
    codecs::encode(&img, Format::Png, &Default::default()).map_err(bad)
}
fn gray_png(bytes: Vec<u8>, size: [u32; 2]) -> Result<Vec<u8>> {
    let img = Image::from_u8(size[0], size[1], ChannelLayout::Gray, bytes).map_err(bad)?;
    codecs::encode(&img, Format::Png, &Default::default()).map_err(bad)
}
fn resize(s: &Surface, area: Rect, size: [u32; 2]) -> Surface {
    let mut local = Surface::new(s.format());
    local.write_region(Rect::from_xywh(0, 0, area.width(), area.height()), &s.read_region(area));
    if size == [area.width(), area.height()] {
        local
    } else {
        photocraft_algo::resample::resize_surface(
            &local,
            f64::from(size[0]) / f64::from(area.width()),
            f64::from(size[1]) / f64::from(area.height()),
            photocraft_algo::resample::Resample::Bicubic,
        )
    }
}

pub(super) struct PreparedPixels {
    pub request: GuidedEditRequest,
    pub mask: Surface,
    pub request_digest: String,
    pub guidance_digest: String,
}
/// Compose only the disclosed context. Protected pixels are enforced locally, regardless of
/// provider behavior. All masks, prompts and annotations are validated before any network call.
pub(super) fn prepare(review: &ReviewedEdit, ctx: &crate::jobs::JobCtx) -> Result<PreparedPixels> {
    ctx.check()?;
    let mut source = (*review.document).clone();
    if let Some(id) = review.replace {
        source.layer_mut(id).ok_or_else(|| bad("previous result missing"))?.visible = false;
    }
    let doc = &source;
    let r = review.context;
    let input = Rect::from_xywh(0, 0, review.input_size[0], review.input_size[1]);
    let rgb = PixelFormat::new(ColorMode::Rgb, SampleType::U8, true);
    let mut composite = Surface::new(rgb);
    photocraft_compose::render_bands(doc, r, 0, |band| -> Result<()> {
        ctx.check()?;
        let mut data = vec![0.0; band.px.len() * 4];
        for (p, out) in band.px.iter().zip(data.as_chunks_mut::<4>().0.iter_mut()) {
            photocraft_raster::from_rgba_into(&rgb, *p, out);
        }
        composite.write_region(band.rect, &data);
        Ok(())
    })?;
    let transform = photocraft_cms::Transform::new(
        &crate::color_cmds::composite_profile(doc),
        photocraft_cms::Builtin::Srgb.profile(),
        photocraft_cms::Intent::RelativeColorimetric,
        true,
    )
    .map_err(bad)?;
    let composite = crate::color_cmds::convert_surface(&composite, ColorMode::Rgb, rgb, &transform);
    let image = encode(&resize(&composite, r, review.input_size), input)?;
    ctx.check()?;
    let full_size = [r.width(), r.height()];
    let protect = crate::guidance_cmds::render_mask(&doc.guidance, r, full_size, Semantic::AreaProtect)?;
    let modify = crate::guidance_cmds::render_mask(&doc.guidance, r, full_size, Semantic::AreaModify)?;
    let restrict_modify = modify.iter().any(|&v| v > 0);
    let mut mask = Surface::new(PixelFormat::new(ColorMode::Grayscale, SampleType::F32, false));
    let roi = review.recipe.region;
    let mut nonzero = false;
    for y in roi.y0..roi.y1 {
        ctx.check()?;
        let row: Vec<f32> = (roi.x0..roi.x1)
            .map(|x| {
                let i = ((y - r.y0) as u32 * r.width() + (x - r.x0) as u32) as usize;
                let selection = doc.selection.as_ref().map_or(1.0, |s| s.sample_channel(x, y, 0));
                let v = selection.clamp(0.0, 1.0) * (1.0 - protect[i] as f32 / 255.0) * if restrict_modify { modify[i] as f32 / 255.0 } else { 1.0 };
                nonzero |= v > 0.0;
                v
            })
            .collect();
        mask.write_region(Rect::new(roi.x0, y, roi.x1, y + 1), &row);
    }
    if !nonzero {
        return Err(bad("selection and guidance leave no editable pixels"));
    }
    let scaled_mask = resize(&mask, r, review.input_size);
    let mask_bytes = scaled_mask.read_region(input).iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).collect();
    let mut guidance = Vec::new();
    for semantic in Semantic::ALL {
        ctx.check()?;
        // These two semantics have already been implemented in the exact final edit mask.
        if matches!(semantic, Semantic::AreaModify | Semantic::AreaProtect) {
            continue;
        }
        let coverage = crate::guidance_cmds::render_mask(&doc.guidance, r, review.input_size, semantic)?;
        if !coverage.iter().any(|&v| v > 0) {
            continue;
        }
        if !review.provider.supports_guidance() {
            return Err(bad(
                "this provider cannot accept the visible semantic guidance; select a compatible provider or explicitly hide those guidance layers",
            ));
        }
        let mut annotations = Vec::new();
        for layer in doc.guidance.layers.iter().filter(|l| l.visible && l.opacity > 0.0) {
            for stroke in layer.strokes.iter().filter(|s| s.semantic == semantic && s.opacity > 0.0) {
                // Include only intersecting strokes, accounting for their full brush radius.
                let margin = stroke.width * 0.5;
                let min_x = stroke.points.iter().map(|p| p.x).fold(f32::INFINITY, f32::min) - margin;
                let max_x = stroke.points.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max) + margin;
                let min_y = stroke.points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min) - margin;
                let max_y = stroke.points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max) + margin;
                if max_x < r.x0 as f32 || min_x > r.x1 as f32 || max_y < r.y0 as f32 || min_y > r.y1 as f32 {
                    continue;
                }
                let to_input = |p: &photocraft_doc::guidance::Point| {
                    [
                        (p.x - r.x0 as f32) * review.input_size[0] as f32 / r.width() as f32,
                        (p.y - r.y0 as f32) * review.input_size[1] as f32 / r.height() as f32,
                    ]
                };
                let first = stroke.points.first().ok_or_else(|| bad("empty guidance"))?;
                let last = stroke.points.last().ok_or_else(|| bad("empty guidance"))?;
                annotations.push(json!({"annotation":stroke.annotation,"directionArrow":stroke.arrow,"start":to_input(first),"end":to_input(last),"geometry":"corresponding coverage mask; start/end indicate stroke direction in input pixel coordinates"}).to_string());
            }
        }
        guidance.push(GuidanceImage {
            semantic: serde_json::to_value(semantic).map_err(bad)?.as_str().ok_or_else(|| bad("semantic encoding"))?.to_owned(),
            mask: gray_png(coverage, review.input_size)?,
            annotations,
        });
    }
    let request = GuidedEditRequest {
        edit: EditRequest {
            image,
            mask: gray_png(mask_bytes, review.input_size)?,
            prompt: format!("{}\nMaterial: {}\nLighting: {}", review.recipe.prompt, review.recipe.material, review.recipe.lighting),
            size: ImageSize::for_long_side(review.input_size[0].max(review.input_size[1])),
        },
        guidance,
    };
    photocraft_genai::guidance::check(&request).map_err(bad)?;
    let mut hash = blake3::Hasher::new();
    hash.update(b"PhotoCraft guided image edit v1");
    for bytes in [&request.edit.image[..], &request.edit.mask[..], request.edit.prompt.as_bytes()] {
        hash.update(&(bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    for g in &request.guidance {
        hash.update(g.semantic.as_bytes());
        hash.update(&g.mask);
        for a in &g.annotations {
            hash.update(a.as_bytes());
        }
    }
    let guidance_digest = blake3::hash(&serde_json::to_vec(&doc.guidance).map_err(bad)?).to_hex().to_string();
    Ok(PreparedPixels { request, mask, request_digest: hash.finalize().to_hex().to_string(), guidance_digest })
}
