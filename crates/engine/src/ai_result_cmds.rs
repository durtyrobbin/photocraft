//! Bounded, separate result layers. Importing a result does not call a model.
use crate::{EngineError, Result, Session, commands::CommandSpec};
use photocraft_codecs::{self as codecs, ChannelLayout, Format, SampleType as CSample};
use photocraft_color::{ColorMode, PixelFormat, SampleType};
use photocraft_doc::{
    Document, Layer, LayerContent, LayerId, LayerMask,
    ai_result::{AiResult, EditRecipe, ResultOrigin, ResultStatus},
};
use photocraft_geom::Rect;
use photocraft_raster::Surface;
use serde_json::{Value, json};
use std::sync::Arc;

#[cfg(test)]
#[path = "ai_result_cmds_tests.rs"]
mod tests;

pub const MAX_BYTES: usize = 48 << 20;
fn bad(e: impl std::fmt::Display) -> EngineError {
    EngineError::BadParams { cmd: "ai.result".into(), msg: e.to_string() }
}
fn enabled(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no document open".into())
}
fn result_id(s: &Session, p: &Value) -> Result<LayerId> {
    let st = s.active().ok_or(EngineError::NoDocument)?;
    let id = match p.get("layer") {
        None => st.active_layer.ok_or_else(|| bad("select a result layer"))?,
        Some(v) => LayerId(v.as_u64().ok_or_else(|| bad("layer must be an integer"))?),
    };
    if st.doc.layer(id).is_none_or(|l| l.ai_result.is_none()) {
        return Err(bad("layer is not an AI result"));
    }
    Ok(id)
}
fn bytes(p: &Value) -> Result<Vec<u8>> {
    if let Some(data) = p.get("data") {
        if p.get("path").is_some() {
            return Err(bad("give data or path, not both"));
        }
        let data = data.as_str().filter(|d| d.len() <= MAX_BYTES / 3 * 4 + 4).ok_or_else(|| bad("result data exceeds 48 MiB or is not base64 text"))?;
        let bytes = photocraft_paint::tile::b64_decode(data).ok_or_else(|| bad("invalid result base64"))?;
        if bytes.len() > MAX_BYTES {
            return Err(bad("result exceeds 48 MiB"));
        }
        return Ok(bytes);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let path =
            p.get("path").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 32768).ok_or_else(|| bad("give an image path or base64 data"))?;
        let file = std::fs::File::open(path).map_err(|e| bad(format!("cannot read result: {e}")))?;
        if file.metadata().map_err(bad)?.len() > MAX_BYTES as u64 {
            return Err(bad("result exceeds 48 MiB"));
        }
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes).map_err(bad)?;
        if bytes.len() > MAX_BYTES {
            return Err(bad("result exceeds 48 MiB"));
        }
        Ok(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    Err(bad("use base64 data in the browser"))
}

/// Decode first with allocation limits, then preserve the image's channel model,
/// depth, alpha and ICC conversion into the current document.
pub(crate) fn decode_into_document(bytes: &[u8], doc: &Document, area: Rect) -> Result<Surface> {
    if !matches!(doc.mode, ColorMode::Rgb | ColorMode::Grayscale | ColorMode::Cmyk | ColorMode::Lab) {
        return Err(bad("result import supports RGB, Grayscale, CMYK and Lab; convert a copy of this document before importing"));
    }
    if bytes.len() > MAX_BYTES {
        return Err(bad("result exceeds 48 MiB"));
    }
    let format =
        codecs::detect(bytes).filter(|f| matches!(f, Format::Png | Format::Jpeg | Format::WebP)).ok_or_else(|| bad("result must be PNG, JPEG or WebP"))?;
    let opts = codecs::DecodeOptions {
        limits: codecs::Limits { max_width: 4096, max_height: 4096, max_pixels: 16_777_216, max_alloc: 256 << 20 },
        keep_orientation: false,
    };
    let img = codecs::decode_as_with(format, bytes, &opts).map_err(bad)?;
    if !img.warnings.is_empty() {
        return Err(bad("result is incomplete or contains unsupported additional frames"));
    }
    if img.dimensions() != (area.width(), area.height()) {
        return Err(bad("result dimensions must match the requested region"));
    }
    let (mode, layout) = match img.layout() {
        ChannelLayout::Rgb | ChannelLayout::Rgba => (ColorMode::Rgb, ChannelLayout::Rgba),
        ChannelLayout::Gray | ChannelLayout::GrayA => (ColorMode::Grayscale, ChannelLayout::GrayA),
        ChannelLayout::Cmyk | ChannelLayout::CmykA => (ColorMode::Cmyk, ChannelLayout::CmykA),
    };
    let (sample, depth) = match img.sample_type() {
        CSample::U8 => (CSample::U8, SampleType::U8),
        CSample::U16 => (CSample::U16, SampleType::U16),
        _ => (CSample::F32, SampleType::F32),
    };
    let profile = match &img.icc {
        Some(icc) => {
            let p = photocraft_cms::Profile::parse(icc).map_err(bad)?;
            if p.color_space != crate::color_cmds::mode_space(mode) {
                return Err(bad("result ICC profile does not match its channels"));
            }
            Arc::new(p)
        }
        None => crate::color_cmds::working_profile(mode),
    };
    let img = img.convert(layout, sample);
    let mut surface = Surface::new(PixelFormat::new(mode, depth, true));
    surface.write_interleaved(area, img.data());
    let fmt = PixelFormat::new(doc.mode, doc.depth, true);
    let transform =
        photocraft_cms::Transform::new(&profile, &crate::color_cmds::document_profile(doc), photocraft_cms::Intent::RelativeColorimetric, true).map_err(bad)?;
    let converted = crate::color_cmds::convert_surface(&surface, mode, fmt, &transform);
    Ok(if converted.format() == fmt { converted } else { converted.convert(fmt) })
}

/// Hash the actual current input region and selection without retaining a second
/// full-size composite. This is import-time context, not a claim about an external run.
pub(crate) fn input_digest(doc: &Document, area: Rect) -> Result<String> {
    let mut hash = blake3::Hasher::new();
    hash.update(b"PhotoCraft result import context v1");
    hash.update(&serde_json::to_vec(&area).map_err(bad)?);
    hash.update(&crate::color_cmds::composite_profile(doc).to_bytes());
    photocraft_compose::render_bands(doc, area, 0, |band| -> Result<()> {
        for (i, pixel) in band.px.iter().enumerate() {
            for channel in pixel {
                hash.update(&channel.to_le_bytes());
            }
            let x = band.rect.x0 + (i % band.rect.width() as usize) as i32;
            let y = band.rect.y0 + (i / band.rect.width() as usize) as i32;
            let coverage = doc.selection.as_ref().map_or(1.0, |s| s.sample_channel(x, y, 0));
            hash.update(&coverage.to_le_bytes());
        }
        Ok(())
    })?;
    Ok(hash.finalize().to_hex().to_string())
}
pub(crate) fn region_mask(doc: &Document, area: Rect) -> Surface {
    let mut mask = Surface::new(PixelFormat::new(ColorMode::Grayscale, SampleType::F32, false));
    // Bounded rows; unrelated selection pixels never enter this result's mask.
    for y in area.y0..area.y1 {
        let values: Vec<_> = (area.x0..area.x1).map(|x| doc.selection.as_ref().map_or(1.0, |s| s.sample_channel(x, y, 0))).collect();
        mask.write_region(Rect::new(area.x0, y, area.x1, y + 1), &values);
    }
    mask
}
pub(crate) fn place(s: &mut Session, pixels: Surface, mask: Surface, record: AiResult, replace: Option<LayerId>) -> Result<Value> {
    record.validate().map_err(bad)?;
    let bounds = record.recipe.region;
    let id = s.edit("AI Result", |doc, active| {
        if let Some(old) = replace {
            if doc.effective_locks(old).all {
                return Err(bad("previous result is locked"));
            }
            let old_layer = doc.layer_mut(old).filter(|l| l.ai_result.is_some()).ok_or_else(|| bad("previous result no longer exists"))?;
            old_layer.visible = false;
        }
        let mut layer = Layer::new(doc.next_layer_name("AI Result"), LayerContent::Raster(pixels));
        layer.mask = Some(LayerMask { surface: mask, ..LayerMask::reveal_all() });
        layer.ai_result = Some(Arc::new(record));
        // A full-composite proposal must not inherit a selected group's blend or be composited beneath already-included upper layers.
        let id = doc.insert_above(None, layer);
        *active = Some(id);
        Ok(id)
    })?;
    Ok(json!({"layer":id.0,"bounds":bounds}))
}
fn import(s: &mut Session, p: &Value) -> Result<Value> {
    let recipe: EditRecipe = serde_json::from_value(p.get("recipe").cloned().ok_or_else(|| bad("recipe is required"))?).map_err(bad)?;
    recipe.validate().map_err(bad)?;
    if recipe.output_size != [recipe.region.width(), recipe.region.height()] {
        return Err(bad("import outputSize must match region dimensions"));
    }
    let st = s.active().ok_or(EngineError::NoDocument)?;
    if recipe.region.intersect(&st.doc.bounds()) != recipe.region {
        return Err(bad("result region lies outside the canvas"));
    }
    let replace = if let Some(value) = p.get("replace") { Some(result_id(s, &json!({"layer":value}))?) } else { None };
    let provider = p.get("provider").map_or(Some("Imported image"), Value::as_str).ok_or_else(|| bad("provider must be text"))?.to_owned();
    let model = match p.get("model") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_str().ok_or_else(|| bad("model must be text"))?.to_owned()),
    };
    let mut record = AiResult {
        version: 1,
        origin: ResultOrigin::Imported,
        status: ResultStatus::Proposed,
        provider,
        model,
        endpoint: None,
        created_unix_seconds: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        input_digest: "0".repeat(64),
        result_digest: "0".repeat(64),
        recipe,
    };
    record.validate().map_err(bad)?;
    let doc = st.doc.clone();
    let params = p.clone();
    crate::jobs::run(
        s,
        "Import AI Result",
        true,
        move |ctx| {
            ctx.check()?;
            ctx.progress(0.05, "Validating result image");
            let bytes = bytes(&params)?;
            let pixels = decode_into_document(&bytes, &doc, record.recipe.region)?;
            ctx.check()?;
            ctx.progress(0.6, "Preparing result layer");
            record.input_digest = input_digest(&doc, record.recipe.region)?;
            record.result_digest = blake3::hash(&bytes).to_hex().to_string();
            let mask = region_mask(&doc, record.recipe.region);
            ctx.check()?;
            Ok((pixels, mask, record))
        },
        move |s, (pixels, mask, record)| place(s, pixels, mask, record, replace),
    )
}
fn get(s: &mut Session, p: &Value) -> Result<Value> {
    let id = result_id(s, p)?;
    let layer = s.active().and_then(|st| st.doc.layer(id)).ok_or(EngineError::NoLayer(id))?;
    Ok(json!({"layer":id.0,"provenance":layer.ai_result,"visible":layer.visible,"opacity":layer.opacity}))
}
fn accept(s: &mut Session, p: &Value) -> Result<Value> {
    let id = result_id(s, p)?;
    s.edit("Accept AI Result", |doc, _| {
        if doc.effective_locks(id).all {
            return Err(bad("result is locked"));
        }
        let record = doc.layer_mut(id).and_then(|l| l.ai_result.as_mut()).ok_or_else(|| bad("missing result record"))?;
        Arc::make_mut(record).status = ResultStatus::Accepted;
        Ok(())
    })?;
    Ok(json!({"layer":id.0,"accepted":true}))
}
fn reject(s: &mut Session, p: &Value) -> Result<Value> {
    let id = result_id(s, p)?;
    s.edit("Reject AI Result", |doc, active| {
        if doc.effective_locks(id).all {
            return Err(bad("result is locked"));
        }
        doc.remove(id).ok_or(EngineError::NoLayer(id))?;
        if *active == Some(id) {
            *active = None;
        }
        Ok(())
    })?;
    Ok(json!({"removed":id.0}))
}
pub fn specs() -> Vec<CommandSpec> {
    [("ai.result.import","Import AI Result",import as fn(&mut Session,&Value)->Result<Value>),
     ("ai.result.get","Inspect AI Result",get),("ai.result.accept","Accept AI Result",accept),("ai.result.reject","Reject AI Result",reject)]
    .into_iter().map(|(id,label,run)|CommandSpec {id,label,menu:&[],shortcut:None,
        params:r#"import:{data:base64?|path:str?,recipe:{prompt,material?,lighting?,region:{x0,y0,x1,y1},contextPixels?,outputSize:[w,h]},provider?:str,model?:str,replace?:layerId}. Others:{layer?:id}. Import records claimed attribution; no model or network call. New editable masked layer, one Undo."#,
        enabled,run,journal:id!="ai.result.get"}).collect()
}
