//! Combined PDF export. Retain immutable snapshots until the atomic write succeeds.
use crate::{EngineError, Result, Session, commands::CommandSpec};
use photocraft_doc::{DocId, Document, Layer, LayerContent};
use photocraft_io::ExportOptions;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;

/// Flattening composites source layers; it does not retain editable PDF layers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PdfBinderMode {
    #[default]
    #[serde(rename = "flatten")]
    FlattenTabs,
    #[serde(rename = "layersAsPages")]
    LayersAsPages,
    #[serde(rename = "preserveLayersOcg")]
    PreserveLayersOcg,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerPolicy {
    pub include_all_layers: bool,
    pub include_hidden_layers: bool,
}
impl Default for LayerPolicy {
    fn default() -> Self {
        Self { include_all_layers: true, include_hidden_layers: false }
    }
}

/// Vector order is PDF page order, captured once, independent of live tab positions.
#[derive(Clone, Debug)]
pub struct BinderDocumentSelection {
    pub document_id: DocId,
    pub layer_policy: LayerPolicy,
    pub mode: PdfBinderMode,
    pub layer_ids: Option<Vec<photocraft_doc::LayerId>>,
}
#[derive(Clone, Debug)]
pub struct BinderExportPlan {
    pub documents: Vec<BinderDocumentSelection>,
    pub paired: bool,
    pub force_psb: bool,
}
impl BinderExportPlan {
    /// Resolve positional fixture flags immediately. GUI callers always pass stable document IDs.
    pub fn from_params(session: &Session, params: &Value) -> Result<Self> {
        let bad = |msg: &str| EngineError::Other(msg.into());
        let mode =
            params.get("mode").map(|v| serde_json::from_value(v.clone())).transpose().map_err(|_| bad("Unsupported PDF binder mode"))?.unwrap_or_default();
        let flag = |key: &str, default| -> Result<bool> {
            params.get(key).map_or(Ok(default), |v| v.as_bool().ok_or_else(|| bad("PDF binder layer options must be boolean")))
        };
        let policy = LayerPolicy {
            include_all_layers: flag("includeAllLayers", true)?,
            include_hidden_layers: flag("includeHiddenLayers", mode == PdfBinderMode::PreserveLayersOcg || flag("pairedPhotoshop", false)?)?,
        };
        if params.get("documentIds").is_some() && params.get("tabs").is_some() {
            return Err(bad("Use documentIds or tabs, not both"));
        }
        let ids: Vec<DocId> = if let Some(ids) = params.get("documentIds") {
            ids.as_array()
                .ok_or_else(|| bad("documentIds must be an array"))?
                .iter()
                .map(|v| v.as_u64().map(DocId).ok_or_else(|| bad("Invalid document ID")))
                .collect::<Result<_>>()?
        } else if let Some(flags) = params.get("tabs") {
            let flags = flags.as_array().ok_or_else(|| bad("tabs must be a boolean array"))?;
            if flags.len() != session.documents().len() || flags.iter().any(|v| !v.is_boolean()) {
                return Err(bad("tabs must match the open documents"));
            }
            session.documents().iter().zip(flags).filter(|(_, v)| v.as_bool() == Some(true)).map(|(st, _)| st.doc.id).collect()
        } else {
            session.documents().iter().map(|st| st.doc.id).collect()
        };
        if params.get("layerIds").is_some_and(|v| !v.is_object()) {
            return Err(bad("layerIds must be an object keyed by document ID"));
        }
        let mut documents = Vec::new();
        for document_id in ids {
            let layer_ids = params
                .get("layerIds")
                .and_then(|v| v.get(document_id.0.to_string()))
                .map(|v| {
                    v.as_array()
                        .ok_or_else(|| bad("layerIds must map document IDs to arrays"))?
                        .iter()
                        .map(|id| id.as_u64().map(photocraft_doc::LayerId).ok_or_else(|| bad("Invalid layer ID")))
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?;
            if layer_ids.as_ref().is_some_and(|ids| ids.len() > 4096) {
                return Err(bad("Select at most 4096 layers per document"));
            }
            if !policy.include_all_layers && layer_ids.as_ref().is_none_or(Vec::is_empty) {
                return Err(bad("Select at least one layer per selected document"));
            }
            documents.push(BinderDocumentSelection { document_id, layer_policy: policy, mode, layer_ids });
        }
        Ok(Self { documents, paired: flag("pairedPhotoshop", false)?, force_psb: flag("forcePsb", false)? })
    }
}

/// Count contributing leaves respecting ancestor visibility. Cached Smart Object appearances
/// are authoritative: exporting must never fetch an external linked file behind the user's back.
pub fn eligible_layers(doc: &Document, hidden: bool) -> usize {
    let mut stack: Vec<_> = doc.layers.iter().collect();
    let mut count = 0;
    while let Some(layer) = stack.pop() {
        if !hidden && !layer.visible {
            continue;
        }
        if let Some(children) = layer.children() {
            stack.extend(children);
        } else {
            count += 1;
        }
    }
    count
}

fn prepare_layers(layers: &mut [Layer], hidden: bool, depth: usize) -> Result<()> {
    if depth > 128 {
        return Err(EngineError::Other("PDF binder layer nesting exceeds 128 levels".into()));
    }
    for layer in layers {
        if hidden {
            layer.visible = true;
        }
        if !layer.visible {
            continue;
        }
        if let LayerContent::Smart(smart) = &layer.content
            && smart.cache.is_none()
        {
            return Err(EngineError::Other(format!("Smart Object '{}' has no rendered appearance; refresh or rasterize it before export", layer.name)));
        }
        if let Some(children) = layer.children_mut() {
            prepare_layers(children, hidden, depth + 1)?;
        }
    }
    Ok(())
}

pub struct Snapshot {
    docs: Vec<(Arc<Document>, u64)>,
    rendered: Vec<Document>,
    mode: PdfBinderMode,
    paired: bool,
    force_psb: bool,
}

impl Snapshot {
    pub fn capture(session: &Session) -> Result<Self> {
        let plan = BinderExportPlan::from_params(session, &json!({}))?;
        Self::capture_plan(session, &plan)
    }

    pub fn capture_plan(session: &Session, plan: &BinderExportPlan) -> Result<Self> {
        if plan.documents.is_empty() || plan.documents.len() > 100 {
            return Err(EngineError::Other("Select between 1 and 100 documents".into()));
        }
        let mut seen = HashSet::new();
        let mut docs = Vec::new();
        let mut rendered = Vec::new();
        for selected in &plan.documents {
            if plan.documents.first().is_some_and(|first| first.mode != selected.mode) {
                return Err(EngineError::Other("Use one PDF mode for the whole binder".into()));
            }
            if !seen.insert(selected.document_id) {
                return Err(EngineError::Other("Duplicate PDF binder document ID".into()));
            }
            let st = session
                .documents()
                .iter()
                .find(|st| st.doc.id == selected.document_id)
                .ok_or_else(|| EngineError::Other("A selected document is no longer open".into()))?;
            if st.doc.max_group_depth() > 128 {
                return Err(EngineError::Other("PDF binder layer nesting exceeds 128 levels".into()));
            }
            let mut doc = (*st.doc).clone();
            if !selected.layer_policy.include_all_layers {
                let ids = selected.layer_ids.as_deref().ok_or_else(|| EngineError::Other("Select layers to export".into()))?;
                if ids.is_empty() || ids.iter().any(|id| doc.layer(*id).is_none()) {
                    return Err(EngineError::Other("A selected layer no longer exists".into()));
                }
                retain_selected(&mut doc.layers, ids);
            }
            if selected.mode != PdfBinderMode::FlattenTabs || plan.paired {
                if !selected.layer_policy.include_hidden_layers {
                    retain_visible(&mut doc.layers);
                }
                // Validate cached appearances without modifying default visibility in the snapshot.
                let mut checked = doc.layers.clone();
                prepare_layers(&mut checked, true, 0)?;
            } else {
                prepare_layers(&mut doc.layers, selected.layer_policy.include_hidden_layers, 0)?;
            }
            rendered.push(doc);
            docs.push((Arc::clone(&st.doc), st.revision));
        }
        Ok(Self { docs, rendered, mode: plan.documents.first().map(|d| d.mode).unwrap_or_default(), paired: plan.paired, force_psb: plan.force_psb })
    }

    pub fn render(&self, progress: &mut dyn FnMut(f32) -> std::result::Result<(), String>) -> Result<photocraft_io::ExportResult> {
        let docs: Vec<_> = self.rendered.iter().collect();
        let weight = if self.paired { 0.5 } else { 1.0 };
        let mut pdf_progress = |f| progress(f * weight);
        let out = match self.mode {
            PdfBinderMode::FlattenTabs => photocraft_io::pdf::export_pdf_canvases(&docs, &ExportOptions::default(), &mut pdf_progress),
            PdfBinderMode::LayersAsPages => photocraft_io::pdf_layers::export_layers_as_pages(&docs, &ExportOptions::default(), &mut pdf_progress),
            PdfBinderMode::PreserveLayersOcg => photocraft_io::pdf_layers::export_ocg(&docs, &mut pdf_progress),
        }
        .map_err(|e| EngineError::Other(e.to_string()))?;
        if self.paired {
            photocraft_io::pdf_layers::paired_package(&docs, out, self.force_psb, &mut |f| progress(0.5 + f * 0.5))
                .map_err(|e| EngineError::Other(e.to_string()))
        } else {
            Ok(out)
        }
    }

    /// Only call after the complete atomic write succeeds.
    pub fn finish(self, session: &mut Session, path: &str, close: bool, bytes: usize, warnings: Vec<String>) -> Value {
        let mut closed = 0;
        if close {
            for i in (0..session.documents().len()).rev() {
                let st = &session.documents()[i];
                if session.job_on(st.doc.id).is_none()
                    && self.docs.iter().any(|(doc, revision)| doc.id == st.doc.id && *revision == st.revision && Arc::ptr_eq(doc, &st.doc))
                {
                    session.close(i);
                    closed += 1;
                }
            }
        }
        json!({"path":path, "tabs":self.docs.len(), "bytes":bytes, "closed":closed, "warnings":warnings})
    }

    /// Compose in the existing cancellable job system. Commit runs only after the worker
    /// result is accepted, so a cancelled worker cannot replace a destination in the background.
    pub fn start(self, session: &mut Session, path: String, close: bool) -> Result<crate::jobs::Started> {
        validate_path(&path, self.paired)?;
        if cfg!(target_arch = "wasm32") {
            return Err(EngineError::Other("PDF binder file export requires the desktop app".into()));
        }
        let target = path.clone();
        session.start_job(
            "file.export.allTabsPdf",
            json!({"path": path}),
            "Export All Tabs to PDF\u{2026}",
            false,
            move |ctx| {
                let out = self.render(&mut |fraction| {
                    ctx.check().map_err(|e| e.to_string())?;
                    ctx.progress(fraction * 0.9, "Compositing PDF pages");
                    Ok(())
                })?;
                ctx.check()?;
                ctx.progress(0.95, "Writing temporary PDF");
                let pending = photocraft_format::atomic::PreparedWrite::prepare(std::path::Path::new(&target), &out.bytes)
                    .map_err(|e| EngineError::Other(e.to_string()))?;
                ctx.check()?;
                Ok((self, pending, out.bytes.len(), out.warnings))
            },
            move |session, (snapshot, pending, bytes, warnings)| {
                pending.commit().map_err(|e| EngineError::Other(e.to_string()))?;
                Ok(snapshot.finish(session, &path, close, bytes, warnings))
            },
        )
    }

    /// The writer must report success only after the complete file has been committed.
    /// Closing is opt-in. New tabs and tabs edited since capture always remain open.
    pub fn write(self, session: &mut Session, path: &str, close: bool, write: impl FnOnce(&str, &[u8]) -> std::result::Result<(), String>) -> Result<Value> {
        validate_path(path, self.paired)?;
        let out = self.render(&mut |_| Ok(()))?;
        write(path, &out.bytes).map_err(EngineError::Other)?;
        Ok(self.finish(session, path, close, out.bytes.len(), out.warnings))
    }
}

fn validate_path(path: &str, paired: bool) -> Result<()> {
    let extension = if paired { "zip" } else { "pdf" };
    if !std::path::Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case(extension)) {
        return Err(EngineError::Other(format!("Choose a filename ending in .{extension}")));
    }
    Ok(())
}

fn run(session: &mut Session, params: &Value) -> Result<Value> {
    let bad = || EngineError::BadParams { cmd: "file.export.allTabsPdf".into(), msg: "expected {path: string, closeAfter?: boolean}".into() };
    let path = params.get("path").and_then(Value::as_str).filter(|p| !p.is_empty()).ok_or_else(bad)?;
    let close = match params.get("closeAfter") {
        None => false,
        Some(v) => v.as_bool().ok_or_else(bad)?,
    };
    Snapshot::capture_plan(session, &BinderExportPlan::from_params(session, params)?)?
        .write(session, path, close, |path, bytes| crate::file_cmds::write_file(path, bytes).map_err(|e| e.to_string()))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "file.export.allTabsPdf",
        label: "Export All Tabs to PDF…",
        menu: &["File", "Export"],
        shortcut: None,
        params: r#"{"path":str,"closeAfter":bool=false,"documentIds"?:[u64],"tabs"?:[bool],"mode":"flatten|layersAsPages|preserveLayersOcg","includeAllLayers":bool=true,"includeHiddenLayers"?:bool,"layerIds"?:{documentId:[layerId]},"pairedPhotoshop":bool=false,"forcePsb":bool=false}"#,
        journal: true,
        enabled: crate::file_cmds::native_doc,
        run,
    }]
}

fn retain_visible(layers: &mut Vec<Layer>) {
    layers.retain(|l| l.visible);
    for layer in layers {
        if let Some(children) = layer.children_mut() {
            retain_visible(children);
        }
    }
}
fn retain_selected(layers: &mut Vec<Layer>, ids: &[photocraft_doc::LayerId]) {
    layers.retain_mut(|layer| {
        if ids.contains(&layer.id) {
            return true;
        }
        if let Some(children) = layer.children_mut() {
            retain_selected(children, ids);
            !children.is_empty()
        } else {
            false
        }
    });
}
