//! Bounded tip-local pigment cells. No particles or per-bristle simulation.
use crate::{
    Dab,
    mixer::{MixerSettings, MixerState, PICKUP_RATE},
    retouch::{Footprint, Region, alpha_index, over_native},
};
use photocraft_color::PixelFormat;
use photocraft_raster::{from_rgba_into, to_rgba};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cell {
    pub loaded: [f32; 4],
    pub pickup: Option<[f32; 4]>,
    pub amount: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reservoir {
    pub side: u32,
    pub cells: Vec<Cell>,
}
impl Reservoir {
    pub fn valid(&self) -> bool {
        let color = |c: &[f32; 4]| c.iter().all(|v| v.is_finite() && v.abs() <= 1_000_000.0);
        [16, 32, 64].contains(&self.side)
            && self.cells.len() == (self.side * self.side) as usize
            && self.cells.iter().all(|c| color(&c.loaded) && c.pickup.as_ref().is_none_or(color) && c.amount.is_finite() && (0.0..=1.0).contains(&c.amount))
    }
    fn new(side: u32, state: &MixerState) -> Self {
        let cells = (0..side * side)
            .map(|i| {
                let col = i % side;
                let row = i / side;
                let at = ((row * 64 + 32) / side).min(63) * 64 + ((col * 64 + 32) / side).min(63);
                let loaded = state.spatial.as_ref().and_then(|v| v.get(at as usize)).copied().or(state.reservoir).unwrap_or([0.0; 4]);
                Cell { loaded, pickup: state.pickup, amount: state.amount.clamp(0.0, 1.0) }
            })
            .collect();
        Self { side, cells }
    }
}
/// Precompute the same projection/rotation/aspect/flip used by `mixer::load_at`.
struct Mapping {
    cx: f32,
    cy: f32,
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    side: usize,
}
impl Mapping {
    fn new(dab: &Dab, side: u32) -> Self {
        let (ps, pc) = dab.proj_angle.sin_cos();
        let extra = if dab.proj_scale < 0.999 { 1.0 / dab.proj_scale.max(0.05) - 1.0 } else { 0.0 };
        let (xx, xy, yx, yy) = (1.0 + extra * pc * pc, -extra * pc * ps, -extra * pc * ps, 1.0 + extra * ps * ps);
        let (s, c) = dab.angle.sin_cos();
        let u = if dab.flip_x { -1.0 } else { 1.0 } / dab.radius.max(0.01);
        let v = if dab.flip_y { -1.0 } else { 1.0 } / (dab.radius * dab.roundness).max(0.5);
        Self {
            cx: dab.center.x as f32,
            cy: dab.center.y as f32,
            a: (xx * c - yx * s) * u,
            b: (xy * c - yy * s) * u,
            c: (xx * s + yx * c) * v,
            d: (xy * s + yy * c) * v,
            side: side as usize,
        }
    }
    fn at(&self, x: i32, y: i32) -> Option<usize> {
        let dx = x as f32 + 0.5 - self.cx;
        let dy = y as f32 + 0.5 - self.cy;
        let u = self.a * dx + self.b * dy;
        let v = self.c * dx + self.d * dy;
        if !(-1.0..1.0).contains(&u) || !(-1.0..1.0).contains(&v) {
            return None;
        }
        let col = ((u + 1.0) * 0.5 * self.side as f32) as usize;
        let row = ((v + 1.0) * 0.5 * self.side as f32) as usize;
        Some(row * self.side + col)
    }
}
#[derive(Clone, Copy, Default)]
struct Acc {
    color: [f32; 5],
    weight: f32,
    alpha: f32,
    pixels: u32,
}
pub fn apply(work: &mut Region, fp: &Footprint, fmt: PixelFormat, sample: Option<&Region>, m: &MixerSettings, state: &mut MixerState, lock: bool) {
    let side = m.reservoir_grid;
    if ![16, 32, 64].contains(&side) {
        return;
    }
    let mut reservoir = match state.local.take().filter(Reservoir::valid) {
        Some(old) if old.side == side => old,
        Some(old) => {
            let cells = (0..side * side)
                .filter_map(|i| {
                    let x = ((i % side) * old.side + old.side / 2) / side;
                    let y = ((i / side) * old.side + old.side / 2) / side;
                    old.cells.get((y.min(old.side - 1) * old.side + x.min(old.side - 1)) as usize).cloned()
                })
                .collect();
            Reservoir { side, cells }
        }
        None => Reservoir::new(side, state),
    };
    let map = Mapping::new(&fp.dab, side);
    let n = fmt.channels();
    let nc = fmt.mode.color_channels();
    let alpha = alpha_index(&fmt);
    let r = fp.rect.intersect(&work.rect);
    let mut acc = vec![Acc::default(); reservoir.cells.len()];
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            let Some(a) = map.at(x, y).and_then(|i| acc.get_mut(i)) else { continue };
            a.pixels += 1;
            let w = fp.at(x, y);
            if w <= 0.0 {
                continue;
            }
            let px = sample.unwrap_or(work).px(x, y);
            let al = alpha.map_or(1.0, |i| px.get(i).copied().unwrap_or(0.0));
            for (c, v) in a.color.iter_mut().take(nc).enumerate() {
                *v += px.get(c).copied().unwrap_or(0.0) * w * al;
            }
            a.weight += w;
            a.alpha += w * al;
        }
    }
    let wet = m.wet.clamp(0.0, 1.0) * fp.dab.wet.clamp(0.0, 1.0);
    let mix = m.mix.clamp(0.0, 1.0) * fp.dab.mix.clamp(0.0, 1.0);
    let flow = m.flow.clamp(0.0, 1.0);
    let load = m.load.clamp(0.0, 1.0);
    let mut colors = vec![[0.0f32; 5]; reservoir.cells.len()];
    let mut strength = vec![0.0; reservoir.cells.len()];
    for (((cell, a), color), strength) in reservoir.cells.iter_mut().zip(&acc).zip(&mut colors).zip(&mut strength) {
        if a.weight <= 0.0 {
            continue;
        }
        let mut pickup = [0.0; 5];
        if let Some(p) = cell.pickup {
            from_rgba_into(&fmt, p, &mut pickup[..n]);
        }
        if a.alpha > 1e-6 && wet > 0.0 {
            let rate = if cell.pickup.is_some() { PICKUP_RATE * wet * (a.alpha / a.weight).clamp(0.0, 1.0) } else { 1.0 };
            for (c, value) in pickup.iter_mut().take(nc).enumerate() {
                *value += (a.color[c] / a.alpha - *value) * rate;
            }
            if let Some(i) = alpha {
                pickup[i] = 1.0;
            }
            cell.pickup = Some(to_rgba(&fmt, &pickup[..n]));
        }
        let mut loaded = [0.0; 5];
        from_rgba_into(&fmt, cell.loaded, &mut loaded[..n]);
        let wr = cell.amount * cell.loaded[3].clamp(0.0, 1.0) * (1.0 - mix * wet);
        let wp = if cell.pickup.is_some() { mix * wet } else { 0.0 };
        let total = wr + wp;
        if total > 1e-6 {
            for (c, value) in color.iter_mut().take(nc).enumerate() {
                *value = (loaded[c] * wr + pickup[c] * wp) / total;
            }
            if let Some(i) = alpha {
                color[i] = 1.0;
            }
            *strength = total.min(1.0) * flow;
        }
        // Contact coverage varies across a soft/textured tip; untouched cells retain paint.
        let contact = (a.weight / a.pixels.max(1) as f32).clamp(0.0, 1.0);
        cell.amount *= 1.0 - 0.08 * (1.0 - load) * flow * contact;
    }
    for y in r.y0..r.y1 {
        for x in r.x0..r.x1 {
            let Some(i) = map.at(x, y) else { continue };
            let k = fp.at(x, y) * strength.get(i).copied().unwrap_or(0.0);
            if k > 0.0
                && let Some(color) = colors.get(i)
            {
                over_native(&fmt, work.px_mut(x, y), &color[..n], k, lock);
            }
        }
    }
    state.amount = reservoir.cells.iter().map(|c| c.amount).sum::<f32>() / reservoir.cells.len().max(1) as f32;
    state.pickup = reservoir.cells.iter().find_map(|c| c.pickup);
    state.local = Some(reservoir);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BrushContext, BrushSettings, Stroke, StrokePoint};
    use photocraft_geom::Rect;
    use photocraft_raster::Surface;
    #[test]
    fn local_pickup_keeps_two_colors_and_uneven_contact_depletes_locally() {
        for side in [16, 32, 64] {
            let fmt = PixelFormat::RGBA32F;
            let mut source = Surface::new(fmt);
            source.fill_rect(Rect::new(0, 0, 128, 64), &[1.0, 0.0, 0.0, 1.0]);
            source.fill_rect(Rect::new(0, 64, 128, 128), &[0.0, 0.0, 1.0, 1.0]);
            let b = BrushSettings { size: 96.0, hardness: 0.1, pressure_size: false, ..Default::default() };
            let stroke = Stroke { brush: b.clone(), points: vec![StrokePoint::new(64.0, 64.0, 1.0)] };
            let dab = crate::dabs(&stroke)[0];
            let ctx = BrushContext::new(&b);
            let fp = ctx.footprint(&dab, 0);
            let mut work = Region::read(&Surface::new(fmt), Rect::new(0, 0, 128, 128));
            let sample = Region::read(&source, work.rect);
            let mut state = MixerState::default();
            state.load([0.0, 1.0, 0.0, 1.0]);
            let m = MixerSettings { wet: 1.0, mix: 1.0, load: 0.0, flow: 1.0, reservoir_grid: side, ..Default::default() };
            apply(&mut work, &fp, fmt, Some(&sample), &m, &mut state, false);
            assert!(work.px(64, 48)[0] > 0.9 && work.px(64, 48)[2] < 0.01);
            assert!(work.px(64, 80)[2] > 0.9 && work.px(64, 80)[0] < 0.01);
            let grid = state.local.as_ref().unwrap();
            assert!(grid.valid());
            let min = grid.cells.iter().map(|c| c.amount).fold(1.0, f32::min);
            let max = grid.cells.iter().map(|c| c.amount).fold(0.0, f32::max);
            assert!(min < 0.96 && max > 0.999, "local contact must drain unevenly: {min} {max}");
            let clone: MixerState = serde_json::from_value(serde_json::to_value(&state).unwrap()).unwrap();
            assert_eq!(clone, state);
            state.clean();
            assert!(state.local.as_ref().unwrap().cells.iter().all(|c| c.pickup.is_none()));
            state.load([1.0, 1.0, 0.0, 1.0]);
            assert!(state.local.is_none());
        }
    }
    #[test]
    fn cell_mapping_tracks_pose_and_native_formats_remain_finite() {
        let load: Vec<_> = (0..4096).map(|i| [i as f32, 0.0, 0.0, 1.0]).collect();
        let b = BrushSettings { size: 64.0, roundness: 0.5, angle: 37.0, flip_x: true, pressure_size: false, ..Default::default() };
        let dab = crate::dabs(&Stroke { brush: b.clone(), points: vec![StrokePoint::new(32.0, 32.0, 1.0)] })[0];
        let map = Mapping::new(&dab, 64);
        for y in 0..64 {
            for x in 0..64 {
                if let Some(i) = map.at(x, y) {
                    assert_eq!(crate::mixer::load_at(&load, &dab, x as f32 + 0.5, y as f32 + 0.5)[0], i as f32);
                }
            }
        }
        for mode in
            [photocraft_color::ColorMode::Rgb, photocraft_color::ColorMode::Grayscale, photocraft_color::ColorMode::Cmyk, photocraft_color::ColorMode::Lab]
        {
            for depth in [photocraft_color::SampleType::U8, photocraft_color::SampleType::U16, photocraft_color::SampleType::F32] {
                let fmt = PixelFormat::new(mode, depth, true);
                let mut s = Surface::new(fmt);
                let mut state = MixerState::default();
                state.load([0.8, 0.2, 0.1, 1.0]);
                let m = MixerSettings { wet: 0.0, reservoir_grid: 32, ..Default::default() };
                crate::mixer::apply_mixer_stroke(
                    &mut s,
                    None,
                    &Stroke { brush: b.clone(), points: vec![StrokePoint::new(32.0, 32.0, 1.0), StrokePoint::new(48.0, 48.0, 0.7)] },
                    &m,
                    &mut state,
                    None,
                    false,
                );
                assert!(s.read_region(Rect::new(0, 0, 80, 80)).iter().all(|v| v.is_finite()));
                assert!(s.rgba(32, 32)[3] > 0.0);
            }
        }
    }
    #[test]
    fn changing_grid_preserves_load_and_invalid_cells_are_bounded() {
        let b = BrushSettings { size: 96.0, pressure_size: false, ..Default::default() };
        let stroke = Stroke { brush: b.clone(), points: vec![StrokePoint::new(64.0, 64.0, 1.0)] };
        let dab = crate::dabs(&stroke)[0];
        let fp = BrushContext::new(&b).footprint(&dab, 0);
        let mut state = MixerState::default();
        state.load([0.8, 0.2, 0.1, 1.0]);
        let mut work = Region::read(&Surface::new(PixelFormat::RGBA32F), Rect::new(0, 0, 128, 128));
        for side in [16, 32, 64, 16] {
            let m = MixerSettings { reservoir_grid: side, wet: 0.0, flow: 0.0, ..Default::default() };
            apply(&mut work, &fp, PixelFormat::RGBA32F, None, &m, &mut state, false);
            let local = state.local.as_ref().unwrap();
            assert_eq!(local.side, side);
            assert!(local.cells.iter().all(|c| c.loaded == [0.8, 0.2, 0.1, 1.0] && c.amount == 1.0));
        }
        crate::mixer::apply_mixer_dab_region(&mut work, &fp, PixelFormat::RGBA32F, None, &MixerSettings::default(), &mut state, false);
        assert!(state.local.is_none(), "uniform mode must not retain an obsolete local grid");
        assert!(!Reservoir { side: u32::MAX, cells: vec![] }.valid());
        assert!(!Reservoir { side: 64, cells: vec![] }.valid());
        assert!(!Reservoir { side: 16, cells: vec![Cell { loaded: [f32::NAN; 4], pickup: None, amount: 1.0 }; 256] }.valid());
    }
}
