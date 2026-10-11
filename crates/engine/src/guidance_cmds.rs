//! Nondestructive semantic guidance, edited through the normal document history.
use crate::{EngineError, Result, Session, commands::CommandSpec};
use photocraft_doc::guidance::{Guidance, Layer, Semantic, Stroke};
use photocraft_geom::Rect;
use serde_json::{Value, json};
use std::sync::Arc;
#[cfg(test)]
#[path = "guidance_cmds_tests.rs"]
mod tests;
fn bad(e: impl std::fmt::Display) -> EngineError {
    EngineError::BadParams { cmd: "guidance".into(), msg: e.to_string() }
}
fn enabled(s: &Session) -> std::result::Result<(), String> {
    s.active().map(|_| ()).ok_or_else(|| "no document open".into())
}
fn id(p: &Value, key: &str) -> Result<u32> {
    p.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).ok_or_else(|| bad(format!("{key} must be an integer")))
}
fn edit(s: &mut Session, op: impl FnOnce(&mut Guidance) -> Result<Value>) -> Result<Value> {
    s.edit("AI Guidance", |doc, _| {
        doc.guidance.validate().map_err(bad)?;
        let mut g = doc.guidance.clone();
        let result = op(&mut g)?;
        g.validate().map_err(bad)?;
        doc.guidance = g;
        Ok(result)
    })
}
fn layer(g: &mut Guidance, id: u32, allow_locked: bool) -> Result<&mut Layer> {
    let l = Arc::make_mut(&mut g.layers).iter_mut().find(|l| l.id == id).ok_or_else(|| bad("guidance layer not found"))?;
    if l.locked && !allow_locked {
        return Err(bad("guidance layer is locked"));
    }
    Ok(l)
}
fn add_layer(s: &mut Session, p: &Value) -> Result<Value> {
    let name = p.get("name").map_or(Some("Guidance"), Value::as_str).ok_or_else(|| bad("name must be text"))?.to_owned();
    edit(s, |g| {
        let id = g.layers.iter().map(|l| l.id).max().unwrap_or(0) + 1;
        Arc::make_mut(&mut g.layers).push(Layer { id, name, visible: true, locked: false, opacity: 1.0, strokes: Arc::new(Vec::new()) });
        Ok(json!({"layer":id}))
    })
}
fn set_layer(s: &mut Session, p: &Value) -> Result<Value> {
    let n = id(p, "layer")?;
    edit(s, |g| {
        let l = layer(g, n, true)?;
        let mut v = serde_json::to_value(&*l).map_err(bad)?;
        let fields = p.as_object().ok_or_else(|| bad("expected object"))?;
        for (k, value) in fields {
            if k == "layer" {
                continue;
            }
            if !matches!(k.as_str(), "name" | "visible" | "locked" | "opacity") {
                return Err(bad("unknown guidance layer field"));
            }
            if l.locked && k != "locked" {
                return Err(bad("guidance layer is locked"));
            }
            v[k] = value.clone();
        }
        *l = serde_json::from_value(v).map_err(bad)?;
        Ok(json!({"layer":n}))
    })
}
fn remove_layer(s: &mut Session, p: &Value) -> Result<Value> {
    let n = id(p, "layer")?;
    edit(s, |g| {
        layer(g, n, false)?;
        Arc::make_mut(&mut g.layers).retain(|l| l.id != n);
        Ok(json!({}))
    })
}
fn add_stroke(s: &mut Session, p: &Value) -> Result<Value> {
    let n = id(p, "layer")?;
    let mut value = p.get("stroke").cloned().ok_or_else(|| bad("stroke required"))?;
    if value.get("points").and_then(Value::as_array).is_none_or(|p| p.len() > 4096) {
        return Err(bad("too many guidance points"));
    }
    edit(s, |g| {
        let l = layer(g, n, false)?;
        let id = l.strokes.iter().map(|s| s.id).max().unwrap_or(0) + 1;
        value["id"] = json!(id);
        let stroke: Stroke = serde_json::from_value(value).map_err(bad)?;
        stroke.validate().map_err(bad)?;
        Arc::make_mut(&mut l.strokes).push(stroke);
        Ok(json!({"layer":n,"stroke":id}))
    })
}
fn set_stroke(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("points").is_some_and(|v| v.as_array().is_none_or(|p| p.len() > 4096)) {
        return Err(bad("invalid or oversized guidance points"));
    }
    let n = id(p, "layer")?;
    let stroke = id(p, "stroke")?;
    edit(s, |g| {
        let l = layer(g, n, false)?;
        let st = Arc::make_mut(&mut l.strokes).iter_mut().find(|s| s.id == stroke).ok_or_else(|| bad("stroke not found"))?;
        let mut v = serde_json::to_value(&*st).map_err(bad)?;
        for (k, value) in p.as_object().ok_or_else(|| bad("expected object"))? {
            if k == "layer" || k == "stroke" {
                continue;
            }
            if k == "id" || k == "translate" {
                if k == "id" {
                    return Err(bad("stroke id is immutable"));
                }
                continue;
            }
            v[k] = value.clone();
        }
        let mut next: Stroke = serde_json::from_value(v).map_err(bad)?;
        if let Some(t) = p.get("translate") {
            let [dx, dy]: [f32; 2] = serde_json::from_value(t.clone()).map_err(bad)?;
            for p in Arc::make_mut(&mut next.points) {
                p.x += dx;
                p.y += dy;
            }
        }
        next.validate().map_err(bad)?;
        *st = next;
        Ok(json!({"layer":n,"stroke":stroke}))
    })
}
fn remove_stroke(s: &mut Session, p: &Value) -> Result<Value> {
    let n = id(p, "layer")?;
    let stroke = id(p, "stroke")?;
    edit(s, |g| {
        let l = layer(g, n, false)?;
        if !l.strokes.iter().any(|s| s.id == stroke) {
            return Err(bad("stroke not found"));
        }
        Arc::make_mut(&mut l.strokes).retain(|s| s.id != stroke);
        Ok(json!({}))
    })
}
fn get(s: &mut Session, _: &Value) -> Result<Value> {
    serde_json::to_value(&s.active().ok_or(EngineError::NoDocument)?.doc.guidance).map_err(bad)
}
/// Rasterize ONE explicit semantic into a bounded mask. Colors are never interpreted.
/// Analytic swept segments avoid dab interpolation and are charged against a work cap.
pub fn render_mask(g: &Guidance, region: Rect, size: [u32; 2], semantic: Semantic) -> Result<Vec<u8>> {
    g.validate().map_err(bad)?;
    let [w, h] = size;
    if region.is_empty() || w == 0 || h == 0 || w > 2048 || h > 2048 {
        return Err(bad("invalid guidance mask dimensions (maximum 2048 per side)"));
    }
    let sx = w as f32 / region.width() as f32;
    let sy = h as f32 / region.height() as f32;
    let scale = sx.max(sy);
    let mut work = 0u64;
    let mut segments = Vec::new();
    for l in g.layers.iter().filter(|l| l.visible && l.opacity > 0.0) {
        for s in l.strokes.iter().filter(|s| s.semantic == semantic && s.opacity > 0.0) {
            for i in 0..s.points.len().saturating_sub(1).max(1) {
                let a = s.points[i];
                let b = s.points[(i + 1).min(s.points.len() - 1)];
                let ax = (a.x - region.x0 as f32) * sx;
                let ay = (a.y - region.y0 as f32) * sy;
                let bx = (b.x - region.x0 as f32) * sx;
                let by = (b.y - region.y0 as f32) * sy;
                let ra = s.width * 0.5 * scale * if s.pressure_size { a.pressure.max(0.01) } else { 1.0 };
                let rb = s.width * 0.5 * scale * if s.pressure_size { b.pressure.max(0.01) } else { 1.0 };
                let r = ra.max(rb);
                let x0 = (ax.min(bx) - r).floor().max(0.0) as u32;
                let x1 = (ax.max(bx) + r).ceil().clamp(0.0, w as f32) as u32;
                let y0 = (ay.min(by) - r).floor().max(0.0) as u32;
                let y1 = (ay.max(by) + r).ceil().clamp(0.0, h as f32) as u32;
                if x0 >= x1 || y0 >= y1 {
                    continue;
                }
                work += u64::from(x1 - x0) * u64::from(y1 - y0);
                if work > 32_000_000 {
                    return Err(bad("guidance mask exceeds 32 million segment-pixel evaluations; simplify strokes or reduce output size"));
                }
                segments.push((ax, ay, bx, by, ra, rb, x0, y0, x1, y1, s.hardness, s.opacity * l.opacity));
            }
        }
    }
    let mut mask = vec![0u8; (w * h) as usize];
    for (ax, ay, bx, by, ra, rb, x0, y0, x1, y1, hard, opacity) in segments {
        let dx = bx - ax;
        let dy = by - ay;
        let len = dx * dx + dy * dy;
        for y in y0..y1 {
            for x in x0..x1 {
                let px = x as f32 + 0.5 - ax;
                let py = y as f32 + 0.5 - ay;
                let t = if len > 0.0 { ((px * dx + py * dy) / len).clamp(0.0, 1.0) } else { 0.0 };
                let d = ((px - t * dx).powi(2) + (py - t * dy).powi(2)).sqrt();
                let r = ra + (rb - ra) * t;
                let coverage = if d <= r * hard { 1.0 } else { ((r - d) / (r * (1.0 - hard)).max(0.0001)).clamp(0.0, 1.0) };
                let index = (y * w + x) as usize;
                mask[index] = mask[index].max((coverage * opacity * 255.0).round() as u8);
            }
        }
    }
    Ok(mask)
}
fn mask(s: &mut Session, p: &Value) -> Result<Value> {
    let g = &s.active().ok_or(EngineError::NoDocument)?.doc.guidance;
    let region: Rect = serde_json::from_value(p["region"].clone()).map_err(bad)?;
    let size: [u32; 2] = serde_json::from_value(p["size"].clone()).map_err(bad)?;
    let semantic: Semantic = serde_json::from_value(p["semantic"].clone()).map_err(bad)?;
    let bytes = render_mask(g, region, size, semantic)?;
    Ok(json!({"semantic":semantic,"region":region,"size":size,"encoding":"gray8-base64","data":photocraft_paint::tile::b64_encode(&bytes)}))
}
pub fn specs() -> Vec<CommandSpec> {
    [
    ("guidance.get","Read semantic guidance",get as fn(&mut Session,&Value)->Result<Value>),
    ("guidance.layer.add","Add guidance layer",add_layer),
    ("guidance.layer.set","Edit guidance layer",set_layer),
    ("guidance.layer.remove","Remove guidance layer",remove_layer),
    ("guidance.stroke.add","Add guidance stroke",add_stroke),
    ("guidance.stroke.set","Edit guidance stroke",set_stroke),
    ("guidance.stroke.remove","Remove guidance stroke",remove_stroke),
    ("guidance.mask","Render semantic mask",mask),
].into_iter().map(|(id,label,run)|CommandSpec{id,label,menu:&[],shortcut:None,params:r#"layer.add:{name?}; layer.set/remove:{layer,...}; stroke.add:{layer,stroke:{semantic,points:[{x,y,pressure}],width,hardness,opacity,pressureSize,arrow,annotation}}; stroke.set/remove:{layer,stroke,...,translate?:[dx,dy]}; mask:{region,size:[w,h],semantic}. Explicit typed guidance; native document data, one Undo/edit, no artwork pixels."#,enabled,run,journal:!matches!(id,"guidance.get"|"guidance.mask")}).collect()
}
