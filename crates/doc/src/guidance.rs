//! Explicit artist intent, independent from rendered artwork and generated results.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};
pub const MAX_LAYERS: usize = 16;
pub const MAX_STROKES: usize = 1024;
pub const MAX_POINTS: usize = 65536;
pub const MAX_STROKE_POINTS: usize = 4096;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Semantic {
    #[default]
    Highlight,
    Shadow,
    FoldDirection,
    MaterialBoundary,
    EdgePreservation,
    TextureDirection,
    DepthCue,
    Reflection,
    AreaModify,
    AreaProtect,
}
impl Semantic {
    pub const ALL: [Self; 10] = [
        Self::Highlight,
        Self::Shadow,
        Self::FoldDirection,
        Self::MaterialBoundary,
        Self::EdgePreservation,
        Self::TextureDirection,
        Self::DepthCue,
        Self::Reflection,
        Self::AreaModify,
        Self::AreaProtect,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Highlight => "Highlight",
            Self::Shadow => "Shadow",
            Self::FoldDirection => "Fold direction",
            Self::MaterialBoundary => "Material boundary",
            Self::EdgePreservation => "Edge preservation",
            Self::TextureDirection => "Texture direction",
            Self::DepthCue => "Depth cue",
            Self::Reflection => "Reflection",
            Self::AreaModify => "Area to modify",
            Self::AreaProtect => "Area to protect",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stroke {
    pub id: u32,
    pub semantic: Semantic,
    pub points: Arc<Vec<Point>>,
    pub width: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub pressure_size: bool,
    pub arrow: bool,
    pub annotation: String,
}
impl Stroke {
    pub fn validate(&self) -> Result<(), String> {
        if self.id == 0
            || self.id > 10_000_000
            || self.points.is_empty()
            || self.points.len() > MAX_STROKE_POINTS
            || !self.width.is_finite()
            || !(1.0..=512.0).contains(&self.width)
            || !unit(self.hardness)
            || !unit(self.opacity)
            || self.annotation.len() > 1024
            || self.points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite() || p.x.abs() > 1_000_000.0 || p.y.abs() > 1_000_000.0 || !unit(p.pressure))
        {
            return Err("invalid or oversized guidance stroke".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Layer {
    pub id: u32,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
    pub strokes: Arc<Vec<Stroke>>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Guidance {
    pub layers: Arc<Vec<Layer>>,
}
fn unit(v: f32) -> bool {
    v.is_finite() && (0.0..=1.0).contains(&v)
}
impl Guidance {
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
    pub fn validate(&self) -> Result<(), String> {
        let mut ids = BTreeSet::new();
        let mut strokes = 0;
        let mut points = 0;
        if self.layers.len() > MAX_LAYERS {
            return Err("at most 16 guidance layers".into());
        }
        for layer in self.layers.iter() {
            if layer.id == 0 || layer.id > 10_000_000 || !ids.insert(layer.id) || layer.name.is_empty() || layer.name.len() > 128 || !unit(layer.opacity) {
                return Err("invalid guidance layer".into());
            }
            if layer.strokes.len() > MAX_STROKES {
                return Err("too many guidance strokes".into());
            }
            let mut stroke_ids = BTreeSet::new();
            for stroke in layer.strokes.iter() {
                strokes += 1;
                points += stroke.points.len();
                if strokes > MAX_STROKES || points > MAX_POINTS {
                    return Err("guidance exceeds 1024 strokes or 65536 points".into());
                }
                stroke.validate()?;
                if !stroke_ids.insert(stroke.id) {
                    return Err("duplicate guidance stroke id".into());
                }
            }
        }
        if strokes > MAX_STROKES || points > MAX_POINTS {
            return Err("guidance exceeds 1024 strokes or 65536 points".into());
        }
        Ok(())
    }
}
