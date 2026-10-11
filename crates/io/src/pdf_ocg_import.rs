//! Conservative reconstruction of the standard OCG/Form/image profile we export.
//! A profile marker never bypasses validation. General PDF graphics, OCMDs and
//! backdrop dependencies remain raster imports with an explicit warning.
use crate::IoError;
use hayro::hayro_syntax::{
    Pdf,
    object::{Array, Dict, Name, Stream, String as PdfString},
};
use photocraft_color::BlendMode;
use photocraft_doc::{ColorMode, Layer, LayerContent, PixelFormat, Rect, SampleType, Surface};
use photocraft_raster::Interrupt;
use std::{collections::HashSet, io::Read};

fn error(message: &str) -> IoError {
    IoError::Pdf(message.into())
}
fn named(dict: &Dict<'_>, key: &[u8], value: &[u8]) -> bool {
    dict.get::<Name<'_>>(key).is_some_and(|n| n.as_ref() == value)
}
fn text(value: PdfString<'_>) -> String {
    let bytes = value.as_bytes();
    if let Some(body) = bytes.strip_prefix(&[0xfe, 0xff]) {
        String::from_utf16_lossy(
            &body
                .chunks_exact(2)
                .filter_map(|p| {
                    let [a, b] = p else { return None };
                    Some(u16::from_be_bytes([*a, *b]))
                })
                .collect::<Vec<_>>(),
        )
    } else {
        bytes.iter().map(|b| char::from(*b)).collect()
    }
}
fn inflate(stream: &Stream<'_>, ctl: &Interrupt<'_>) -> Result<Vec<u8>, IoError> {
    if !named(stream.dict(), b"Filter", b"FlateDecode") || stream.dict().contains_key(b"DecodeParms") {
        return Err(error("Unsupported OCG stream encoding"));
    }
    let raw = stream.raw_data();
    let mut decoder = flate2::read::ZlibDecoder::new(raw.as_ref());
    let mut output = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        ctl.check().map_err(|_| IoError::Cancelled)?;
        let n = decoder.read(&mut chunk).map_err(|_| error("Invalid OCG stream"))?;
        if n == 0 {
            break;
        }
        if output.len().saturating_add(n) > 64 * 1024 * 1024 {
            return Err(error("OCG stream exceeds 64MB"));
        }
        output.extend_from_slice(chunk.get(..n).ok_or_else(|| error("Invalid inflated byte count"))?);
    }
    Ok(output)
}
fn clean_keys(dict: &Dict<'_>, allowed: &[&[u8]]) -> bool {
    dict.keys().all(|k| allowed.contains(&k.as_ref()))
}
fn same_ops(bytes: &[u8], expected: &str) -> bool {
    std::str::from_utf8(bytes).is_ok_and(|s| s.split_whitespace().eq(expected.split_whitespace()))
}
fn blend(dict: &Dict<'_>) -> Result<BlendMode, IoError> {
    let name = dict.get::<Name<'_>>(b"BM").ok_or_else(|| error("Missing layer blend mode"))?;
    BlendMode::LAYER_MODES
        .iter()
        .copied()
        .find(|b| crate::pdf_layers::blend_name(*b).is_ok_and(|s| s.as_bytes() == name.as_ref()))
        .ok_or_else(|| error("Unsupported OCG blend mode"))
}

/// Readable source OCG hierarchy for the import chooser, without image decoding.
#[derive(Clone, Debug)]
pub struct LayerEntry {
    pub id: i32,
    pub name: String,
    pub depth: usize,
    pub group: bool,
    pub supported: bool,
    pub visible: bool,
}

pub fn inspect(bytes: &[u8]) -> Result<Vec<LayerEntry>, IoError> {
    if cfg!(target_arch = "wasm32") {
        return Err(error("PDF import is desktop-only"));
    }
    if bytes.len() > 256 * 1024 * 1024 {
        return Err(error("PDF exceeds 256MB"));
    }
    std::panic::catch_unwind(|| {
        let pdf = Pdf::new(std::sync::Arc::new(bytes.to_vec())).map_err(|_| error("Cannot inspect PDF layers"))?;
        let Some(props) = properties(&pdf) else { return Ok(Vec::new()) };
        let off = hidden(&props);
        let mut entries = Vec::new();
        let groups = props.get::<Array<'_>>(b"OCGs").ok_or_else(|| error("Invalid OCG catalog"))?;
        for group in groups.iter::<Dict<'_>>() {
            if entries.len() >= 4096 {
                return Err(error("PDF exceeds 4096 optional content groups"));
            }
            let id = group.obj_id().ok_or_else(|| error("OCG must be indirect"))?.obj_number;
            entries.push(LayerEntry {
                id,
                name: group.get::<PdfString<'_>>(b"Name").map(text).unwrap_or_else(|| format!("Layer {id}")),
                depth: 0,
                group: false,
                supported: pdf.pages().iter().all(|p| p.raw().get::<u32>(b"PhotoCraftLayered") == Some(1)),
                visible: !off.contains(&id),
            });
        }
        if let Some(order) = props.get::<Dict<'_>>(b"D").and_then(|d| d.get::<Array<'_>>(b"Order")) {
            let mut ordered = Vec::new();
            order_entries(order, 0, &mut entries, &mut ordered, &mut 0)?;
            entries.sort_by_key(|e| ordered.iter().position(|id| *id == e.id).unwrap_or(usize::MAX));
        }
        Ok(entries)
    })
    .map_err(|_| error("Cannot inspect PDF layer catalog"))?
}
fn properties<'a>(pdf: &'a Pdf) -> Option<Dict<'a>> {
    pdf.xref().get::<Dict<'_>>(pdf.xref().root_id())?.get::<Dict<'_>>(b"OCProperties")
}
fn hidden(props: &Dict<'_>) -> HashSet<i32> {
    let Some(config) = props.get::<Dict<'_>>(b"D") else { return HashSet::new() };
    let mut off = if named(&config, b"BaseState", b"OFF") {
        props.get::<Array<'_>>(b"OCGs").map(|a| a.iter::<Dict<'_>>().filter_map(|d| d.obj_id().map(|i| i.obj_number)).collect()).unwrap_or_default()
    } else {
        HashSet::new()
    };
    if let Some(on) = config.get::<Array<'_>>(b"ON") {
        for group in on.iter::<Dict<'_>>() {
            if let Some(id) = group.obj_id() {
                off.remove(&id.obj_number);
            }
        }
    }
    if let Some(disabled) = config.get::<Array<'_>>(b"OFF") {
        for group in disabled.iter::<Dict<'_>>() {
            if let Some(id) = group.obj_id() {
                off.insert(id.obj_number);
            }
        }
    }
    off
}
fn order_entries(array: Array<'_>, depth: usize, entries: &mut [LayerEntry], order: &mut Vec<i32>, visited: &mut usize) -> Result<(), IoError> {
    if depth > 100 {
        return Err(error("PDF layer hierarchy exceeds 100 levels"));
    }
    let mut parent = None;
    for object in array.iter::<hayro::hayro_syntax::object::Object<'_>>() {
        *visited += 1;
        if *visited > 16384 {
            return Err(error("PDF layer hierarchy exceeds limits"));
        }
        match object {
            hayro::hayro_syntax::object::Object::Dict(d) => {
                parent = d.obj_id().and_then(|id| entries.iter().position(|e| e.id == id.obj_number));
                if let Some(index) = parent
                    && let Some(entry) = entries.get_mut(index)
                {
                    entry.depth = depth;
                    order.push(entry.id);
                }
            }
            hayro::hayro_syntax::object::Object::Array(children) => {
                if let Some(index) = parent
                    && let Some(entry) = entries.get_mut(index)
                {
                    entry.group = true;
                }
                order_entries(children, depth + usize::from(parent.is_some()), entries, order, visited)?;
            }
            _ => {
                parent = None;
            }
        }
    }
    Ok(())
}

struct Reader<'a> {
    off: HashSet<i32>,
    selected: Option<&'a [i32]>,
    ctl: &'a Interrupt<'a>,
    pixels: u64,
    seen: HashSet<i32>,
    allowed: HashSet<i32>,
    y: i32,
    size: (u32, u32),
    paper: (f32, f32),
}
impl Reader<'_> {
    fn layers(&mut self, resources: Dict<'_>, ops: &[u8], depth: usize, parent_selected: bool) -> Result<Vec<Layer>, IoError> {
        if depth > 100 {
            return Err(error("OCG nesting exceeds 100 levels"));
        }
        if !clean_keys(&resources, &[b"XObject", b"ExtGState"]) {
            return Err(error("Unsupported layer resources"));
        }
        let forms = resources.get::<Dict<'_>>(b"XObject").ok_or_else(|| error("Missing OCG forms"))?;
        let graphics = resources.get::<Dict<'_>>(b"ExtGState").ok_or_else(|| error("Missing OCG graphics state"))?;
        if forms.len() != graphics.len() || forms.len() > 4096 {
            return Err(error("Invalid OCG resource count"));
        }
        let mut expected = String::new();
        let mut layers = Vec::new();
        for i in 0..forms.len() {
            self.ctl.check().map_err(|_| IoError::Cancelled)?;
            expected.push_str(&format!("q /G{i} gs /L{i} Do Q\n"));
            let form = forms.get::<Stream<'_>>(format!("L{i}")).ok_or_else(|| error("Unsupported OCG form naming"))?;
            let dict = form.dict();
            if !named(dict, b"Subtype", b"Form")
                || !clean_keys(dict, &[b"Type", b"Subtype", b"BBox", b"OC", b"Resources", b"Group", b"Filter", b"Length", b"PhotoCraftBackground"])
            {
                return Err(error("Unsupported OCG form"));
            }
            let bounds = dict.get::<Array<'_>>(b"BBox").map(|a| a.iter::<f32>().collect::<Vec<_>>()).ok_or_else(|| error("Missing OCG bounds"))?;
            if bounds.len() != 4 || bounds.iter().zip([0., 0., self.paper.0, self.paper.1]).any(|(a, b)| (*a - b).abs() > 0.001) {
                return Err(error("Unsupported OCG bounds"));
            }
            let oc = dict.get::<Dict<'_>>(b"OC").ok_or_else(|| error("Missing OCG association"))?;
            if !named(&oc, b"Type", b"OCG") {
                return Err(error("OCMD cannot be separated safely"));
            }
            let reference = oc.obj_id().ok_or_else(|| error("Missing OCG reference"))?;
            let id = reference.obj_number;
            if reference.gen_number != 0 || !self.allowed.contains(&id) {
                return Err(error("OCG reference is outside the supported catalog"));
            }
            if !self.seen.insert(id) || self.seen.len() > 4096 {
                return Err(error("Repeated/cyclic OCG references"));
            }
            let chosen = parent_selected || self.selected.is_none_or(|ids| ids.contains(&id));
            let state = graphics.get::<Dict<'_>>(format!("G{i}")).ok_or_else(|| error("Missing OCG state"))?;
            if !clean_keys(&state, &[b"Type", b"ca", b"CA", b"BM"]) {
                return Err(error("Unsupported OCG graphics state"));
            }
            let opacity = state.get::<f32>(b"ca").ok_or_else(|| error("Missing OCG opacity"))?;
            if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
                return Err(error("Invalid OCG opacity"));
            }
            let mode = blend(&state)?;
            if let Some(g) = dict.get::<Dict<'_>>(b"Group")
                && (!clean_keys(&g, &[b"S", b"I", b"K", b"CS"])
                    || !named(&g, b"S", b"Transparency")
                    || g.get::<bool>(b"I") != Some(true)
                    || g.get::<bool>(b"K") != Some(false)
                    || !named(&g, b"CS", b"DeviceRGB"))
            {
                return Err(error("Unsupported OCG transparency group"));
            }
            let inner = dict.get::<Dict<'_>>(b"Resources").ok_or_else(|| error("Missing OCG resources"))?;
            let mut content = inflate(&form, self.ctl)?;
            let background = dict.get::<Array<'_>>(b"PhotoCraftBackground").map(|a| a.iter::<f32>().collect::<Vec<_>>());
            if let Some(values) = &background {
                let [r, g, b] = values.as_slice() else {
                    return Err(error("Invalid artboard background"));
                };
                if values.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
                    return Err(error("Invalid artboard background color"));
                }
                let prefix = format!("q {r:.6} {g:.6} {b:.6} rg 0 0 {:.6} {:.6} re f Q\n", self.paper.0, self.paper.1);
                content = content.strip_prefix(prefix.as_bytes()).ok_or_else(|| error("Mismatched artboard background appearance"))?.to_vec();
            }
            let image = inner.get::<Dict<'_>>(b"XObject").and_then(|d| d.get::<Stream<'_>>(b"Im0"));
            let mut layer = if let Some(image) = image {
                if background.is_some()
                    || !clean_keys(&inner, &[b"XObject"])
                    || inner.get::<Dict<'_>>(b"XObject").is_none_or(|d| d.len() != 1)
                    || !same_ops(&content, &format!("q {:.6} 0 0 {:.6} 0 0 cm /Im0 Do Q", self.paper.0, self.paper.1))
                {
                    return Err(error("Unsupported OCG image placement"));
                }
                if !chosen {
                    continue;
                }
                Layer::new("", LayerContent::Raster(self.image(&image)?))
            } else {
                let children = self.layers(inner, &content, depth + 1, chosen)?;
                if children.is_empty() && !chosen {
                    continue;
                }
                let mut group = Layer::group("", children);
                if let Some(values) = &background {
                    let [r, g, b] = values.as_slice() else {
                        return Err(error("Invalid artboard background"));
                    };
                    if let LayerContent::Group(native_group) = &mut group.content {
                        native_group.artboard = Some(photocraft_doc::Artboard {
                            rect: Rect::new(0, self.y, self.size.0 as i32, self.y + self.size.1 as i32),
                            background: photocraft_doc::ArtboardBackground::Custom(photocraft_doc::Color::rgb(*r, *g, *b)),
                            preset: String::new(),
                        });
                    }
                }
                if let Some(g) = dict.get::<Dict<'_>>(b"Group") {
                    if !named(&g, b"S", b"Transparency") || g.get::<bool>(b"I") != Some(true) || g.get::<bool>(b"K") != Some(false) {
                        return Err(error("Unsupported PDF transparency group"));
                    }
                    group.blend = mode;
                } else if mode != BlendMode::Normal || opacity != 1.0 {
                    return Err(error("Unsupported pass-through OCG group state"));
                }
                group
            };
            if !layer.is_group() {
                layer.blend = mode;
            }
            layer.name = oc.get::<PdfString<'_>>(b"Name").map(text).unwrap_or_else(|| format!("Layer {id}"));
            layer.opacity = opacity;
            layer.visible = !self.off.contains(&id);
            layers.push(layer);
        }
        if !same_ops(ops, &expected) {
            return Err(error("PDF contains graphics outside the supported OCG profile"));
        }
        Ok(layers)
    }
    fn image(&mut self, stream: &Stream<'_>) -> Result<Surface, IoError> {
        let d = stream.dict();
        if !clean_keys(d, &[b"Type", b"Subtype", b"Width", b"Height", b"ColorSpace", b"BitsPerComponent", b"Filter", b"Length", b"SMask"])
            || !named(d, b"Subtype", b"Image")
            || d.get::<u32>(b"BitsPerComponent") != Some(8)
        {
            return Err(error("Unsupported OCG image samples"));
        }
        let w = d.get::<u32>(b"Width").ok_or_else(|| error("Missing image width"))?;
        let h = d.get::<u32>(b"Height").ok_or_else(|| error("Missing image height"))?;
        let n = u64::from(w) * u64::from(h);
        self.pixels = self.pixels.saturating_add(u64::from(self.size.0) * u64::from(self.size.1));
        if n == 0 || n > 16_000_000 || w.max(h) > 16384 || self.pixels > 512_000_000 {
            return Err(error("Layered import exceeds pixel limits"));
        }
        let cs = d.get::<Array<'_>>(b"ColorSpace").ok_or_else(|| error("Layered import requires explicit sRGB ICC"))?;
        let mut colors = cs.iter::<hayro::hayro_syntax::object::Object<'_>>();
        if colors.next().and_then(|o| o.into_name()).is_none_or(|n| n.as_ref() != b"ICCBased") {
            return Err(error("Unsupported OCG color space"));
        }
        let profile = colors.next().and_then(|o| o.into_stream()).ok_or_else(|| error("Missing OCG ICC profile"))?;
        if inflate(&profile, self.ctl)? != *photocraft_cms::Builtin::Srgb.profile().to_bytes() {
            return Err(error("OCG image profile is not the supported sRGB profile"));
        }
        let rgb = inflate(stream, self.ctl)?;
        if rgb.len() != n as usize * 3 {
            return Err(error("Invalid OCG RGB byte count"));
        }
        let mask = d.get::<Stream<'_>>(b"SMask").ok_or_else(|| error("Missing OCG soft mask"))?;
        let md = mask.dict();
        if !clean_keys(md, &[b"Type", b"Subtype", b"Width", b"Height", b"ColorSpace", b"BitsPerComponent", b"Filter", b"Length"])
            || md.get::<u32>(b"Width") != Some(w)
            || md.get::<u32>(b"Height") != Some(h)
            || md.get::<u32>(b"BitsPerComponent") != Some(8)
            || !named(md, b"ColorSpace", b"DeviceGray")
        {
            return Err(error("Unsupported OCG soft mask"));
        }
        let alpha = inflate(&mask, self.ctl)?;
        if alpha.len() != n as usize {
            return Err(error("Invalid OCG mask byte count"));
        }
        let rgba: Vec<u8> = rgb.chunks_exact(3).zip(alpha).flat_map(|(p, a)| p.iter().copied().chain(std::iter::once(a))).collect();
        let mut surface = Surface::new(PixelFormat::new(ColorMode::Rgb, SampleType::U8, true));
        surface.write_interleaved(Rect::new(0, 0, w as i32, h as i32), &rgba);
        if (w, h) != self.size {
            surface = photocraft_algo::resample::resize_surface(
                &surface,
                f64::from(self.size.0) / f64::from(w),
                f64::from(self.size.1) / f64::from(h),
                photocraft_algo::resample::Resample::Bilinear,
            );
        }
        if self.y != 0 {
            surface = photocraft_algo::resample::translate_surface(&surface, 0, self.y);
        }
        Ok(surface)
    }
}

/// Called only after the existing full PDF security preflight.
pub(crate) fn page_layers(pdf: &Pdf, index: usize, size: (u32, u32), y: i32, selected: Option<&[i32]>, ctl: &Interrupt) -> Result<Option<Vec<Layer>>, IoError> {
    let Some(props) = properties(pdf) else { return Ok(None) };
    let page = pdf.pages().get(index).ok_or_else(|| error("Invalid page"))?;
    if page.raw().get::<u32>(b"PhotoCraftLayered") != Some(1) {
        return Ok(None);
    }
    if page.raw().contains_key(b"Annots") || page.raw().contains_key(b"Rotate") || page.raw().contains_key(b"CropBox") {
        return Err(error("Modified layered PDF requires explicit flattened import"));
    }
    let config = props.get::<Dict<'_>>(b"D").ok_or_else(|| error("Missing OCG default configuration"))?;
    if (!named(&config, b"BaseState", b"ON") && !named(&config, b"BaseState", b"OFF")) || config.contains_key(b"AS") {
        return Err(error("Unsupported OCG visibility configuration"));
    }
    let catalog = props.get::<Array<'_>>(b"OCGs").ok_or_else(|| error("Missing OCG catalog"))?;
    let mut allowed = HashSet::new();
    for object in catalog.iter::<hayro::hayro_syntax::object::Object<'_>>() {
        if allowed.len() >= 4096 {
            return Err(error("OCG catalog exceeds limits"));
        }
        let group = object.into_dict().ok_or_else(|| error("Invalid OCG catalog entry"))?;
        let id = group.obj_id().ok_or_else(|| error("OCG catalog entry must be indirect"))?;
        if id.gen_number != 0 || !named(&group, b"Type", b"OCG") || !allowed.insert(id.obj_number) {
            return Err(error("Invalid or duplicate OCG catalog entry"));
        }
    }
    let mut reader = Reader { off: hidden(&props), selected, ctl, pixels: 0, seen: HashSet::new(), allowed, y, size, paper: page.render_dimensions() };
    let resources = page.raw().get::<Dict<'_>>(b"Resources").ok_or_else(|| error("Missing page resources"))?;
    let stream = page.page_stream().ok_or_else(|| error("Missing page content"))?;
    reader.layers(resources, stream, 0, false).map(Some)
}
