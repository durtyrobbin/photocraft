# Universal interchange audit — 2026-10-11

Gate 0 baseline: Binder 2.0 `376802580556965aaaf3c2a836676798c9e4e12d`, merged with fetched upstream `334ae57` in local `eb674ed`. Worktree: `photocraft-universal`, branch `universal-format-engine`, on verified migrated E: storage. Original checkout and installed applications are unchanged. No publication authorized.

## Existing architecture and entry points

`io::import_with` / `io::export`, the existing Document model, CMS, CPU compositor, command registry and platform services remain authoritative. Open and Place share decoding (Place may embed as Smart Object). Native Save retains source eligibility; read-only imports cannot overwrite their source automatically. Save As and CLI conversion/batch use the IO encoders; UI format lists are narrower than IO capability. Thus an encoder below is not a claim that every picker exposes it. The baseline Export As listed PNG/JPEG/WebP/TIFF/TGA/PDF. The local patch now derives its encoder list from existing codec capabilities and includes native/PSD/PSB/ORA/PDF; optional read-only codecs are not advertised as encoders. Layered formats retain the editing stack instead of honoring a stale flat-format transparency checkbox. Save As additionally exposes native/PSD/PSB/ORA and other configured formats. Batch has the same encoding limits as IO. Native files remain the full editing archive.

In the matrix, O/P = Open and Place through the shared importer; S/A/E/B = Save (eligible source only), Save As, IO export and batch encoder availability. 'yes' describes implemented paths, not universal fidelity or independent qualification.

|Format|O/P|S/A/E/B|Layers / groups / masks|Channels / alpha|Vector objects|Depth|ICC / metadata|Animation or pages|Round-trip limits|
|---|---|---|---|---|---|---|---|---|---|
|pcraft|yes|yes|native model|native|native|8/16/32f|native|artboards/video model|schema-complete model; preserve unknown version gates|
|PDF|desktop raster pages/Smart Object|source read-only / yes / yes / yes|flattened baseline; local supported OCG profile implemented|RGB alpha soft mask export|rasterized baseline|8-bit sRGB export; import selected ppi|CMS conversion / selected XMP|100 pages; binder selected tabs|not Photoshop editing data; PDF text/forms/links lost on raster import|
|PSD|yes|yes|raster/groups/masks, modeled type/vector/adjustments/effects/Smart Objects plus opaque blocks|RGB/Gray/CMYK/Lab and auxiliary channels|supported PSD paths/type|1/8/16/32 import; supported 8/16/32 export|ICC/resources retained|artboards, not general pages|unsupported semantics produce warnings; corpus floors are not total Photoshop parity|
|PSB|yes|yes|same PSD model|same PSD model|same PSD model|8/16/32|ICC/resources|artboards|large-document container; historical >4GB Windows qualification remains separate|
|ORA|yes|yes|raster/groups; leaf masks baked; group masks/adjustments limited|RGBA, no auxiliary channels|rasterized independently|8/16 PNG; float quantized|profile limitations, metadata warning|one layered canvas|ZIP stack; visibility/order/blends/offsets; fix pending fill opacity/name whitespace|
|EXR|yes with part/channel picker|yes, flat baseline|parts/channel groups reconstructed; no native blend semantics|named channels/deep readers; alpha|none|half/full float; U32 exactness limited beyond 2^24|linear scene data; EXR attributes limited|multipart read, flat write|do not apply display transforms to depth/normals; multipart write pending|
|TIFF|yes; page/layer handling|yes, optional layered export|Photoshop tags 37724/34377 reuse PSD conversion|RGB/Gray/CMYK alpha; auxiliary export gap|via Photoshop layer extension|8/16/32f|ICC/XMP/DPI/text|multipage/BigTIFF read; image/layered write|generic multipage is not layers; spot/alpha PR pending review|
|PNG|yes|yes|flat only|Gray/RGB alpha|none|8/16|ICC/EXIF/XMP/DPI/text|codec first frame|lossless pixels; no editable stack|
|JPEG|yes|yes|flat only|Gray/RGB/CMYK; no alpha|none|8|ICC/EXIF/XMP/DPI|single|lossy; transparency flattened explicitly|
|WebP|yes|yes|flat/frame model|RGBA|none|8|ICC/EXIF/XMP|separate animation path; codec first frame|lossless/lossy; no general layers|
|SVG|yes; supported paths/groups|read-only baseline; export pending|shape groups; complex filters/clips raster fallback|RGBA appearance|native supported shapes; text outlined|vector + raster fallback|not native ICC/metadata archive|one viewBox|gradient geometry approximations; bounded SVGZ/XML/rasterization|
|JXL|not general images at baseline|no baseline encoder|no general layer model|pending decoder PR|none|pending decoder PR|pending decoder PR|pending|DNG internal decoder is not standalone format support|
|AVIF|no decoder|optional encoder|flat|RGB alpha|none|configured encoder depth|codec-specific|still|pure Rust encode only|
|HEIF/HEIC|optional decoder|read only|flat|RGB alpha|none|8/16|ICC/metadata decoder-specific|still|optional heic-rs, no HEVC encoder|
|GIF/BMP/TGA/ICO/PNM/QOI/HDR|existing codecs retained|existing writers retained|flat (animation separate)|format-dependent alpha; HDR RGB|none|integer formats or HDR F32/RGBE|format-specific caps|GIF frame workflow, ICO images|no generic editable layers; codec caps remain source of truth|

## Security, licensing and reuse

PDF: 256MB input, 100 pages, 16,384px side, 64MP/page, 512MP total; decoded streams 64MB/256MB total, embedded images 16MP/64MP total. Unsupported JPX/JBIG2/CCITT/LZW/filter chains and inline images reject before rendering. Native unwind guard; wasm import rejects before parsing. Export bands are cancellable, snapshots immutable, writes prepared then atomically committed before optional tab close. All new modes must retain those boundaries.

ORA: bounded ZIP/XML/pixels and depth. SVG: bounded nodes, recursion, inflated XML and fallback pixels. PSD/TIFF: existing codec limits, with additional upstream depth-limit fixes under review. Rust-only, no unsafe, no new native codec shell. Existing dependencies/attribution are retained. Never commit font binaries or proprietary assets.

Relevant fetched active PRs: #2868 JXL decoder (dfbc320); #2872 extra exports (b419451); #3169 mixed vector/raster (7bb746a, contains #2872); #3181 ORA fill opacity (a0b03b0); #3207 ORA names (b6ee682); #3121 TIFF channels (c3f2a22); #2975 PSD/TIFF depth bounds (ebcd229). These are candidate implementations, not verified or merged claims. Mixed-document PRs introduce PDF writers that must be reconciled with `io::pdf_writer`, not duplicated. Existing PDF PRs #2011 and #2751 remain open; Binder ancestry is preserved.

## Photoshop acceptance clarification

The owner requires distinct verification of standard OCG visibility and Photoshop editable layer reconstruction. Adobe documents that generic PDFs rasterize on import, while Photoshop PDFs saved with editing capabilities retain Photoshop data. OCGs, embedded PSD attachments, and matching sidecars do not prove direct Photoshop PDF reconstruction. No Photoshop installation was found in standard Adobe directories or App Paths during initial inspection; experimental verification remains pending. No proprietary private-data representation will be guessed or advertised as compatible.

Sources: https://helpx.adobe.com/photoshop/desktop/create-open-import-images/import-files/open-pdf-files-in-photoshop.html ; https://helpx.adobe.com/photoshop/desktop/save-and-export/save-files/save-as-photoshop-pdf.html ; Adobe public PSD specification and ISO 32000 PDF specification. The implemented PSD writer is the editing interchange path; paired PDF+PSD/PSB must use the same snapshot and retain every supported selected layer.

## Qualification ledger

Audit complete; local PDF/PSD implementation and qualification are recorded in [the Binder 2.1 report](pdf-binder-2.1-qualification.md). The full universal-format mission is still PARTIAL. Existing tests cover PDF page selection/security/canvas crop/binder snapshot/cancellation/atomic writes, ORA/PSD/TIFF structures, codec depths and corpus oracles. New gates: overlapping five-layer/group/mask/blend fixtures, independent MuPDF OCG toggles, independent PSD layer/channel inspection, all affected native tests, clippy/layers/wasm, relevant corpus and panic-hunt, native visual checks, high-entropy release performance. Unavailable applications and unrun gates must remain explicitly pending.

## Binder 2.1 audit and active qualification

Source under qualification: local `eb674ed` plus working changes, upstream `334ae5779ceebbf7681d345b35dd66e79115e352`. `git merge-base --is-ancestor` succeeds for `6c7fea8`, `10e620d`, `0bacacf` and `3768025`. Authoritative import remains `io/pdf.rs`, `ui-egui/pdf_import_ui.rs`, `engine/jobs.rs::pdf_page_documents`; explicit placement remains `file_cmds::place_pdf_bytes`.

Existing: thumbnail virtualization/cache (32), 1–2400ppi resolution, Ctrl/Cmd toggling, Shift range, source-order deduplication, separate stable document IDs, read-only source status, prepare-all then insert tab set, cancellable jobs, target-bound Smart Objects and one placement history transaction. Demonstrated gaps: queue selected placement solely because any tab was active; picker lacked Select All/Clear Selection. Local patch adds explicit destination (Open defaults Separate Tabs, explicit Place defaults captured document) and selection buttons. No second page importer/writer. Twelve-page resolution/isolation/crop/Undo/Redo/binder regressions pass in release, including 72/144/300 ppi dimensions, stable IDs and source protection. Widget/visual and platform qualification are tracked in the linked report.

Owner priority update: retain and credit Skyfuller1303's PR #2751 thumbnail, placement and safety improvements. The remaining work is standard OCG export, qualified Photoshop editing interchange, original PDF layers where supported, and explicit Tabs / Smart Objects / Layers selection. No replacement importer or overlapping public PR is being created. PR head `c85f89d3f1e317227ec873e896b05b1cc4305334` is an ancestor of local HEAD; public API recheck reports open/unmerged.
