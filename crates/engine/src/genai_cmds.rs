//! Regional studio edits using the upstream provider abstraction and existing jobs/history.
use crate::{EngineError, Result, Session, commands::CommandSpec};
use photocraft_doc::{
    LayerId,
    ai_result::{AiResult, EditRecipe, GenerationRecord, ResultOrigin, ResultStatus},
};
use photocraft_genai::ImageProvider;
use photocraft_geom::Rect;
use serde::Deserialize;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[path = "studio_prepare.rs"]
mod prepare;
pub use prepare::ReviewedEdit;
pub type SharedProvider = Arc<dyn ImageProvider>;
pub const FILL: &str = "edit.generativeFill";
#[derive(Default)]
pub(crate) struct Runtime {
    provider: Option<SharedProvider>,
    approved: Option<ReviewedEdit>,
    busy: Arc<AtomicBool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    recipe: EditRecipe,
    #[serde(default)]
    replace: Option<LayerId>,
}
fn bad(e: impl std::fmt::Display) -> EngineError {
    EngineError::BadParams { cmd: FILL.into(), msg: e.to_string() }
}
impl Session {
    /// Trusted shell configuration. No socket or credential lookup occurs here.
    pub fn set_image_provider(&mut self, provider: Option<SharedProvider>) {
        self.genai.provider = provider;
        self.genai.approved = None;
    }
    pub fn image_provider(&self) -> Option<&SharedProvider> {
        self.genai.provider.as_ref()
    }
    pub fn review_image_edit(&self, p: &Value) -> Result<ReviewedEdit> {
        let p: Params = serde_json::from_value(p.clone()).map_err(bad)?;
        let provider = self.image_provider().cloned().ok_or_else(|| bad("configure an image provider first"))?;
        let doc = self.active().ok_or(EngineError::NoDocument)?.doc.clone();
        prepare::review(doc, provider, p.recipe, p.replace)
    }
    /// Only a direct, non-automation native confirmation may call this method.
    /// Deliberately no JSON command grants remote permission; replay cannot authorize a call.
    pub fn approve_image_edit(&mut self, review: ReviewedEdit) -> Result<()> {
        let same_doc = self.active().is_some_and(|s| Arc::ptr_eq(&s.doc, &review.document));
        let same_provider = self.image_provider().is_some_and(|p| Arc::ptr_eq(p, &review.provider));
        self.genai.approved = None;
        if !same_doc || !same_provider {
            return Err(bad("the reviewed document or provider changed; review again"));
        }
        self.genai.approved = Some(review);
        Ok(())
    }
    pub fn discard_image_edit_approval(&mut self) {
        self.genai.approved = None;
    }
}
struct Busy(Arc<AtomicBool>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
fn enabled(s: &Session) -> std::result::Result<(), String> {
    if s.active().is_none() {
        return Err("no document open".into());
    }
    if s.image_provider().is_none() {
        return Err("configure an image provider first".into());
    }
    if s.genai.busy.load(Ordering::Acquire) {
        return Err("the previous provider request has not finished; a cancelled HTTP call may remain in flight until its timeout".into());
    }
    Ok(())
}
fn review_command(s: &mut Session, p: &Value) -> Result<Value> {
    Ok(s.review_image_edit(p)?.disclosure())
}
fn fill(s: &mut Session, p: &Value) -> Result<Value> {
    enabled(s).map_err(bad)?;
    let review = s.review_image_edit(p)?;
    let approved = s.genai.approved.take(); // one use, including invalid attempts
    if !review.provider.local_only() {
        let approved = approved.ok_or_else(|| bad("remote image editing requires a fresh native confirmation of this exact request"))?;
        if !Arc::ptr_eq(&approved.document, &review.document)
            || !Arc::ptr_eq(&approved.provider, &review.provider)
            || approved.recipe != review.recipe
            || approved.replace != review.replace
            || approved.context != review.context
            || approved.input_size != review.input_size
        {
            return Err(bad("the request changed after review; review and confirm again"));
        }
    }
    if s.genai.busy.swap(true, Ordering::AcqRel) {
        return Err(bad("a provider request is already in flight"));
    }
    let busy = Busy(s.genai.busy.clone());
    let apply_provider = review.provider.clone();
    let replace = review.replace;
    crate::jobs::run(
        s,
        "AI Image Edit",
        true,
        move |ctx| {
            let _busy = busy; // retain across cancellation until the blocking transport returns
            ctx.progress(0.02, "Preparing regional image and guidance");
            let prepared = prepare::prepare(&review, ctx)?;
            ctx.check()?;
            ctx.progress(0.2, "Waiting for image provider");
            let output = review.provider.edit_guided(&prepared.request).map_err(bad)?;
            ctx.check()?;
            ctx.progress(0.9, "Validating and placing result");
            let result_digest = blake3::hash(&output.bytes).to_hex().to_string();
            let (decoded, full) = crate::ai_result_cmds::decode_provider_into_document(&output.bytes, &review.document)?;
            let context = review.context;
            let expected = f64::from(review.input_size[0]) / f64::from(review.input_size[1]);
            let received = f64::from(full.width()) / f64::from(full.height());
            if full.is_empty() || !received.is_finite() || (received / expected - 1.0).abs() > 0.02 {
                return Err(bad("provider result has an unexpected aspect ratio; it was not stretched into the artwork"));
            }
            let fitted = photocraft_algo::resample::resize_surface(
                &decoded,
                f64::from(context.width()) / f64::from(full.width()),
                f64::from(context.height()) / f64::from(full.height()),
                photocraft_algo::resample::Resample::Bicubic,
            );
            let roi = review.recipe.region;
            let local = Rect::from_xywh(roi.x0 - context.x0, roi.y0 - context.y0, roi.width(), roi.height());
            let mut pixels = photocraft_raster::Surface::new(fitted.format());
            pixels.write_region(roi, &fitted.read_region(local));
            let info = review.provider.info();
            let record = AiResult {
                version: 1,
                origin: if review.provider.local_only() { ResultOrigin::LocalProvider } else { ResultOrigin::RemoteProvider },
                status: ResultStatus::Proposed,
                provider: info.name.into(),
                model: Some(info.model),
                endpoint: Some(review.provider.endpoint()),
                created_unix_seconds: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()),
                input_digest: prepared.request_digest,
                result_digest,
                recipe: review.recipe,
                generation: Some(GenerationRecord {
                    context,
                    input_size: review.input_size,
                    guidance_digest: prepared.guidance_digest,
                    semantics: prepared.request.guidance.iter().map(|g| g.semantic.clone()).collect(),
                    annotations: prepared.request.guidance.iter().flat_map(|g| g.annotations.iter().map(|a| format!("{}: {}", g.semantic, a))).collect(),
                    decoded_size: [full.width(), full.height()],
                }),
            };
            record.validate().map_err(bad)?;
            ctx.check()?;
            Ok((pixels, prepared.mask, record))
        },
        move |s, (pixels, mask, record)| {
            if s.image_provider().is_none_or(|p| !Arc::ptr_eq(p, &apply_provider)) {
                return Err(bad("provider settings changed; result discarded"));
            }
            crate::ai_result_cmds::place(s, pixels, mask, record, replace)
        },
    )
}
pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "ai.edit.review",
            label: "Review AI image edit",
            menu: &[],
            shortcut: None,
            params: r#"{recipe,replace?}; discloses request scope without rendering, transmitting or granting permission"#,
            enabled,
            run: review_command,
            journal: false,
        },
        CommandSpec {
            id: FILL,
            label: "Generative Fill…",
            menu: &["Edit"],
            shortcut: None,
            params: r#"{recipe:{prompt,material?,lighting?,region,contextPixels?,outputSize},replace?}; bounded image editing through the configured provider. Remote calls require a fresh direct native confirmation. A recorded action cannot authorize remote calls."#,
            enabled,
            run: fill,
            journal: true,
        },
    ]
}

#[cfg(test)]
#[path = "genai_cmds_tests.rs"]
mod tests;
