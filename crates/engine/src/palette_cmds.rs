//! A bounded artist's mixing surface. This is tool state, not document artwork.
use crate::{EngineError, Result, Session, commands::CommandSpec};
use photocraft_color::PixelFormat;
use photocraft_geom::Rect;
use photocraft_paint::{
    BrushSettings, GrayTile, Stroke, StrokePoint,
    mixer::{MixerSettings, MixerState},
};
use photocraft_raster::Surface;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SIDE: u32 = 256;
pub const MAX_PRESETS: usize = 8;
pub type Image = [GrayTile; 4];
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    pub paint: Image,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub version: u32,
    pub paint: Image,
    pub skin: Option<Image>,
    pub presets: Vec<Preset>,
}

#[derive(Clone, Debug)]
pub struct Palette {
    pub paint: Surface,
    pub skin: Option<Image>,
    pub presets: Vec<Preset>,
    pub revision: u64,
    history: Vec<Surface>,
    mix: MixerState,
}
impl Default for Palette {
    fn default() -> Self {
        Self { paint: Surface::new(PixelFormat::RGBA16), skin: None, presets: Vec::new(), revision: 0, history: Vec::new(), mix: MixerState::default() }
    }
}
pub fn bounds() -> Rect {
    Rect::new(0, 0, SIDE as i32, SIDE as i32)
}
pub fn contact(x: f32, y: f32) -> bool {
    let (u, v) = ((x - 128.0) / 124.0, (y - 128.0) / 116.0);
    let hole = ((x - 62.0) / 19.0).powi(2) + ((y - 178.0) / 27.0).powi(2);
    u * u + v * v <= 1.0 && hole >= 1.0
}
fn selection() -> Surface {
    let mut s = Surface::new(PixelFormat::new(photocraft_color::ColorMode::Grayscale, photocraft_color::SampleType::F32, false));
    s.write_region(
        bounds(),
        &(0..SIDE).flat_map(|y| (0..SIDE).map(move |x| if contact(x as f32 + 0.5, y as f32 + 0.5) { 1.0 } else { 0.0 })).collect::<Vec<_>>(),
    );
    s
}
pub fn image(surface: &Surface) -> Image {
    std::array::from_fn(|channel| GrayTile::from_fn(SIDE, SIDE, |x, y| surface.rgba(x as i32, y as i32)[channel]))
}
fn valid_image(image: &Image) -> bool {
    image.iter().all(|plane| plane.width == SIDE && plane.height == SIDE && plane.is_valid())
}
pub fn surface(image: &Image) -> Result<Surface> {
    if !valid_image(image) {
        return Err(bad("palette images must be four 256 x 256 GrayTile planes"));
    }
    let mut s = Surface::new(PixelFormat::RGBA16);
    let mut data = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for i in 0..(SIDE * SIDE) as usize {
        for plane in image {
            data.push(f32::from(plane.data.get(i).copied().unwrap_or(0)) / 65535.0);
        }
    }
    s.write_region(bounds(), &data);
    Ok(s)
}
impl Palette {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { version: 1, paint: image(&self.paint), skin: self.skin.clone(), presets: self.presets.clone() }
    }
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1).max(1);
    }
    fn remember(&mut self) {
        if self.history.len() == 16 {
            self.history.remove(0);
        }
        self.history.push(self.paint.clone());
    }
}
fn bad(msg: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: "palette".into(), msg: msg.into() }
}
fn always(_: &Session) -> std::result::Result<(), String> {
    Ok(())
}
fn color_transform(s: &Session, to_palette: bool) -> Result<Option<photocraft_cms::Transform>> {
    let Some(st) = s.active() else { return Ok(None) };
    let doc = crate::color_cmds::composite_profile(&st.doc);
    let srgb = photocraft_cms::builtin::srgb();
    if doc.content_hash() == srgb.content_hash() {
        return Ok(None);
    }
    let (src, dst) = if to_palette { (doc.as_ref(), srgb) } else { (srgb, doc.as_ref()) };
    photocraft_cms::Transform::new(src, dst, photocraft_cms::Intent::RelativeColorimetric, true)
        .map(Some)
        .map_err(|e| bad(format!("palette color transform: {e}")))
}
fn transformed(color: [f32; 4], t: Option<&photocraft_cms::Transform>) -> [f32; 4] {
    let mut out = color;
    if let Some(t) = t {
        t.eval(&color[..3], &mut out[..3]);
    }
    out.map(|v| v.clamp(0.0, 1.0))
}
fn flag(p: &Value, key: &str, default: bool) -> Result<bool> {
    match p.get(key) {
        None => Ok(default),
        Some(v) => v.as_bool().ok_or_else(|| bad(format!("{key} must be boolean"))),
    }
}
fn number(p: &Value, key: &str, default: f64, min: f64, max: f64) -> Result<f64> {
    let n = match p.get(key) {
        None => default,
        Some(v) => v.as_f64().ok_or_else(|| bad(format!("{key} must be numeric")))?,
    };
    if !n.is_finite() || !(min..=max).contains(&n) {
        return Err(bad(format!("{key} must be between {min} and {max}")));
    }
    Ok(n)
}
fn points(p: &Value) -> Result<Vec<StrokePoint>> {
    let raw = p
        .get("points")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .ok_or_else(|| bad("points requires 1..128 [x,y,pressure] samples"))?;
    let mut out = Vec::with_capacity(raw.len());
    for raw in raw {
        let a = raw.as_array().filter(|a| (2..=3).contains(&a.len())).ok_or_else(|| bad("each sample is [x,y,pressure?]"))?;
        let coord = |i: usize, default: f64, max: f64| -> Result<f64> {
            let v = a.get(i).map_or(Some(default), Value::as_f64).ok_or_else(|| bad("sample is not numeric"))?;
            if !v.is_finite() || !(0.0..=max).contains(&v) {
                return Err(bad("sample outside palette bounds"));
            }
            Ok(v)
        };
        out.push(StrokePoint::new(coord(0, 0.0, 255.0)?, coord(1, 0.0, 255.0)?, coord(2, 1.0, 1.0)? as f32));
    }
    Ok(out)
}
fn paint(s: &mut Session, p: &Value, mix: bool) -> Result<Value> {
    let points = points(p)?;
    let size = number(p, "size", 24.0, 2.0, 128.0)? as f32;
    let begin = flag(p, "begin", true)?;
    let opacity = number(p, "opacity", 1.0, 0.0, 1.0)? as f32;
    let color = match p.get("color") {
        Some(v) => <[f32; 4]>::deserialize(v).map_err(|e| bad(e.to_string()))?,
        None => s.tools.foreground,
    };
    if color.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
        return Err(bad("palette colors must be finite SDR RGBA values in 0..1"));
    }
    let color = transformed(color, color_transform(s, true)?.as_ref());
    let brush = BrushSettings { size, hardness: 0.75, spacing: 0.2, pressure_size: false, opacity, color, ..Default::default() };
    let stroke = Stroke { brush, points };
    if photocraft_paint::dabs(&stroke).len() > 4096 {
        return Err(bad("palette gesture exceeds 4096 dabs; use fewer points"));
    }
    let mask = selection();
    let palette = &mut s.tools.artist_palette;
    if begin {
        palette.remember();
        palette.mix = MixerState::default();
    }
    if mix {
        photocraft_paint::mixer::apply_mixer_stroke(
            &mut palette.paint,
            None,
            &stroke,
            &MixerSettings { wet: 1.0, mix: 1.0, flow: opacity, load: 0.0, ..Default::default() },
            &mut palette.mix,
            Some(&mask),
            false,
        );
    } else {
        photocraft_paint::apply_stroke(&mut palette.paint, &stroke, Some(&mask), false);
    }
    palette.changed();
    Ok(json!({"revision":palette.revision}))
}
fn deposit(s: &mut Session, p: &Value) -> Result<Value> {
    paint(s, p, false)
}
fn mix(s: &mut Session, p: &Value) -> Result<Value> {
    paint(s, p, true)
}
fn deposit_load(s: &mut Session, p: &Value) -> Result<Value> {
    let x = number(p, "x", 128.0, 0.0, 255.0)? as f32;
    let y = number(p, "y", 96.0, 0.0, 255.0)? as f32;
    let size = number(p, "size", 48.0, 2.0, 128.0)? as f32;
    let load = s.tools.mixer.spatial.clone();
    let uniform = s.tools.mixer.reservoir;
    if load.is_none() && uniform.is_none() {
        return Err(bad("load paint from the canvas first with Alt-click"));
    }
    let transform = color_transform(s, true)?;
    let amount = s.tools.mixer.amount.clamp(0.0, 1.0);
    let palette = &mut s.tools.artist_palette;
    palette.remember();
    let rect = Rect::new((x - size / 2.0).floor() as i32, (y - size / 2.0).floor() as i32, (x + size / 2.0).ceil() as i32, (y + size / 2.0).ceil() as i32)
        .intersect(&bounds());
    let mut pixels = palette.paint.read_region(rect);
    for (i, pixel) in pixels.chunks_exact_mut(4).enumerate() {
        let (px, py) = (rect.x0 + (i % rect.width() as usize) as i32, rect.y0 + (i / rect.width() as usize) as i32);
        if !contact(px as f32 + 0.5, py as f32 + 0.5) {
            continue;
        }
        let u = (((px as f32 + 0.5 - x) / size + 0.5) * 64.0).clamp(0.0, 63.0) as usize;
        let v = (((py as f32 + 0.5 - y) / size + 0.5) * 64.0).clamp(0.0, 63.0) as usize;
        let color = load.as_ref().and_then(|l| l.get(v * 64 + u)).copied().or(uniform).unwrap_or([0.0; 4]);
        let color = transformed(color, transform.as_ref());
        let alpha = (color[3] * amount).clamp(0.0, 1.0);
        let old = pixel[3] * (1.0 - alpha);
        let total = alpha + old;
        if total > 0.0 {
            for k in 0..3 {
                pixel[k] = (pixel[k] * old + color[k] * alpha) / total;
            }
            pixel[3] = total;
        }
    }
    palette.paint.write_region(rect, &pixels);
    palette.changed();
    Ok(json!({"revision":palette.revision}))
}
fn pickup(s: &mut Session, p: &Value) -> Result<Value> {
    let x = number(p, "x", 128.0, 0.0, 255.0)?;
    let y = number(p, "y", 128.0, 0.0, 255.0)?;
    let size = number(p, "size", 24.0, 2.0, 128.0)? as f32;
    let transform = color_transform(s, false)?;
    let color = transformed(s.tools.artist_palette.paint.rgba(x as i32, y as i32), transform.as_ref());
    let spatial: Vec<_> = (0..64)
        .flat_map(|row| (0..64).map(move |col| (row, col)))
        .map(|(row, col)| {
            let sx = x as f32 + ((col as f32 + 0.5) / 64.0 - 0.5) * size;
            let sy = y as f32 + ((row as f32 + 0.5) / 64.0 - 0.5) * size;
            transformed(s.tools.artist_palette.paint.rgba(sx.floor() as i32, sy.floor() as i32), transform.as_ref())
        })
        .collect();
    if spatial.iter().all(|v| v[3] <= 0.0) {
        return Err(bad("there is no paint under this pickup area"));
    }
    let mut average = [0.0; 4];
    for c in &spatial {
        for k in 0..3 {
            average[k] += c[k] * c[3];
        }
        average[3] += c[3];
    }
    let weight = average[3];
    for v in average.iter_mut().take(3) {
        *v /= weight;
    }
    average[3] = 1.0;
    s.tools.mixer.load(average);
    s.tools.mixer.clean();
    s.tools.mixer.spatial = Some(spatial);
    if color[3] > 0.0 {
        s.tools.foreground = color;
    }
    Ok(json!({"loaded":true,"color":average}))
}
fn clear(s: &mut Session, _: &Value) -> Result<Value> {
    let p = &mut s.tools.artist_palette;
    p.remember();
    p.paint = Surface::new(PixelFormat::RGBA16);
    p.mix = MixerState::default();
    p.changed();
    Ok(json!({"cleared":true}))
}
fn undo(s: &mut Session, _: &Value) -> Result<Value> {
    let p = &mut s.tools.artist_palette;
    let old = p.history.pop().ok_or_else(|| bad("palette undo is empty"))?;
    p.paint = old;
    p.mix = MixerState::default();
    p.changed();
    Ok(json!({"undone":true}))
}
pub fn parse_snapshot(raw: &Value) -> Result<Snapshot> {
    fn check_image(raw: &Value) -> Result<()> {
        let planes = raw.as_array().filter(|v| v.len() == 4).ok_or_else(|| bad("palette image needs four planes"))?;
        for p in planes {
            if p.get("width").and_then(Value::as_u64) != Some(256) || p.get("height").and_then(Value::as_u64) != Some(256) {
                return Err(bad("palette image dimensions must be 256 square"));
            }
            let bounded = match p.get("data") {
                Some(Value::String(s)) => s.len() <= 174764,
                Some(Value::Array(a)) => a.len() == 65536,
                _ => false,
            };
            if !bounded {
                return Err(bad("palette image data exceeds its fixed bound"));
            }
        }
        Ok(())
    }
    check_image(raw.get("paint").ok_or_else(|| bad("missing palette paint"))?)?;
    if let Some(skin) = raw.get("skin").filter(|v| !v.is_null()) {
        check_image(skin)?;
    }
    let presets =
        raw.get("presets").and_then(Value::as_array).filter(|v| v.len() <= MAX_PRESETS).ok_or_else(|| bad("at most eight palette presets are supported"))?;
    for p in presets {
        check_image(p.get("paint").ok_or_else(|| bad("preset has no paint"))?)?;
    }
    let snapshot = Snapshot::deserialize(raw).map_err(|e| bad(e.to_string()))?;
    if snapshot.version != 1 || snapshot.presets.iter().any(|p| p.name.trim().is_empty() || p.name.len() > 128) {
        return Err(bad("unsupported palette version or preset name"));
    }
    Ok(snapshot)
}
fn restore(s: &mut Session, p: &Value) -> Result<Value> {
    let snapshot = parse_snapshot(p)?;
    let paint = surface(&snapshot.paint)?;
    let palette = &mut s.tools.artist_palette;
    palette.remember();
    palette.paint = paint;
    palette.skin = snapshot.skin;
    palette.presets = snapshot.presets;
    palette.mix = MixerState::default();
    palette.changed();
    Ok(json!({"restored":true}))
}
fn get(s: &mut Session, _: &Value) -> Result<Value> {
    serde_json::to_value(s.tools.artist_palette.snapshot()).map_err(|e| bad(e.to_string()))
}
fn preset_save(s: &mut Session, p: &Value) -> Result<Value> {
    let name =
        p.get("name").and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty() && v.len() <= 128).ok_or_else(|| bad("name requires 1..128 bytes"))?;
    let palette = &mut s.tools.artist_palette;
    let existing = palette.presets.iter().position(|v| v.name == name);
    if existing.is_none() && palette.presets.len() >= MAX_PRESETS {
        return Err(bad("eight presets already saved; replace a named preset"));
    }
    let preset = Preset { name: name.into(), paint: image(&palette.paint) };
    if let Some(old) = existing.and_then(|i| palette.presets.get_mut(i)) {
        *old = preset;
    } else {
        palette.presets.push(preset);
    }
    palette.changed();
    Ok(json!({"saved":name}))
}
fn preset_load(s: &mut Session, p: &Value) -> Result<Value> {
    let name = p.get("name").and_then(Value::as_str).ok_or_else(|| bad("preset name is required"))?;
    let palette = &mut s.tools.artist_palette;
    let preset = palette.presets.iter().find(|v| v.name == name).ok_or_else(|| bad("unknown palette preset"))?;
    let paint = surface(&preset.paint)?;
    palette.remember();
    palette.paint = paint;
    palette.mix = MixerState::default();
    palette.changed();
    Ok(json!({"loaded":name}))
}
fn skin(s: &mut Session, p: &Value) -> Result<Value> {
    let transform = color_transform(s, true)?;
    let skin = if flag(p, "clear", false)? {
        None
    } else {
        let doc = &s.active().ok_or(EngineError::NoDocument)?.doc;
        let buf = photocraft_compose::thumbnail_buffer(doc, SIDE);
        let mut surface = Surface::new(PixelFormat::RGBA16);
        let width = buf.rect.width() as usize;
        let height = buf.rect.height() as usize;
        if width == 0 || height == 0 {
            return Err(bad("skin source image is empty"));
        }
        let mut pixels = Vec::with_capacity((SIDE * SIDE * 4) as usize);
        for y in 0..SIDE as usize {
            for x in 0..SIDE as usize {
                let at = (y * height / SIDE as usize) * width + x * width / SIDE as usize;
                pixels.extend_from_slice(&transformed(buf.px.get(at).copied().unwrap_or([0.0; 4]), transform.as_ref()));
            }
        }
        surface.write_region(bounds(), &pixels);
        Some(image(&surface))
    };
    s.tools.artist_palette.skin = skin;
    s.tools.artist_palette.changed();
    Ok(json!({"skin":s.tools.artist_palette.skin.is_some()}))
}
pub fn specs() -> Vec<CommandSpec> {
    [ ("palette.deposit","Deposit palette paint",deposit as fn(&mut Session,&Value)->Result<Value>),
      ("palette.mix","Mix palette paint",mix), ("palette.pickup","Pick up palette paint",pickup),
      ("palette.depositLoad","Deposit sampled canvas paint",deposit_load),
      ("palette.clear","Clear palette paint",clear),("palette.undo","Undo palette paint",undo),
      ("palette.get","Get artist palette",get),("palette.restore","Restore artist palette",restore),
      ("palette.presets.save","Save palette preset",preset_save),("palette.presets.load","Load palette preset",preset_load),
      ("palette.skin","Use current image as palette skin",skin)]
    .into_iter().map(|(id,label,run)|CommandSpec {id,label,menu:&[],shortcut:None,params:r#"deposit/mix:{points:[[x,y,pressure?],...],size:2..128,opacity:0..1,color:[r,g,b,a]?,begin:bool=true}; pickup:{x,y,size}; preset:{name}; skin:{clear:bool=false}; restore:versioned palette.get snapshot. Tool state only; canvas coordinates 0..255."#,enabled:always,run,journal:id!="palette.get"}).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_tool_edits_do_not_drop_a_floating_canvas_selection() {
        let mut s=Session::new();s.execute("file.new",json!({"width":64,"height":64,"background":"#405080"})).unwrap();
        s.execute("select.all",json!({})).unwrap();s.execute("select.float",json!({"dx":4,"dy":0,"copy":true})).unwrap();
        let before=s.active().unwrap().doc.clone();let steps=s.active().unwrap().history.past_len();
        s.execute("palette.deposit",json!({"points":[[128,128]],"color":[1,0,0,1]})).unwrap();
        s.execute("palette.pickup",json!({"x":128,"y":128})).unwrap();s.execute("palette.skin",json!({})).unwrap();
        assert!(crate::float_cmds::floating(s.active().unwrap()).is_some());
        assert!(std::sync::Arc::ptr_eq(&before,&s.active().unwrap().doc));assert_eq!(s.active().unwrap().history.past_len(),steps);
    }
    #[test]
    fn palette_converts_linear_document_colors_and_preserves_canvas_load_variation() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width":64,"height":64})).unwrap();
        s.edit("linear profile", |d, _| {
            d.icc_profile = Some(photocraft_cms::Builtin::LinearSrgb.profile().to_bytes());
            Ok(())
        })
        .unwrap();
        s.execute("palette.deposit", json!({"points":[[128,96]],"size":32,"color":[0.214,0.214,0.214,1]})).unwrap();
        let pixel = s.tools.artist_palette.paint.rgba(128, 96);
        assert!((pixel[0] - 0.5).abs() < 0.003, "{pixel:?}");
        s.execute("palette.pickup", json!({"x":128,"y":96,"size":8})).unwrap();
        assert!((s.tools.foreground[0] - 0.214).abs() < 0.003);
        s.tools.mixer.load([1.0, 0.0, 0.0, 1.0]);
        s.tools.mixer.spatial = Some((0..4096).map(|i| if i % 64 < 32 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] }).collect());
        s.execute("palette.depositLoad", json!({"x":128,"y":160,"size":48})).unwrap();
        assert!(s.tools.artist_palette.paint.rgba(115, 160)[0] > 0.99);
        assert!(s.tools.artist_palette.paint.rgba(141, 160)[2] > 0.99);
    }
    #[test]
    fn palette_deposit_mix_pickup_undo_and_presets_preserve_document() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width":64,"height":64})).unwrap();
        let original = s.active().unwrap().doc.clone();
        let steps = s.active().unwrap().history.past_len();
        for (x, c) in [(96, [1, 0, 0, 1]), (150, [0, 0, 1, 1])] {
            s.execute("palette.deposit", json!({"points":[[x,100]],"size":60,"color":c})).unwrap();
        }
        let before = s.tools.artist_palette.paint.clone();
        s.execute("palette.mix", json!({"points":[[96,100],[160,100],[96,100]],"size":30})).unwrap();
        let c = s.tools.artist_palette.paint.rgba(140, 100);
        assert!(c[0] > 0.01 && c[2] > 0.01, "{c:?}");
        s.execute("palette.pickup", json!({"x":140,"y":100})).unwrap();
        assert!(s.tools.mixer.spatial.is_some());
        s.execute("palette.undo", json!({})).unwrap();
        assert_eq!(s.tools.artist_palette.paint, before);
        s.execute("palette.presets.save", json!({"name":"Study"})).unwrap();
        let snapshot = s.execute("palette.get", json!({})).unwrap();
        s.execute("palette.clear", json!({})).unwrap();
        s.execute("palette.presets.load", json!({"name":"Study"})).unwrap();
        assert_eq!(s.tools.artist_palette.paint, before);
        let mut restored = Session::new();
        restored.execute("palette.restore", snapshot).unwrap();
        assert_eq!(restored.tools.artist_palette.paint, before);
        assert_eq!(*s.active().unwrap().doc, *original);
        assert_eq!(s.active().unwrap().history.past_len(), steps);
    }
    #[test]
    fn palette_bounds_thumbhole_and_hostile_params_are_safe() {
        let mut s = Session::new();
        for p in [
            json!({}),
            json!({"points":[[0,0]],"size":1e20}),
            json!({"points":[[-1,0]]}),
            json!({"points":[[1,1,"x"]]}),
            json!({"points":[[1,1]],"begin":"bad"}),
        ] {
            assert!(s.execute("palette.deposit", p).is_err());
        }
        let before = s.tools.artist_palette.paint.clone();
        s.execute("palette.deposit", json!({"points":[[62,178]],"size":8})).unwrap();
        assert_eq!(s.tools.artist_palette.paint.read_region(bounds()), before.read_region(bounds()));
        let mut snapshot = s.execute("palette.get", json!({})).unwrap();
        snapshot["paint"][0]["width"] = json!(1000000);
        assert!(s.execute("palette.restore", snapshot).is_err());
    }
}
