# Regional image providers

Status: **PARTIAL**. The provider, consent, transaction and validation contracts have synthetic automated coverage. An installed local model, live remote generation and artistic quality have not been qualified. No backend or model is bundled. Painting and comparison do not require a provider.

## Artist workflow

1. Open Preferences → Integrations → Generative AI Settings. Providers are off by default. Configure a compatible local AUTOMATIC1111 API endpoint, or Gemini with your own key. Local setup does not require a key. Configuration is a direct native action; scripts cannot save credentials or enable a provider through dialog confirmation.
2. Select the region to refine. Add separate editable guidance through Window → AI Guidance. Protection and modification coverage constrain the final result locally. Highlight, shadow, fold direction, material boundary, edge preservation, texture direction, depth cue and reflection are explicit semantic references.
3. Open Edit → Generative Fill. Enter the requested edit, material and lighting. Set context padding, then choose Review request. Review displays provider, model, endpoint, input size and the exact image-context rectangle.
4. Generate confirms this specific request. External transmission requires direct native confirmation. Changing the document, provider or reviewed parameters invalidates permission. Every external attempt consumes its grant; action replay, control commands and synthetic clicks cannot grant one.
5. A validated response becomes a new masked, editable result layer. Use Window → AI Results for visibility, opacity, mask editing, accept/reject and comparison. Regenerate uses current artwork and guidance with the selected previous result hidden. The previous layer and record remain; Undo restores the prior state in one step.

The jacket-highlight example can use a fold-direction stroke with an arrow, a silhouette protection stroke, material “dark woven jacket,” lighting “upper left,” and an instruction to preserve seams, proportions and palette. The included jacket images and provider fixtures demonstrate UI/contracts only, not generated artwork or model quality.

## Provider boundary

This extends texjer's existing upstream [PR #2878](https://github.com/storytold/photocraft/pull/2878), inspected at `4754d3c6edb9b37a7ad8516f15425abeca84987d`. `photocraft-genai` retains its ImageProvider, HttpTransport, ApiKey and image-editing abstractions; engine jobs and native credential services reuse the same architecture. Typed guidance extends that interface rather than introducing another document model or renderer. The transport is supplied by the native application; lower crates cannot open sockets or read credentials. No model selection is hardwired into painting tools.

The local adapter accepts only `http://127.0.0.1:PORT` or `http://[::1]:PORT` with a numeric port from 1 to 65535. It posts to `/sdapi/v1/img2img`, with a PNG input/mask, requested dimensions, strength, steps and optional checkpoint name. It requests image return without image saving and restores a temporary checkpoint override. Proxies and redirects are disabled. A local backend must be separately installed, started with its API enabled, and checked against its own `/docs`. Local destination enforcement does not prove that the backend itself is offline. The adapter handles protect/modify masks; other visible semantic references are rejected before transmission because this endpoint has no implemented semantic-reference protocol. Explicitly hide those layers or select a compatible provider.

Gemini uses its fixed HTTPS API destination and the upstream model registry. The request contains artwork, an edit mask, and up to eight named semantic masks plus textual annotations and stroke start/end direction. `store:false` disables stored Interactions/chaining; broader provider terms and retention policies still apply. No key or document pixels were sent during qualification. A provider's acceptance of this payload does not establish that it follows the artist's intent accurately.

## Bounds, color and transactions

Review performs validation without rendering pixels. Source rendering, masks, encoding, response decoding and placement run through existing background jobs. Region plus padding must fit within 2048 pixels per side. Context can grow inside the canvas to match the provider aspect ratio; that exact rectangle is disclosed before confirmation. Inputs are not silently reduced from a full document. Local input dimensions round upward to multiples of eight. Output size in the recipe must equal the native region dimensions.

Only the disclosed composite rectangle is encoded, using CMS conversion to sRGB RGBA8. The original document remains in its native RGB/Gray/CMYK/Lab mode and 8/16/F32 depth. Indexed, Bitmap, Duotone and Multichannel require an explicit conversion of a copy. The selection, visible protection mask and any intersecting modification coverage are combined into a final local mask. Protection limits result visibility even if a provider ignores its mask. Partially opaque protection permits a corresponding partial change.

Encoded image/mask/reference bytes are capped at 32 MiB total. Inputs/masks must agree in dimensions. Prompt including material/lighting is capped at 8,000 bytes. Semantic names are whitelisted; at most 256 annotations per semantic, 1,024 encoded bytes per annotation, and 16 KiB combined annotation text are accepted. A stored guidance annotation near its 1,024-byte document limit may exceed the provider limit after JSON direction metadata/escaping is added; this fails explicitly before transmission. Reduce the annotation rather than losing it silently. Guidance masks have their own bounded segment-work limit.

The native transport caps request bodies at 64 MiB and response bodies at 48 MiB, with a 180-second timeout. The response cap includes JSON/base64 overhead, so the usable encoded image can be smaller than the independent 48 MiB result-byte cap. PNG/JPEG/WebP dimensions are capped at 4096 per side and 16,777,216 pixels; decoder allocation at 256 MiB. Malformed/unsupported output fails atomically. Responses must be within 2% of the requested input aspect, then are fitted to context and cropped to the native region. CMS maps pixels back to the document format. Oversized or mismatched provider outputs are rejected rather than changing the canvas.

Result provenance stores provider/model/clean endpoint, origin, timestamp, recipe, request and encoded-response hashes, actual context/input/decoded sizes, a guidance digest and sent semantic annotations. These records remain frozen when editable guidance changes. Digests identify request/response data, not later hand-painted pixels. Native `.pcraft` saves preserve them; older records may omit generation details. Non-native exports warn about provenance/guidance loss.

Cancellation prevents insertion, but it cannot retract a request already sent. A blocking HTTP call can remain active until it returns or times out; the single-in-flight guard persists through cancellation and provider changes. Stale document/provider results are discarded. Errors or cancellation create no history entry or result layer.

## Credentials and automation

Windows Credential Manager and macOS Keychain retain Gemini keys; Linux/BSD fallback keeps them in process memory only. Keys are outside preferences, document data and inspectable dialog fields. Credential-store operations were not exercised against real credentials. Only local synthetic socket responses were used by transport tests.

`ai.edit.review` exposes a disclosure without authorizing external work. `edit.generativeFill` takes a strict `{recipe, replace?}` object. A configured local provider can be invoked by the normal command API; remote requests require the trusted native review/confirmation path. Selection bounds are cached by document identity and replacement layer, so stable dialogs do not scan a full selection every frame.

## Qualification and remaining work

See `provider-qualification.md` in the accompanying output packet for measured test counts, benchmarks, native screenshots and retained failures. The snapshot `--provider-fixture` installs an example-only service with no transport and unsupported image generation; it cannot access credentials or call a model. Mock pass-through images are not AI output.

Physical tablets, native Linux/Wayland/macOS, browser interactions, provider image quality, backend availability and current-main integration remain open. Advanced bristle simulation stays blocked by the large-brush performance gate. Release requires a separate licensing/contribution review and the owner's explicit approval; this local implementation is unpublished.

## Protocol references

- [AUTOMATIC1111 API guide](https://github.com/AUTOMATIC1111/stable-diffusion-webui/wiki/API) and [request/response schema source](https://github.com/AUTOMATIC1111/stable-diffusion-webui/blob/master/modules/api/models.py). The installed backend's `/docs` is authoritative for its version. No backend implementation code is copied or bundled.
- [Gemini image generation](https://ai.google.dev/gemini-api/docs/image-generation), [Interactions request-format changes](https://ai.google.dev/gemini-api/docs/interactions-breaking-changes-may-2026), and [Interactions data storage](https://ai.google.dev/gemini-api/docs/interactions-overview#data-storage-and-retention), checked 2026-10-11. These APIs/model names can change; successful live integration is not claimed.
