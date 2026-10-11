//! Palette knife: a bounded blade contact map using the existing incremental pigment mixer.
use crate::{EngineError, Result, Session, commands::CommandSpec};
use photocraft_paint::{BrushSettings, Control, Dynamic, GrayTile, ShapeDynamics, TipShape};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub version: u32,
    pub width: f32,
    pub angle: f32,
    pub contact: f32,
    pub softness: f32,
    pub breakup: f32,
    pub follow_direction: bool,
    pub pressure_contact: bool,
    pub wet: f32,
    pub mix: f32,
    pub flow: f32,
    pub load: f32,
    pub sample_all_layers: bool,
    pub clean_after_stroke: bool,
    pub reload_after_stroke: bool,
    pub seed: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            width: 64.0,
            angle: 90.0,
            contact: 0.2,
            softness: 0.05,
            breakup: 0.2,
            follow_direction: true,
            pressure_contact: true,
            wet: 0.8,
            mix: 0.8,
            flow: 0.8,
            load: 0.7,
            sample_all_layers: true,
            clean_after_stroke: true,
            reload_after_stroke: true,
            seed: 42,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.version != 1
            || !self.width.is_finite()
            || !(1.0..=1024.0).contains(&self.width)
            || !self.angle.is_finite()
            || self.angle.abs() > 3600.0
            || !self.contact.is_finite()
            || !(0.05..=1.0).contains(&self.contact)
        {
            return Err("knife version, width, angle or contact is outside the supported range".into());
        }
        if [self.softness, self.breakup, self.wet, self.mix, self.flow, self.load].iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
            return Err("knife paint and edge fractions must be finite and between 0 and 1".into());
        }
        Ok(())
    }
    pub fn brush(&self) -> std::result::Result<BrushSettings, String> {
        self.validate()?;
        let mask = GrayTile::from_fn(64, 64, |x, y| {
            let u = ((x as f32 + 0.5) / 32.0 - 1.0).abs();
            let v = ((y as f32 + 0.5) / 32.0 - 1.0).abs();
            let edge = 1.0 - u.max(v);
            let coverage = if self.softness > 0.0 { (edge / (self.softness * 0.5)).clamp(0.0, 1.0) } else { 1.0 };
            coverage * (1.0 - self.breakup * (1.0 - edge) * photocraft_paint::rng::hash2(x as i32, y as i32, self.seed))
        });
        let mut brush = BrushSettings {
            size: self.width,
            angle: self.angle,
            roundness: self.contact,
            tip: TipShape::Sampled(mask),
            pressure_size: false,
            pressure_opacity: self.pressure_contact,
            spacing: (self.contact * 0.15).clamp(0.01, 0.1),
            seed: self.seed,
            shape_dynamics: ShapeDynamics {
                enabled: true,
                angle: Dynamic::controlled(if self.follow_direction { Control::Direction } else { Control::Off }),
                roundness: Dynamic { control: if self.pressure_contact { Control::PenPressure } else { Control::Off }, minimum: 0.15, ..Default::default() },
                ..Default::default()
            },
            mixer: photocraft_paint::mixer::MixerSettings {
                wet: self.wet,
                mix: self.mix,
                flow: self.flow,
                load: self.load,
                sample_all_layers: self.sample_all_layers,
                clean_after_stroke: self.clean_after_stroke,
                reload_after_stroke: self.reload_after_stroke,
            },
            ..Default::default()
        };
        brush.smoothing.amount = 0.0;
        Ok(brush)
    }
}

fn bad(message: impl Into<String>) -> EngineError {
    EngineError::BadParams { cmd: "paint.paletteKnife".into(), msg: message.into() }
}
pub fn prepare_params(p: &Value) -> Result<Value> {
    if !p.is_object() {
        return Err(bad("expected a knife stroke object"));
    }
    if p.get("layer").is_some_and(|v| v.as_u64().is_none()) {
        return Err(bad("layer must be an unsigned integer"));
    }
    if let Some(target) = p.get("target") {
        let valid = target.as_str().is_some_and(|v| matches!(v, "pixels" | "mask" | "quickMask"))
            || target.as_object().is_some_and(|o| o.len() == 1 && o.get("channel").and_then(Value::as_u64).is_some_and(|v| usize::try_from(v).is_ok()));
        if !valid {
            return Err(bad("invalid paint target"));
        }
    }
    if p.get("brush").is_some() {
        if p.get("knife").is_some() {
            return Err(bad("use either knife settings or an explicit replay brush"));
        }
        return Ok(p.clone());
    }
    let settings: Settings = Settings::deserialize(p.get("knife").unwrap_or(&json!({}))).map_err(|e| bad(e.to_string()))?;
    let brush = settings.brush().map_err(bad)?;
    let mut params = p.clone();
    if let Some(object) = params.as_object_mut() {
        object.remove("knife");
        object.insert("brush".into(), serde_json::to_value(brush).map_err(|e| bad(e.to_string()))?);
    }
    Ok(params)
}
fn run(s: &mut Session, p: &Value) -> Result<Value> {
    crate::brush_cmds::mixer_live::run_with_label(s, &prepare_params(p)?, "Palette Knife")
}
pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "paint.paletteKnife",
        label: "Palette Knife",
        menu: &[],
        shortcut: None,
        params: r#"{"points":[[x,y,pressure?],…],"knife":{"version":1,"width":1..1024,"angle":degrees,"contact":0.05..1,"softness":0..1,"breakup":0..1,"followDirection":bool,"pressureContact":bool,"wet":0..1,"mix":0..1,"flow":0..1,"load":0..1,"sampleAllLayers":bool,"cleanAfterStroke":bool,"reloadAfterStroke":bool,"seed":u64}?,"brush":BrushSettings? (explicit replay alternative to knife),"target":"pixels"|"mask"|"quickMask"|{"channel":i},"seed":u64?}"#,
        enabled: crate::commands::has_paintable,
        run,
        journal: true,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn knife_contact_rotation_softness_and_pressure_use_real_dabs() {
        use photocraft_paint::{Stroke, StrokePoint};
        let hard = Settings { softness: 0.0, breakup: 0.0, follow_direction: false, ..Default::default() };
        let b = hard.brush().unwrap();
        assert!(b.tip.bitmap().unwrap().data.iter().all(|v| *v == 65535));
        let soft = Settings { softness: 1.0, ..hard.clone() }.brush().unwrap();
        assert!(soft.tip.bitmap().unwrap().data.iter().any(|v| *v < 20000));
        let dab = |pressure| photocraft_paint::dabs(&Stroke { brush: b.clone(), points: vec![StrokePoint::new(40.0, 40.0, pressure)] }).remove(0);
        let (light, heavy) = (dab(0.2), dab(1.0));
        assert!(light.roundness < heavy.roundness);
        assert!((heavy.angle - std::f32::consts::FRAC_PI_2).abs() < 0.001);
        assert!(Settings { width: f32::INFINITY, ..hard }.brush().is_err());
    }
    #[test]
    fn knife_live_commit_replay_undo_and_source_preservation() {
        use crate::brush_cmds::mixer_live::LiveMixer;
        let mut s = Session::new();
        s.execute("file.new", json!({"width":160,"height":120,"background":"#23a060"})).unwrap();
        s.execute("layer.new.layer", json!({"name":"Knife"})).unwrap();
        let before = s.active().unwrap().doc.clone();
        let p = prepare_params(&json!({"points":[[20,50,1],[80,60,0.7],[135,70,1]],"knife":{"width":32,"wet":1,"mix":1}})).unwrap();
        let live = LiveMixer::begin(&s, &p).unwrap();
        let shown = live.doc.clone();
        let commit = live.prepare_commit(&mut s).unwrap();
        s.execute("paint.paletteKnife", commit.clone()).unwrap();
        assert_eq!(*s.active().unwrap().doc, *shown);
        assert_eq!(before.layers[0], shown.layers[0]);
        assert_ne!(before.layers[1], shown.layers[1]);
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(*s.active().unwrap().doc, *before);
        s.execute("paint.paletteKnife", commit).unwrap();
        assert_eq!(*s.active().unwrap().doc, *shown);
    }

    #[test]
    fn knife_hostile_settings_fail_atomically_and_clean_load_stays_empty() {
        let mut s = Session::new();
        s.execute("file.new", json!({"width":64,"height":64})).unwrap();
        s.execute("layer.new.layer", json!({})).unwrap();
        let before = s.active().unwrap().doc.clone();
        let history = s.active().unwrap().history.past_len();
        for p in [
            json!({"points":[[20,20]],"knife":{"width":1e300}}),
            json!({"points":[[20,20]],"knife":{"contact":0}}),
            json!({"points":[[20,20]],"knife":{"version":2}}),
            json!({"points":[[20,20]],"knife":{"unknown":1}}),
            json!({"points":[[20,20]],"knife":{},"brush":{}}),
            json!({"points":[[20,20]],"target":"misspelled"}),
            json!({"points":[[20,20]],"layer":-1}),
            json!(null),
        ] {
            assert!(s.execute("paint.paletteKnife", p).is_err());
            assert_eq!(*s.active().unwrap().doc, *before);
            assert_eq!(s.active().unwrap().history.past_len(), history);
        }
        s.execute("brush.mixer.clean", json!({})).unwrap();
        s.execute("paint.paletteKnife", json!({"points":[[20,20],[40,20]],"knife":{"width":16,"reloadAfterStroke":false,"wet":0,"mix":0}})).unwrap();
        for (actual, old) in s.active().unwrap().doc.layers.iter().zip(&before.layers) {
            assert_eq!(actual.surface().unwrap().read_region(before.bounds()), old.surface().unwrap().read_region(before.bounds()));
        }
    }
}
