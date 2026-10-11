//! Bounded procedural paint droplets, independent of UI animation and network services.
use crate::{
    GrayTile,
    retouch::{Region, over_native},
    rng,
};
use photocraft_geom::Rect;
use photocraft_raster::{Surface, from_rgba};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub count: u32,
    pub min_size: f32,
    pub max_size: f32,
    pub size_variation: f32,
    pub spread: f32,
    pub angle: f32,
    pub directional_bias: f32,
    pub distance: f32,
    pub density: f32,
    pub clustering: f32,
    pub opacity_variation: f32,
    pub color_variation: f32,
    pub paint_load: f32,
    pub seed: u64,
    pub edge_breakup: f32,
    pub secondary: f32,
    pub distribution: Option<GrayTile>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            count: 160,
            min_size: 1.0,
            max_size: 10.0,
            size_variation: 1.0,
            spread: 100.0,
            angle: 0.0,
            directional_bias: 0.3,
            distance: 128.0,
            density: 1.0,
            clustering: 0.2,
            opacity_variation: 0.3,
            color_variation: 0.1,
            paint_load: 1.0,
            seed: 42,
            edge_breakup: 0.15,
            secondary: 0.15,
            distribution: None,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported splatter settings version".into());
        }
        if self.count > 2048 {
            return Err("splatter count must be at most 2048".into());
        }
        let unit = [
            self.size_variation,
            self.directional_bias,
            self.density,
            self.clustering,
            self.opacity_variation,
            self.color_variation,
            self.paint_load,
            self.edge_breakup,
            self.secondary,
        ];
        if unit.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
            return Err("splatter fractions must be finite and between 0 and 1".into());
        }
        if !self.min_size.is_finite() || !self.max_size.is_finite() || self.min_size < 0.5 || self.min_size > self.max_size || self.max_size > 128.0 {
            return Err("droplet diameters must be ordered and between 0.5 and 128 px".into());
        }
        if !self.distance.is_finite()
            || !(0.0..=1024.0).contains(&self.distance)
            || !self.spread.is_finite()
            || !(0.0..=360.0).contains(&self.spread)
            || !self.angle.is_finite()
            || self.angle.abs() > 3600.0
        {
            return Err("splatter distance, spread or angle is outside its supported range".into());
        }
        // Bound worst-case raster work before allocating or touching the target.
        let side = (self.max_size + 4.0).ceil() as u64;
        if u64::from(self.count).saturating_mul(2).saturating_mul(side.saturating_mul(side)) > 8_388_608 {
            return Err("splatter exceeds the pixel-work budget; reduce droplet count or diameter".into());
        }
        if let Some(mask) = &self.distribution {
            if mask.width > 512 || mask.height > 512 || !mask.is_valid() {
                return Err("distribution mask must be a valid bitmap no larger than 512 x 512".into());
            }
            if mask.data.iter().all(|v| *v == 0) {
                return Err("distribution mask contains no paint".into());
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Droplet {
    pub x: f64,
    pub y: f64,
    pub diameter: f32,
    pub opacity: f32,
    pub color_scale: [f32; 3],
    pub index: u64,
}

/// Generate once per flick. Pressure controls contact/intensity. A gesture may choose
/// `angle` and `distance` before calling this function; an identical request replays exactly.
pub fn droplets(settings: &Settings, center: [f64; 2], pressure: f32) -> Result<Vec<Droplet>, String> {
    settings.validate()?;
    if center.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.0) || !pressure.is_finite() || !(0.0..=1.0).contains(&pressure) {
        return Err("invalid splatter position or pressure".into());
    }
    let n = (settings.count as f32 * settings.density * pressure).round() as u32;
    let mut result = Vec::with_capacity(n as usize * 2);
    let mut total = 0.0_f64;
    let cdf: Vec<_> = settings
        .distribution
        .as_ref()
        .map(|m| {
            m.data
                .iter()
                .map(|v| {
                    total += f64::from(*v);
                    total
                })
                .collect()
        })
        .unwrap_or_default();
    let (sin, cos) = settings.angle.to_radians().sin_cos();
    for i in 0..n {
        let index = u64::from(i);
        let random = |stream| rng::rand01(settings.seed, index, stream);
        let grouped = if random(20) < settings.clustering { index / 8 } else { index };
        let (u, v) = if let Some(mask) = &settings.distribution {
            let chosen = f64::from(rng::rand01(settings.seed, grouped, 21)) * total;
            let cell = cdf.partition_point(|v| *v <= chosen).min(mask.data.len().saturating_sub(1));
            let col = cell % mask.width as usize;
            let row = cell / mask.width as usize;
            ((col as f32 + random(22)) / mask.width as f32 * 2.0 - 1.0, (row as f32 + random(23)) / mask.height as f32 * 2.0 - 1.0)
        } else {
            let angle = rng::rand_signed(settings.seed, grouped, 24) * settings.spread.to_radians() * 0.5 * (1.0 - settings.directional_bias * random(25));
            let radius = rng::rand01(settings.seed, grouped, 26).sqrt();
            (angle.cos() * radius, angle.sin() * radius)
        };
        let x = center[0] + f64::from((u * cos - v * sin) * settings.distance);
        let y = center[1] + f64::from((u * sin + v * cos) * settings.distance);
        let diameter = (settings.max_size - (settings.max_size - settings.min_size) * random(27) * settings.size_variation) * (0.25 + 0.75 * pressure.sqrt());
        let opacity = settings.paint_load * (1.0 - settings.opacity_variation * random(28));
        let color_scale = std::array::from_fn(|c| 1.0 + rng::rand_signed(settings.seed, index, 29 + c as u64) * settings.color_variation);
        result.push(Droplet { x, y, diameter, opacity, color_scale, index: index * 2 });
        if random(32) < settings.secondary {
            let angle = random(33) * std::f32::consts::TAU;
            let radius = diameter * (0.75 + random(34));
            result.push(Droplet {
                x: x + f64::from(angle.cos() * radius),
                y: y + f64::from(angle.sin() * radius),
                diameter: (diameter * 0.3).max(0.5),
                opacity: opacity * 0.7,
                color_scale,
                index: index * 2 + 1,
            });
        }
    }
    Ok(result)
}

/// Normal-mode pigment deposition in the target's native model and depth. The caller owns
/// the document transaction. Only bounded droplet regions are read and written.
pub fn apply(
    target: &mut Surface,
    settings: &Settings,
    center: [f64; 2],
    pressure: f32,
    color: [f32; 4],
    selection: Option<&Surface>,
    lock: bool,
) -> Result<Rect, String> {
    if color.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.0) || !(0.0..=1.0).contains(&color[3]) {
        return Err("invalid splatter color".into());
    }
    let drops = droplets(settings, center, pressure)?;
    if color[3] == 0.0 {
        return Ok(Rect::EMPTY);
    }
    let fmt = target.format();
    let mut damage = Rect::EMPTY;
    for drop in drops {
        if drop.opacity <= 0.0 {
            continue;
        }
        let radius = drop.diameter / 2.0;
        let r = Rect::new(
            (drop.x - f64::from(radius) - 1.0).floor() as i32,
            (drop.y - f64::from(radius) - 1.0).floor() as i32,
            (drop.x + f64::from(radius) + 1.0).ceil() as i32,
            (drop.y + f64::from(radius) + 1.0).ceil() as i32,
        );
        let mut work = Region::read(target, r);
        let mask = selection.map(|s| Region::read(s, r));
        let mut rgba = color;
        for (c, factor) in rgba.iter_mut().take(3).zip(drop.color_scale) {
            *c *= factor;
        }
        let native = from_rgba(&fmt, rgba);
        let mut changed = false;
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                let d = ((x as f64 + 0.5 - drop.x).powi(2) + (y as f64 + 0.5 - drop.y).powi(2)).sqrt() as f32;
                let edge = (d / radius.max(0.25)).clamp(0.0, 1.0);
                let breakup = 1.0 - settings.edge_breakup * edge * rng::hash2(x, y, settings.seed ^ drop.index);
                let selected = mask.as_ref().and_then(|m| m.px(x, y).first()).copied().unwrap_or(1.0).clamp(0.0, 1.0);
                let alpha = if fmt.alpha { 1.0 } else { color[3] };
                if lock && fmt.alpha && work.px(x, y).get(fmt.mode.color_channels()).copied().unwrap_or(0.0) <= 0.0 {
                    continue;
                }
                let k = crate::dab_coverage(d, radius, 0.85) * drop.opacity * breakup * selected * alpha;
                if k > 0.0 {
                    over_native(&fmt, work.px_mut(x, y), &native, k, lock);
                    changed = true;
                }
            }
        }
        if changed {
            work.write(target);
            damage = damage.union(&r);
        }
    }
    Ok(damage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use photocraft_color::{ColorMode, PixelFormat, SampleType};
    #[test]
    fn seeded_direction_pressure_and_custom_distribution_are_deterministic() {
        let s = Settings { spread: 0.0, directional_bias: 1.0, secondary: 0.0, ..Default::default() };
        let a = droplets(&s, [0.0, 0.0], 1.0).unwrap();
        assert_eq!(a, droplets(&s, [0.0, 0.0], 1.0).unwrap());
        assert!(a.iter().all(|p| p.x >= 0.0 && p.y == 0.0));
        let b = droplets(&Settings { angle: 90.0, ..s.clone() }, [0.0, 0.0], 1.0).unwrap();
        assert!(b.iter().all(|p| p.x.abs() < 0.001 && p.y >= 0.0));
        assert!(droplets(&s, [0.0, 0.0], 0.2).unwrap().len() < a.len());
        assert_ne!(a, droplets(&Settings { seed: 8, ..s.clone() }, [0.0, 0.0], 1.0).unwrap());
        let s = Settings { distribution: Some(GrayTile::from_fn(32, 32, |x, y| if x < 8 && y < 8 { 1.0 } else { 0.0 })), ..s };
        assert!(droplets(&s, [0.0, 0.0], 1.0).unwrap().iter().all(|p| p.x < 0.0 && p.y < 0.0));
    }
    #[test]
    fn deposition_respects_selection_depth_lock_and_bounds() {
        let s = Settings { count: 80, distance: 24.0, spread: 360.0, ..Default::default() };
        let mut selection = Surface::new(PixelFormat::GRAY8);
        selection.fill_rect(Rect::new(0, 0, 32, 64), &[1.0]);
        for mode in [ColorMode::Rgb, ColorMode::Grayscale, ColorMode::Cmyk, ColorMode::Lab] {
            for depth in [SampleType::U8, SampleType::U16, SampleType::F32] {
                let mut target = Surface::new(PixelFormat::new(mode, depth, true));
                let before = target.clone();
                let damage = apply(&mut target, &s, [32.0, 32.0], 1.0, [0.9, 0.2, 0.1, 1.0], Some(&selection), false).unwrap();
                assert!(!damage.is_empty());
                assert_ne!(target, before);
                assert!(target.read_region(Rect::new(32, 0, 64, 64)).iter().all(|v| *v == 0.0));
                let mut replay = before.clone();
                apply(&mut replay, &s, [32.0, 32.0], 1.0, [0.9, 0.2, 0.1, 1.0], Some(&selection), false).unwrap();
                assert_eq!(target, replay);
                let mut locked = before.clone();
                apply(&mut locked, &s, [32.0, 32.0], 1.0, [0.9, 0.2, 0.1, 1.0], None, true).unwrap();
                assert_eq!(locked.read_region(Rect::new(0, 0, 64, 64)), before.read_region(Rect::new(0, 0, 64, 64)));
            }
        }
        assert!(Settings { count: u32::MAX, ..s.clone() }.validate().is_err());
        assert!(Settings { distance: f32::NAN, ..s.clone() }.validate().is_err());
        assert!(Settings { count: 2048, max_size: 128.0, ..s.clone() }.validate().is_err());
        assert!(droplets(&s, [f64::INFINITY, 0.0], 1.0).is_err());
    }
}
