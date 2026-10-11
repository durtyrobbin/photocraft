# Editable AI guidance

Guidance is explicit document data, separate from artwork layers and generated result layers. Open **Window → AI Guidance**, add a guidance layer, choose the semantic brush, then enable **Draw guidance**. Escape cancels the current stroke and leaves drawing mode. Normal pan/zoom remains available. Comparison takes priority over guidance drawing and hides its overlay.

Each stroke stores its semantic enum, document-coordinate points and pressure, width, hardness, opacity, pressure-width response, direction-arrow flag and annotation. The ten semantics are highlight, shadow, fold direction, material boundary, edge preservation, texture direction, depth cue, reflection, area to modify and area to protect. Colors aid visibility; they never determine meaning. Labels and annotations accompany strokes.

Guidance layers can be named, hidden, locked, faded and deleted. Select a stroke in the panel to change its semantics or annotation, apply current brush settings, move it in document pixels, or remove it. `guidance.stroke.set` also replaces the complete point sequence and supports arbitrary bounded translation. A pointer gesture previews without touching the document; release adds exactly one Undo step. Escape, a changed source document, or leaving guidance mode discards the pending stroke.

## Storage and bounds

Native `.pcraft` files preserve guidance. Old files default to an empty collection. Save and load validate layer/stroke IDs, numeric ranges, text sizes and aggregate limits. Other export formats omit the guidance and show a warning to save `.pcraft`; normal flattened image export never includes its overlay.

The collection is copy-on-write and limited to 16 layers, 1,024 strokes, 65,536 points total and 4,096 points in a stroke. Width is 1–512 document pixels; normalized opacity, hardness and pressure are finite. Annotations are at most 1,024 UTF-8 bytes. A pointer stroke that reaches the point limit is cancelled with a message, rather than silently changing its path.

`guidance.mask` rasterizes one requested semantic into gray8 coverage at an explicit region and output size. Gray8 is an intent mask, independent of the document's color channels or sample depth. Hidden layers are excluded. Layer/stroke opacity, pressure width and hardness participate. It uses bounded swept segments, at most 2,048 pixels per output side and 32 million segment-pixel evaluations. Excess work fails before output raster allocation. No network service is involved.

## Command examples

`guidance.layer.add {"name":"Jacket highlights"}` returns a guidance layer ID, independent of artwork layer IDs.

`guidance.stroke.add {"layer":1,"stroke":{"semantic":"foldDirection","points":[{"x":200,"y":100,"pressure":0.4},{"x":250,"y":180,"pressure":1}],"width":12,"hardness":0.8,"opacity":1,"pressureSize":true,"arrow":true,"annotation":"Follow this fold"}}`

`guidance.stroke.set {"layer":1,"stroke":1,"semantic":"areaProtect","translate":[10,-5]}`

`guidance.mask {"region":{"x0":100,"y0":50,"x1":400,"y1":350},"size":[300,300],"semantic":"areaProtect"}` returns explicit semantic metadata and base64 gray8 bytes.

## Limits of this phase

The overlay is a vector editing aid; its solid display strokes do not preview the exact soft mask coverage. The raster mask command uses the stored hardness. Guidance remains at document coordinates; automatic remapping during canvas resizing/cropping/rotation is not implemented. Translate or revise guidance after changing document geometry. Point editing is available through the command API; the panel offers selection, property edits and translation, not individual point handles.

The native pointer path currently samples the available per-frame pointer position/pressure; physical pen and coalesced tablet-event quality require hardware qualification. No provider is invoked by guidance alone, and a protection semantic is not a promise about an external model. Provider integration must enforce protection when inserting its result, retain original intent in provenance, and require explicit external authorization.
