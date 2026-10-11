//! Layer-aware PDF export. OCG visibility is distinct from Photoshop editing data.
use crate::{
    ExportOptions, ExportResult, IoError, pdf,
    pdf_writer::{self, OptionalContent, OptionalLayer, OptionalPage},
};
use photocraft_color::BlendMode;
use photocraft_doc::{Document, Layer, LayerContent};

fn error(s: impl Into<String>) -> IoError {
    IoError::Pdf(s.into())
}

pub(crate) fn blend_name(blend: BlendMode) -> Result<&'static str, IoError> {
    use BlendMode::*;
    Ok(match blend {
        Normal | PassThrough => "Normal",
        Multiply => "Multiply",
        Screen => "Screen",
        Overlay => "Overlay",
        Darken => "Darken",
        Lighten => "Lighten",
        ColorDodge => "ColorDodge",
        ColorBurn => "ColorBurn",
        HardLight => "HardLight",
        SoftLight => "SoftLight",
        Difference => "Difference",
        Exclusion => "Exclusion",
        Hue => "Hue",
        Saturation => "Saturation",
        Color => "Color",
        Luminosity => "Luminosity",
        _ => return Err(error(format!("PDF cannot preserve the {} blend mode; use flattened PDF plus layered PSD/PSB", blend.label()))),
    })
}

fn validate(layers: &[Layer], depth: usize, leaves: &mut u64, nodes: &mut u64) -> Result<(), IoError> {
    if depth > 100 {
        return Err(error("PDF layer nesting exceeds 100 levels"));
    }
    for layer in layers {
        *nodes += 1;
        if *nodes > 4096 {
            return Err(error("PDF exceeds 4096 layers and groups"));
        }
        blend_name(layer.blend)?;
        if !layer.opacity.is_finite() || !(0.0..=1.0).contains(&layer.opacity) {
            return Err(error("Invalid layer opacity"));
        }
        if layer.clipped
            || layer.excluded_channels != 0
            || layer.blend_if != Default::default()
            || layer.advanced != Default::default()
            || matches!(layer.content, LayerContent::Adjustment(_))
        {
            return Err(error(format!("'{}' depends on its backdrop or advanced blending; use flattened PDF with layered PSD/PSB", layer.name)));
        }
        if let Some(children) = layer.children() {
            if layer.mask.is_some()
                || layer.vector_mask.is_some()
                || (layer.effects.enabled && !layer.effects.items.is_empty())
                || layer.fill_opacity != 1.0
                || (layer.blend == BlendMode::PassThrough && layer.opacity != 1.0)
            {
                return Err(error(format!(
                    "Group '{}' has masks, effects, fill opacity or pass-through opacity not supported in independent PDF layers; use flattened PDF plus PSD/PSB",
                    layer.name
                )));
            }
            validate(children, depth + 1, leaves, nodes)?;
        } else {
            *leaves += 1;
        }
    }
    Ok(())
}

fn encode_layers(
    doc: &Document,
    layers: &[Layer],
    done: &mut u64,
    count: u64,
    progress: &mut dyn FnMut(f32) -> Result<(), String>,
) -> Result<Vec<OptionalLayer>, IoError> {
    let mut output = Vec::new();
    for layer in layers {
        progress(*done as f32 / count.max(1) as f32).map_err(error)?;
        let content = if let Some(children) = layer.children() {
            OptionalContent::Group(encode_layers(doc, children, done, count, progress)?)
        } else {
            let mut one = doc.clone();
            let mut appearance = layer.clone();
            appearance.visible = true;
            appearance.blend = BlendMode::Normal;
            appearance.opacity = 1.0;
            one.layers = vec![appearance];
            let image = pdf::render_image(&one, doc.bounds(), &mut |f| progress((*done as f32 + f) / count.max(1) as f32))?;
            *done += 1;
            OptionalContent::Image(image)
        };
        let background = if let Some(board) = layer.artboard() {
            if board.rect != doc.bounds() || layer.opacity != 1.0 || !matches!(layer.blend, BlendMode::Normal | BlendMode::PassThrough) {
                return Err(error(
                    "OCG export supports canvas-sized artboards with normal/pass-through blending and full opacity; use flattened PDF plus PSD/PSB otherwise",
                ));
            }
            match board.background.rgba() {
                Some([r, g, b, a]) if a == 1.0 && [r, g, b].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) => Some(background_srgb(doc, [r, g, b])?),
                None => None,
                _ => return Err(error("Translucent artboard backgrounds require flattened PDF plus PSD/PSB")),
            }
        } else {
            None
        };
        output.push(OptionalLayer {
            name: layer.name.clone(),
            visible: layer.visible,
            opacity: layer.opacity,
            blend: blend_name(layer.blend)?,
            isolated: !layer.is_group() || layer.blend != BlendMode::PassThrough,
            background,
            content,
        });
    }
    Ok(output)
}

// Artboard background operators need the same output color space as raster leaves.
fn background_srgb(doc: &Document, rgb: [f32; 3]) -> Result<[f32; 3], IoError> {
    use photocraft_cms::{Builtin, ColorSpace, Intent, Profile, Transform};
    if doc.mode != photocraft_doc::ColorMode::Rgb {
        return Err(error("OCG artboard backgrounds require an RGB document; use flattened PDF plus PSD/PSB"));
    }
    let Some(bytes) = doc.icc_profile.as_deref() else { return Ok(rgb) };
    let profile = Profile::parse(bytes).map_err(|e| error(e.to_string()))?;
    if profile.color_space != ColorSpace::Rgb {
        return Err(error("Artboard RGB profile has an incompatible color space"));
    }
    let transform = Transform::new(&profile, Builtin::Srgb.profile(), Intent::RelativeColorimetric, true).map_err(|e| error(e.to_string()))?;
    let mut output = [0.0; 3];
    transform.convert_f32(&rgb, 3, &mut output, 3, false);
    if output.iter().any(|c| !c.is_finite()) {
        return Err(error("Invalid artboard color conversion"));
    }
    Ok(output.map(|c| c.clamp(0.0, 1.0)))
}

/// Each source tab is a page; each selected layer is an independent OCG/Form resource.
/// Pixel masks and supported effects are baked into each leaf's alpha/appearance.
/// Unsupported backdrop dependencies fail explicitly instead of silently flattening.
pub fn export_ocg(docs: &[&Document], progress: &mut dyn FnMut(f32) -> Result<(), String>) -> Result<ExportResult, IoError> {
    if docs.is_empty() || docs.len() > 100 {
        return Err(error("Select 1 to 100 PDF documents"));
    }
    let mut count = 0;
    let mut nodes = 0;
    let mut total = 0u64;
    for doc in docs {
        let before = count;
        validate(&doc.layers, 0, &mut count, &mut nodes)?;
        let pixels = u64::from(doc.size.width) * u64::from(doc.size.height);
        // These match the existing bounded image decoder (RGB and soft mask both count).
        if pixels == 0 || pixels > 16_000_000 || doc.size.width.max(doc.size.height) > 16384 {
            return Err(error("Layered PDF appearances exceed the existing 16-megapixel embedded-image limit"));
        }
        total = total.saturating_add(pixels.saturating_mul(count - before).saturating_mul(2));
        if total > 64_000_000 || count > 4096 {
            return Err(error("Layered PDF exceeds 64 megapixels across embedded images and masks, or 4096 layers"));
        }
        if !doc.resolution_dpi.is_finite()
            || doc.resolution_dpi <= 0.0
            || f64::from(doc.size.width.max(doc.size.height)) * 72.0 / f64::from(doc.resolution_dpi) > 14400.0
        {
            return Err(error("Invalid PDF resolution or physical page size"));
        }
    }
    let mut done = 0;
    let mut pages = Vec::new();
    for doc in docs {
        pages.push(OptionalPage {
            name: doc.name.clone(),
            paper: (f64::from(doc.size.width) * 72.0 / f64::from(doc.resolution_dpi), f64::from(doc.size.height) * 72.0 / f64::from(doc.resolution_dpi)),
            layers: encode_layers(doc, &doc.layers, &mut done, count, progress)?,
        });
    }
    progress(1.0).map_err(error)?;
    Ok(ExportResult { bytes: pdf_writer::write_optional_pdf(&pages, None), warnings: vec!["PDF contains standard OCG layers with 8-bit sRGB raster appearances; masks/effects are baked per leaf, type/vector/Smart Object editability is not retained. OCGs do not establish editable Photoshop PDF compatibility. Use the matching PSD/PSB for editing; direct Photoshop PDF verification is pending.".into()] })
}

/// Export isolated eligible leaves on separate canvas-sized pages. Backdrop-dependent
/// features are rejected using the same validation as OCG export.
pub fn export_layers_as_pages(docs: &[&Document], opts: &ExportOptions, progress: &mut dyn FnMut(f32) -> Result<(), String>) -> Result<ExportResult, IoError> {
    fn collect(doc: &Document, layers: &[Layer], output: &mut Vec<Document>) -> Result<(), IoError> {
        for layer in layers {
            if let Some(children) = layer.children() {
                if layer.opacity != 1.0 || !matches!(layer.blend, BlendMode::Normal | BlendMode::PassThrough) {
                    return Err(error("Layers as pages cannot preserve group blending/opacity; use OCG or flatten"));
                }
                collect(doc, children, output)?;
            } else {
                let mut one = doc.clone();
                let mut leaf = layer.clone();
                leaf.visible = true;
                one.layers = vec![leaf];
                output.push(one);
                if output.len() > 100 {
                    return Err(error("Layers as pages exceeds 100 pages"));
                }
            }
        }
        Ok(())
    }
    let mut pages = Vec::new();
    for doc in docs {
        validate(&doc.layers, 0, &mut 0, &mut 0)?;
        collect(doc, &doc.layers, &mut pages)?;
    }
    let refs: Vec<_> = pages.iter().collect();
    pdf::export_pdf_canvases(&refs, opts, progress)
}

/// A single atomic ZIP delivery, containing the PDF and a true layered PSD/PSB per tab.
/// No cross-file partial commit can close tabs before the editing files exist.
pub fn paired_package(
    docs: &[&Document],
    mut pdf: ExportResult,
    force_psb: bool,
    progress: &mut dyn FnMut(f32) -> Result<(), String>,
) -> Result<ExportResult, IoError> {
    let mut zip = photocraft_format::zip::ZipWriter::new();
    zip.add("binder.pdf", &pdf.bytes)?;
    let mut files = Vec::new();
    for (i, doc) in docs.iter().enumerate() {
        progress(i as f32 / docs.len().max(1) as f32).map_err(error)?;
        let opts = ExportOptions { force_psb, ..Default::default() };
        let out = crate::export(doc, if force_psb { "psb" } else { "psd" }, &opts)?;
        // The existing PSD writer automatically upgrades oversized documents to PSB.
        let ext = if out.bytes.get(4..6) == Some(&[0, 2]) { "psb" } else { "psd" };
        let filename = format!("document-{:03}.{ext}", i + 1);
        zip.add(&filename, &out.bytes)?;
        files.push(serde_json::json!({"file": filename, "document":doc.name, "documentId": doc.id.0}));
        pdf.warnings.extend(out.warnings.into_iter().map(|w| format!("{}: {w}", doc.name)));
    }
    let manifest = serde_json::json!({"format":"PhotoCraft paired export", "version":1, "pdf":"binder.pdf", "documents":files, "photoshopPdfVerification":"pending; OCG visibility does not prove Photoshop editing compatibility", "warnings":pdf.warnings});
    zip.add("manifest.json", &serde_json::to_vec_pretty(&manifest).map_err(|e| error(e.to_string()))?)?;
    progress(1.0).map_err(error)?;
    Ok(ExportResult { bytes: zip.finish()?, warnings: pdf.warnings })
}
