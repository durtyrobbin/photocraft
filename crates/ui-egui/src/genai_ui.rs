//! Generative AI in the shell (#41): Generative AI Settings (provider, model and API key), the
//! Edit › Generative Fill prompt, and installing the configured provider on the session.
//!
//! Off by default: until the user configures a provider, no provider exists, the
//! Generative Fill menu item is hidden and nothing is ever contacted. The provider and model live
//! in preferences (shell-owned `dialogs` entry); a remote key lives in the credential service
//! ([`crate::GenAiServices`]) and in process memory. The key is typed into a buffer outside the
//! dialog's fields, so `ui.inspect` and the control channel never see it.

use serde_json::{Map, Value, json};

use crate::PhotocraftApp;
use crate::state::DialogKind;

/// Opens Generative AI Settings.
pub const SETTINGS: &str = "ui.generativeAiSettings";
const FILL: &str = photocraft_engine::genai_cmds::FILL;
const SETTINGS_MARK: &str = "__genaiSettings";
const PROMPT_MARK: &str = "__genaiPrompt";
/// Shell preference entry holding provider/model and bounded local endpoint options.
const PREF: &str = "ui.generativeAiSettings";
const KEY_PAGE: &str = "https://aistudio.google.com/apikey";

/// Shell state that must not be serialised or inspected.
#[derive(Default)]
pub struct Runtime {
    /// What is typed in the settings dialog's key field. Cleared when the dialog closes.
    key_input: String,
    review: Option<(Value, photocraft_engine::genai_cmds::ReviewedEdit)>,
    region_cache: Option<RegionCache>,
}
struct RegionCache {
    document: std::sync::Arc<photocraft_doc::Document>,
    result: Option<photocraft_doc::LayerId>,
    region: photocraft_geom::Rect,
}

/// Providers the settings offer: (id, name). Only providers the app can build appear.
fn providers() -> Vec<(String, &'static str)> {
    vec![
        (String::new(), tl!("None")),
        (photocraft_genai::local_webui::ID.into(), photocraft_genai::local_webui::NAME),
        (photocraft_genai::gemini::ID.into(), photocraft_genai::gemini::NAME),
    ]
}

fn saved(app: &PhotocraftApp) -> (String, String) {
    let v = app.session.prefs().dialogs.get(PREF);
    let get = |k: &str| v.and_then(|v| v.get(k)).and_then(Value::as_str).unwrap_or("").to_owned();
    (get("provider"), get("model"))
}

/// Build the saved provider and install it on the session (at launch and after Settings). Reads
/// the credential store only when a provider is configured; never touches the network.
pub fn restore(app: &mut PhotocraftApp) -> Result<(), String> {
    let (provider, model) = saved(app);
    if provider.is_empty() {
        app.session.set_image_provider(None);
        return Ok(());
    }
    let Some(services) = app.services.genai.as_ref() else {
        return Ok(());
    };
    let config = app.session.prefs().dialogs.get(PREF).cloned().unwrap_or(Value::Null);
    if provider == photocraft_genai::local_webui::ID {
        let built = (services.provider)(&provider, &model, None, &config)?;
        app.session.set_image_provider(Some(built));
        return Ok(());
    }
    let key = (services.load_key)(&provider)?.and_then(|k| photocraft_genai::ApiKey::new(&k));
    let Some(key) = key else {
        app.session.set_image_provider(None);
        return Err(crate::i18n::fmt(tl!("No API key is saved for {provider}."), &[("provider", &provider)]));
    };
    let built = (services.provider)(&provider, &model, Some(key), &config)?;
    app.session.set_image_provider(Some(built));
    Ok(())
}

/// Menu, palette and shortcut entry points. Clicking Generative Fill asks for a prompt; with
/// `recipe` already in the params it dispatches through the engine permission boundary.
pub fn menu(app: &mut PhotocraftApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    match id {
        SETTINGS => Some(Ok(json!({"dialog": open_settings(app)}))),
        FILL if params.get("recipe").is_none() => {
            if app.session.image_provider().is_none() {
                return Some(Ok(json!({"dialog": open_settings(app)})));
            }
            if let Some(why) = app.session.disabled_reason(FILL) {
                return Some(Err(why));
            }
            Some(Ok(json!({"dialog": open_prompt(app, false)})))
        }
        "ui.regenerateAiResult" => Some(Ok(json!({"dialog":open_prompt(app,true)}))),
        _ => None,
    }
}

/// Is the Edit › Generative Fill item shown? Only once a provider can run it (#41: no
/// "coming soon" items).
pub fn menu_visible(app: &PhotocraftApp) -> bool {
    app.session.image_provider().is_some_and(|p| p.capabilities().edit)
}

pub fn owns(fields: &Map<String, Value>) -> bool {
    fields.contains_key(SETTINGS_MARK) || fields.contains_key(PROMPT_MARK)
}

pub fn ok_label(fields: &Map<String, Value>) -> Option<&'static str> {
    if fields.contains_key(PROMPT_MARK) { Some(tl!("Generate")) } else { None }
}

pub fn open_settings(app: &mut PhotocraftApp) -> u64 {
    if let Some(d) = app.ui.dialogs.iter().find(|d| d.fields.contains_key(SETTINGS_MARK)) {
        return d.id;
    }
    app.genai.key_input.clear();
    let (mut provider, mut model) = saved(app);
    if provider.is_empty() {
        // Default the editor to local setup; nothing is enabled until native confirmation.
        provider = photocraft_genai::local_webui::ID.into();
    }
    if model.is_empty() {
        model = if provider == photocraft_genai::gemini::ID { photocraft_genai::gemini::DEFAULT_MODEL.into() } else { String::new() };
    }
    let has_key = provider == photocraft_genai::gemini::ID && app.services.genai.as_ref().and_then(|s| (s.load_key)(&provider).ok().flatten()).is_some();
    let mut f = app.session.prefs().dialogs.get(PREF).and_then(Value::as_object).cloned().unwrap_or_default();
    f.insert(SETTINGS_MARK.into(), json!(true));
    f.insert("__label".into(), json!("Generative AI Settings"));
    f.insert("provider".into(), json!(provider));
    f.insert("model".into(), json!(model));
    f.insert("hasKey".into(), json!(has_key));
    app.ui.open_dialog(DialogKind::Command, f)
}

fn open_prompt(app: &mut PhotocraftApp, replace: bool) -> u64 {
    if let Some(d) = app.ui.dialogs.iter().find(|d| d.fields.contains_key(PROMPT_MARK)) {
        return d.id;
    }
    app.genai.review = None;
    app.session.discard_image_edit_approval();
    let mut f = Map::new();
    f.insert(PROMPT_MARK.into(), json!(true));
    f.insert("__label".into(), json!("Generative Fill"));
    let selected = app.session.active().and_then(|s| s.active_layer.and_then(|id| s.doc.layer(id)).and_then(|l| l.ai_result.clone()));
    if let Some(record) = selected.filter(|_| replace) {
        f.insert("prompt".into(), json!(record.recipe.prompt));
        f.insert("material".into(), json!(record.recipe.material));
        f.insert("lighting".into(), json!(record.recipe.lighting));
        f.insert("context".into(), json!(record.recipe.context_pixels));
    } else {
        f.insert("prompt".into(), json!(app.ui.ai_results.prompt));
        f.insert("material".into(), json!(app.ui.ai_results.material));
        f.insert("lighting".into(), json!(app.ui.ai_results.lighting));
        f.insert("context".into(), json!(64));
    }
    f.insert("replace".into(), json!(replace));
    app.ui.open_dialog(DialogKind::Command, f)
}
fn request_params(app: &mut PhotocraftApp, f: &Map<String, Value>) -> Result<Value, String> {
    let st = app.session.active().ok_or("no document open")?;
    let replace = f.get("replace").and_then(Value::as_bool) == Some(true);
    let id = if replace {
        Some(st.active_layer.filter(|id| st.doc.layer(*id).is_some_and(|l| l.ai_result.is_some())).ok_or("select a result layer to regenerate")?)
    } else {
        None
    };
    let region = if let Some(cache) = app.genai.region_cache.as_ref().filter(|c| std::sync::Arc::ptr_eq(&c.document, &st.doc) && c.result == id) {
        cache.region
    } else {
        let region = if let Some(id) = id {
            st.doc.layer(id).and_then(|l| l.ai_result.as_ref()).ok_or("missing result")?.recipe.region
        } else {
            st.doc.selection.as_ref().map_or(st.doc.bounds(), |s| s.content_bounds().intersect(&st.doc.bounds()))
        };
        app.genai.region_cache = Some(RegionCache { document: st.doc.clone(), result: id, region });
        region
    };
    let text = |k| f.get(k).and_then(Value::as_str).unwrap_or("").to_owned();
    Ok(
        json!({"recipe":{"prompt":text("prompt"),"material":text("material"),"lighting":text("lighting"),"region":region,"contextPixels":f.get("context").and_then(Value::as_u64).unwrap_or(64),"outputSize":[region.width(),region.height()]},"replace":id}),
    )
}

pub fn body(app: &mut PhotocraftApp, ui: &mut egui::Ui, f: &mut Map<String, Value>) {
    if f.contains_key(PROMPT_MARK) {
        prompt_body(app, ui, f);
    } else {
        settings_body(app, ui, f);
    }
}

fn faint(ui: &mut egui::Ui, text: &str) {
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.add(egui::Label::new(egui::RichText::new(text).color(t.text_faint).size(12.0)).wrap());
}

fn prompt_body(app: &mut PhotocraftApp, ui: &mut egui::Ui, f: &mut Map<String, Value>) {
    for (key, label, limit) in [("prompt", tl!("Prompt"), 8000), ("material", tl!("Material"), 2000), ("lighting", tl!("Lighting"), 2000)] {
        let mut text = f.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
        ui.label(label);
        let r = ui.add(egui::TextEdit::multiline(&mut text).desired_rows(2).char_limit(limit).desired_width(440.0));
        crate::field_tab::register(ui.ctx(), r.id);
        if r.changed() {
            f.insert(key.into(), json!(text));
        }
    }
    let mut context = f.get("context").and_then(Value::as_u64).unwrap_or(64).min(1024) as u32;
    if ui.add(egui::Slider::new(&mut context, 0..=1024).text(tl!("Context pixels"))).changed() {
        f.insert("context".into(), json!(context));
    }
    let mut replace = f.get("replace").and_then(Value::as_bool) == Some(true);
    if ui.checkbox(&mut replace, tl!("Regenerate selected result")).changed() {
        f.insert("replace".into(), json!(replace));
    }
    faint(
        ui,
        tl!(
            "Selection and protection limit the result. Visible guidance and annotations are included. Regeneration uses current artwork and guidance with the selected result hidden."
        ),
    );
    let params = request_params(app, f);
    if app.genai.review.as_ref().is_some_and(|(p, _)| params.as_ref() != Ok(p)) {
        app.genai.review = None;
        app.session.discard_image_edit_approval();
    }
    if crate::widgets::secondary_button(ui, tl!("Review request"), 0.0).clicked() {
        match params.and_then(|p| app.session.review_image_edit(&p).map(|r| (p, r)).map_err(|e| e.to_string())) {
            Ok(review) => app.genai.review = Some(review),
            Err(e) => app.ui.status = e,
        }
    }
    if let Some((_, review)) = &app.genai.review {
        let d = review.disclosure();
        ui.separator();
        for (key, label) in [("provider", tl!("Provider")), ("model", tl!("Model")), ("endpoint", tl!("Endpoint"))] {
            ui.label(format!("{label}: {}", d[key].as_str().unwrap_or("")));
        }
        ui.label(format!("{}: {} × {}", tl!("Input size"), d["inputSize"][0], d["inputSize"][1]));
        let c = &d["context"];
        ui.label(format!("{}: ({}, {}) – ({}, {})", tl!("Context"), c["x0"], c["y0"], c["x1"], c["y1"]));
        if d["localOnly"] == true {
            faint(ui, tl!("Local endpoint only. No model availability or output quality has been verified."));
        } else {
            faint(
                ui,
                tl!(
                    "Generate sends this regional image, masks, prompt and guidance to the displayed external provider. Charges and its data terms apply. Confirm directly for this request only."
                ),
            );
        }
    }
}

fn settings_body(app: &mut PhotocraftApp, ui: &mut egui::Ui, f: &mut Map<String, Value>) {
    let t = crate::theme::Tokens::get(ui.ctx());
    if app.services.genai.is_none() {
        faint(ui, tl!("Generative AI needs the desktop app."));
        return;
    }
    let mut provider = f.get("provider").and_then(Value::as_str).unwrap_or("").to_owned();
    let options = providers();
    let opts: Vec<(String, &str)> = options.iter().map(|(id, name)| (id.clone(), *name)).collect();
    egui::Grid::new("genai-settings").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
        ui.label(tl!("Provider"));
        if crate::widgets::dropdown(ui, "genai-provider", &mut provider, &opts, 220.0) {
            f.insert("provider".into(), json!(provider));
            app.genai.key_input.clear();
            f.insert("model".into(), json!(if provider == photocraft_genai::gemini::ID { photocraft_genai::gemini::DEFAULT_MODEL } else { "" }));
            let has_key =
                provider == photocraft_genai::gemini::ID && app.services.genai.as_ref().and_then(|s| (s.load_key)(&provider).ok().flatten()).is_some();
            f.insert("hasKey".into(), json!(has_key));
        }
        ui.end_row();
        if provider == photocraft_genai::local_webui::ID {
            for (key, label, default) in [("endpoint", tl!("Endpoint"), "http://127.0.0.1:7860"), ("model", tl!("Model"), "")] {
                let mut text = f.get(key).and_then(Value::as_str).unwrap_or(default).to_owned();
                ui.label(label);
                if ui.add(egui::TextEdit::singleline(&mut text).char_limit(256)).changed() {
                    f.insert(key.into(), json!(text));
                }
                ui.end_row();
            }
            let mut strength = f.get("strength").and_then(Value::as_f64).unwrap_or(0.45) as f32;
            ui.label(tl!("Strength"));
            if ui.add(egui::Slider::new(&mut strength, 0.0..=1.0)).changed() {
                f.insert("strength".into(), json!(strength));
            }
            ui.end_row();
            let mut steps = f.get("steps").and_then(Value::as_u64).unwrap_or(20).min(100) as u32;
            ui.label(tl!("Steps"));
            if ui.add(egui::Slider::new(&mut steps, 1..=100)).changed() {
                f.insert("steps".into(), json!(steps));
            }
            ui.end_row();
        }
        if provider == photocraft_genai::gemini::ID {
            ui.label(tl!("Model"));
            let mut model = f.get("model").and_then(Value::as_str).unwrap_or(photocraft_genai::gemini::DEFAULT_MODEL).to_owned();
            let models: Vec<(String, &str)> = photocraft_genai::gemini::MODELS.iter().map(|(id, name)| ((*id).to_owned(), *name)).collect();
            if crate::widgets::dropdown(ui, "genai-model", &mut model, &models, 220.0) {
                f.insert("model".into(), json!(model));
            }
            ui.end_row();
            ui.label(tl!("API key"));
            let has_key = f.get("hasKey").and_then(Value::as_bool) == Some(true);
            ui.horizontal(|ui| {
                let hint = if has_key { tl!("Saved (paste to replace)") } else { tl!("Paste your Gemini API key") };
                let r = ui.add(egui::TextEdit::singleline(&mut app.genai.key_input).password(true).hint_text(hint).desired_width(220.0));
                crate::field_tab::register(ui.ctx(), r.id);
                if has_key && crate::widgets::secondary_button(ui, tl!("Remove Key"), 0.0).clicked() && !app.automation_input && app.session.authorize.is_none()
                {
                    let removed = app.services.genai.as_ref().map(|s| (s.delete_key)(&provider));
                    match removed {
                        Some(Ok(())) => {
                            f.insert("hasKey".into(), json!(false));
                            app.session.set_image_provider(None);
                        }
                        Some(Err(e)) => app.ui.status = e,
                        None => {}
                    }
                }
            });
            ui.end_row();
        }
    });
    if provider == photocraft_genai::gemini::ID {
        ui.add_space(4.0);
        if ui.link(tl!("Get a Gemini API key from Google AI Studio")).clicked() {
            crate::links::open(app, &ui.ctx().clone(), KEY_PAGE);
        }
        ui.add_space(8.0);
        crate::widgets::hairline(ui);
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tl!("What is sent, and to whom")).font(crate::theme::semibold(12.5)).color(t.text));
        ui.add_space(2.0);
        faint(
            ui,
            tl!(
                "Nothing is sent until you run Generative Fill. Each time you do, PhotoCraft sends the selected area and the image around it, a black-and-white mask of the selection, and your prompt to Google (generativelanguage.googleapis.com) with your API key. Google's Gemini API terms and privacy policy apply, and its usage is billed to your key. Results carry Google's SynthID watermark."
            ),
        );
        ui.add_space(4.0);
        let store = app.services.genai.as_ref().map_or("", |s| s.key_store_name);
        faint(ui, &crate::i18n::fmt(tl!("Your key is kept in {store}, never in PhotoCraft's settings files."), &[("store", tl!(store))]));
    }
}

/// OK: remember the choice, save a newly typed key, and install the provider (or remove it).
pub fn confirm(app: &mut PhotocraftApp, f: &Map<String, Value>) -> Result<Value, String> {
    if f.contains_key(PROMPT_MARK) {
        let params = request_params(app, f)?;
        let (reviewed, review) = app.genai.review.take().ok_or_else(|| tl!("Review the request before generating.").to_string())?;
        if reviewed != params {
            return Err(tl!("The request changed. Review it again.").into());
        }
        if app.session.image_provider().is_some_and(|p| !p.local_only()) && (app.automation_input || app.session.authorize.is_some()) {
            return Err(tl!("External generation requires direct native confirmation.").into());
        }
        app.session.approve_image_edit(review).map_err(|e| e.to_string())?;
        return app.run(FILL, params);
    }
    if app.automation_input || app.session.authorize.is_some() {
        return Err(tl!("Configure providers directly in the native app.").into());
    }
    let typed = std::mem::take(&mut app.genai.key_input);
    let provider = f.get("provider").and_then(Value::as_str).unwrap_or("").to_owned();
    let model = f.get("model").and_then(Value::as_str).unwrap_or("").to_owned();
    if !provider.is_empty() {
        let services = app.services.genai.as_ref().ok_or_else(|| tl!("Generative AI needs the desktop app.").to_string())?;
        let typed_key = photocraft_genai::ApiKey::new(&typed);
        let key = if provider == photocraft_genai::gemini::ID {
            match typed_key.clone() {
                Some(key) => Some(key),
                None => (services.load_key)(&provider)?.and_then(|k| photocraft_genai::ApiKey::new(&k)),
            }
        } else {
            None
        };
        // Validate every configuration before changing preferences or credential storage.
        (services.provider)(&provider, &model, key, &Value::Object(f.clone()))?;
        if provider == photocraft_genai::gemini::ID
            && let Some(key) = typed_key
        {
            (services.save_key)(&provider, key.expose())?;
        }
    }
    let mut config = Map::new();
    for key in ["provider", "model", "endpoint", "strength", "steps"] {
        if let Some(v) = f.get(key) {
            config.insert(key.into(), v.clone());
        }
    }
    app.session.prefs.edit(|p| p.dialogs.insert(PREF.into(), Value::Object(config)));
    restore(app)?;
    Ok(json!({"provider": provider, "model": model, "ready": app.session.image_provider().is_some()}))
}

/// Dialog closed without OK: drop whatever was typed in the key field.
pub fn cancelled(app: &mut PhotocraftApp) {
    app.genai.key_input.clear();
    app.genai.review = None;
    app.genai.region_cache = None;
    app.session.discard_image_edit_approval();
}

/// Preferences › Integrations: where Generative AI is set up.
pub fn prefs_rows(app: &PhotocraftApp, ui: &mut egui::Ui) -> bool {
    let t = crate::theme::Tokens::get(ui.ctx());
    if app.services.genai.is_none() {
        return false;
    }
    ui.label(egui::RichText::new(tl!("Generative AI")).font(crate::theme::semibold(12.5)).color(t.text));
    let status = match app.session.image_provider().map(|p| p.info()) {
        Some(info) => crate::i18n::fmt(tl!("Generative Fill uses {name} ({model})."), &[("name", info.name), ("model", &info.model)]),
        None => tl!("Off. Configure a local or external provider to use Generative Fill.").to_string(),
    };
    faint(ui, &status);
    ui.add_space(4.0);
    crate::widgets::secondary_button(ui, tl!("Generative AI Settings…"), 0.0).clicked()
}

#[cfg(test)]
#[path = "genai_ui_tests.rs"]
mod tests;
