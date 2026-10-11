# PDF Binder 2.1 and layered interchange qualification

## Source and isolation

Local integration branch: `universal-format-engine`, based on Binder 2.0 `376802580556965aaaf3c2a836676798c9e4e12d` and upstream `334ae5779ceebbf7681d345b35dd66e79115e352`, joined by `eb674ed`. Implementation commit: `e63823a810cee95e0263f1c8351b491b3d74315b` on the isolated local branch. This is not an upstream release. The history contains `6c7fea8`, `10e620d`, `0bacacf` and `3768025`. PR #2011 and #2751 remain separate public contributions; neither was overwritten or published to. The original `pdf-binder-2` checkout, installed app and user documents remain untouched. Skyfuller1303 contributed the thumbnail previews, Smart Object placement and safety improvements in PR #2751 while retaining durtyrobbin's original commits and attribution. Its current head `c85f89d3f1e317227ec873e896b05b1cc4305334` is already an ancestor of this local branch; the public PR remains open and unmerged as checked on 2026-10-11. This patch does not replace that importer or publish another overlapping PR.

Storage: verified migrated `E:/PROJECTS/AIOS/projects/codex-c-resident/2026-10-09/make/work/photocraft-universal`. The original checkout migration receipt reports PASS; no development storage was created on C:.

## Using the workflow

Open or drop a PDF to show the existing numbered thumbnail picker. **Import As: Separate Tabs** is now explicit and is the default even with another document open. Click one page, Ctrl/Cmd-click to toggle pages, or Shift-click for a range. Select All and Clear Selection operate on the page selection. The selector also offers **Smart Objects** (placement in the captured document) and **Layers** (supported source OCGs reconstructed as independent raster layers, one document per selected page). Tabs and Smart Objects explicitly import page appearances. Layers refuses unsupported or absent OCG structure instead of silently flattening. General third-party OCG reconstruction remains PARTIAL; the verified reconstruction profile is PhotoCraft's bounded standard OCG/Form/image output. Open Selected uses source-page order; Open All opens every page. Resolution remains 1–2400 ppi, default 144, subject to the existing size and decoding limits.

File > Place Embedded retains the original captured document as its target. Changing the active tab cannot redirect placement. Closing that target fails safely and keeps the picker available. Importing several pages prepares the complete set before inserting tabs. An error or cancellation does not insert a partial set. Imported documents have independent IDs and no automatic overwrite path to the source PDF.

Choose **File > Export > Export All Tabs to PDF…** for the binder. Check the wanted tabs; Shift activates an inclusive tab range. Select a mode:

- Flatten each selected tab: one canvas-sized composited page per tab, including the canvas crop.
- Layers as separate PDF pages: one isolated supported leaf per page.
- PDF viewer layers (OCG): one canvas page per tab with real optional-content groups, names, nested groups, visibility, opacity and supported PDF blend modes.

The layer chooser filters each selected document. Selecting a group selects its descendants; select individual leaves when finer control is needed. In OCG/paired output, Include hidden layers retains their hidden state. Ordinary flattened output retains Binder 2.0's explicit include-hidden override, which makes them visible in that snapshot only.

Enable **Include layered PSD/PSB editing files (ZIP package)** to produce one atomic ZIP containing `binder.pdf`, one numbered PSD/PSB per selected document, and a manifest with source names and warnings. Use PSB for every editing file forces the larger container; the existing writer may also upgrade oversized PSD output. This package prevents cross-file partial completion: tabs close only after the whole package is committed, and changed/new/busy tabs remain open. Cancellation or write failure retains all tabs.

## Fidelity boundaries

OCG files contain independently toggleable 8-bit sRGB raster appearances. Leaf masks/effects are baked into alpha/appearance. Type, vectors and Smart Objects are not editable as those types in PDF. The supported import profile reconstructs independent raster layers and groups; other PDF structures require choosing Tabs or Smart Objects for a composited appearance. The lower-level automatic import API also retains its disclosed composited fallback. A modified profile that cannot be validated requires explicit flattened import. The profile marker never bypasses parsing or resource checks.

OCG export rejects backdrop adjustment layers, clipping, Blend If, excluded channels, unsupported blends and group effects/masks that cannot be represented independently. Canvas-sized opaque/transparent artboard backgrounds are supported; other artboard geometry/compositing is rejected. Use flattened PDF plus layered PSD/PSB for unsupported OCG content.

PSD/PSB reuse the established native layer encoder and preserve supported raster layers, masks, groups, visibility, opacity, type, vector shapes, adjustments, effects and opaque supported data. Unsupported semantics still carry the existing format warnings. Native `.pcraft` remains the full editing archive.

**Direct Photoshop PDF compatibility is BLOCKED / unverified.** Adobe distinguishes generic PDF rasterization from Photoshop PDF's editing data. Standard OCGs and PSD attachments do not establish Photoshop reconstruction. No documented, independently verified clean-room private editing representation was found. No Photoshop or Acrobat installation was found in the checked standard Adobe directories, App Paths, Windows uninstall records or running processes. No generated file has been reopened in Photoshop in this qualification. Acrobat application verification is also pending; MuPDF provides independent OCG parsing/rendering evidence only.

Sources: [Adobe PDF import](https://helpx.adobe.com/photoshop/desktop/create-open-import-images/import-files/open-pdf-files-in-photoshop.html), [Adobe Photoshop PDF save](https://helpx.adobe.com/photoshop/desktop/save-and-export/save-files/save-as-photoshop-pdf.html), [public PSD specification](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/), [ISO 32000](https://opensource.adobe.com/dc-acrobat-sdk-docs/standards/pdfstandards/pdf/PDF32000_2008.pdf).

## Qualification ledger

| Requirement | Classification | Evidence |
|---|---|---|
| Existing picker, page order, bounded thumbnails, Ctrl/Cmd/Shift, resolution | VERIFIED | Original implementation reused |
| Explicit Tabs / Smart Objects / Layers and Select All/Clear Selection | IMPLEMENTED AND VERIFIED | `pdf_workflow`: 11 passed; picker widgets: 3 passed, including Shift and small-window layouts; legacy picker retained |
| Original third-party PDF layer reconstruction | PARTIAL | Independent layers supported for the validated OCG/Form/image profile; arbitrary third-party PDF operators are not reconstructed |
| Twelve-page selection at 72/144/300 ppi | IMPLEMENTED AND VERIFIED | Pages 1, 3, 4, 7, deduplication, dimensions, independent IDs |
| Source/document isolation; crop/paint/Undo/Redo | VERIFIED | `pdf_import_tabs`, `pdf_export`, `pdf_binder2` |
| Binder snapshots, cancellation, atomic writes, safe close | VERIFIED | Existing regression tests and paired write-failure test |
| Explicit Smart Object placement, captured target and Undo | VERIFIED | `pdf_workflow` |
| OCG structure, blends, hidden state, groups, transparent canvas | IMPLEMENTED AND VERIFIED for supported profile | Eight IO tests; independent MuPDF baseline/toggle checks |
| Layered PSD/PSB, same-snapshot paired ZIP | IMPLEMENTED AND VERIFIED for tested features | Rust round trips; independent reader evidence below |
| Photoshop direct PDF reopening | BLOCKED | Application unavailable; no verified representation |
| Photoshop PSD/PSB application import | BLOCKED | Independent readers qualify structure, not actual Photoshop |
| Acrobat UI layer controls | BLOCKED | Application unavailable in checked locations |
| Native GUI, high DPI, themes | VERIFIED native launch and offscreen layouts; full live interaction PARTIAL | Real Windows window with three sample tabs and source layers; 48 final captures, ten themes, two viewport sizes, 1x/2x |
| Linux/Wayland and macOS | BLOCKED in this Windows environment | No execution claimed |
| Native corpus, panic hunt, layers, parity, quick performance | VERIFIED | All completed successfully; performance samples are not speedup claims |
| WASM safe rejection and PDF export | VERIFIED | Real Node.js execution of the release no-unwind WASM module |
| Final selector widgets, Clippy and WASM | VERIFIED | Three picker widget tests, strict all-target Clippy and WASM UI check passed after the final selector update |

## Executed tests

Release flags: `--release --locked --offline -j 4 --config profile.release.lto=false --config profile.release.codegen-units=16`; tests use `-- --test-threads=1`. Compiler/cache/output roots are on E:.

`cargo test -p photocraft-ui-egui -p photocraft-io -p photocraft-engine --test pdf_workflow --test pdf_layers --test pdf --test pdf_import_tabs --test pdf_binder2 --test pdf_export` with those flags: **46 passed** at the earlier patch stage, zero failed; one 24MP benchmark intentionally ignored in this functional run. Includes malformed/oversized PDFs, image/stream decoding limits, cancellation, physical page size and canvas crop regressions.

Strict Clippy for IO/engine/UI all targets passed. The subsequent owner-requested three-way selector has its own focused Clippy, widget and WASM rerun. Windows `cargo fmt --all` exceeded the command-length limit (error 206); per-changed-file rustfmt passed for all 26 changed Rust files. `git diff --check` also passed.

Independent toolchain: MuPDF/PyMuPDF 1.28.2 and psd-tools 1.24.0, isolated development environment on E:. The five-leaf/two-level fixture includes noise, transparency, Multiply, Screen, opacity, a half mask, Unicode layer name and a hidden layer. Initial independent OCG baseline error <=2/255; six independent visibility toggles <=3/255. PSD and PSB retain five independently readable pixel records and a group at 8/16/32-bit depth. Independent psd-tools composition differs by at most 1/255. Two additional complex PSD/PSB files pass independent native type, vector-mask, Invert adjustment, nested-group and enabled Drop Shadow checks. All eight source leaves survive in both containers.

Retained diagnostic history: the first independent OCG render exposed double blending in non-isolated leaf Forms; isolated transparency groups fixed it. MuPDF `get_ocgs().on` describes usage state, so the validator now reads default `/OFF` and layer UI state. A build during concurrent editing observed stale Rust metadata; subsequent frozen-source targeted builds passed. No prior failed evidence was rewritten.

Full unit execution: IO **127 passed / 1 ignored**, UI **1848 passed / 10 ignored**. Engine **1207 passed / 12 ignored** with one brittle printing test failure: the test assumed ICC object number 7, while the shared allocator now assigns 3. The assertion now resolves and validates the actual ICC object instead of prescribing an object number; the corpus-enabled rerun passes all 1208 engine unit tests (12 ignored). All 118 native integration binaries passed (536 tests, 9 ignored). All PDF and export-dialog widget tests pass, including keyboard activation, Shift ranges, Ctrl toggling, Select All/Clear and both import destinations. No rasterization prompt regression remains for explicitly placed PDF Smart Objects.

Visual qualification generated 40 page-picker images (all ten themes, 1000x750 and 800x600 points, 1x/2x pixels), four OCG-picker images and four paired-binder images. Inspected representative full-resolution captures and a contact sheet spanning all themes. Actions remain reachable; thumbnail/layer/binder content scrolls at small sizes. No render error, missing glyph or missing asset was observed in those captures. This is native Windows offscreen egui/wgpu execution. A subsequent real desktop launch and screenshot is recorded below; no manual end-to-end binder interaction in that window is claimed.

## Performance and limits

The existing PDF safety policy remains: 256MB input, 100 pages, 16,384-pixel side, 64MP/page, 512MP combined import; decoded stream and image limits remain active. OCG export uses 16MP per embedded image and 64MP aggregate including alpha masks. Thus 4096 squared and larger full-canvas OCG leaves are explicitly rejected, as are ten 2048 squared leaves. These are bounded rejections, not successful large-document performance qualifications. PSD/PSB encoding cancellation is checked between documents; a single existing PSD encode cannot yet be interrupted internally. No performance improvement is claimed.

Evidence folder: sibling `universal-qualification`; native captures/timing reports are stored there. Broader universal-format work (standalone JXL, SVG export, multipart EXR writing, pending TIFF/ORA candidate fixes) remains separately PARTIAL and must not be described as complete based on these PDF gates.

### Measured local samples

The synthetic 12-page picker inspected metadata in 0.24 ms; one bounded thumbnail took 3.45 ms. Queueing the picker took 0.11–0.14 ms (this excludes first-frame display). Importing four selected pages took 5.19 / 6.12 / 12.33 ms at 72 / 144 / 300 ppi, with sampled process peaks 23.2 / 25.3 / 29.9 MB. These are small 1 by 1.44 inch synthetic pages, not architectural scans.

One 2048x2048 high-entropy RGBA layer exported as a 12,591,160-byte OCG PDF in 485.1 ms, with a 59,809,792-byte sampled process peak. Cancellation at the first callback returned in 0.004 ms. The 10/50/100-layer cases and all 4096x4096, 6000x4000 and 8192x8192 cases tested the bounded preflight and rejected without allocating large pixel surfaces. They do not establish high-resolution throughput. The existing 6000x4000 solid-white benchmark passed: flat export 926 ms, binder snapshot/render 922 ms, output 94,871 bytes. Compilation/other qualification work was concurrent; single samples are observational, not an idle-host comparison or proof of speedup. RSS was polled every 5 ms and can miss shorter spikes. PDF page rasterization itself is not internally interruptible; cancellation is checked around rendering and during the bounded preflight/stream decode. First-frame picker display was subsequently measured below; mid-render cancellation on a large real-world PDF remains unmeasured.

### Final gate command record

Executed from the isolated checkout with the release environment described above:

- `cargo xtask layers`, `cargo xtask wasm`, `cargo xtask parity`: passed.
- `cargo xtask test-corpus -p io -p engine -- --test-threads=1`: passed with pinned verified corpora: 97 test/doc-test executables, 1798 passed, 22 ignored, zero failed.
- `cargo test -p photocraft-engine -p photocraft-io --features photocraft-engine/corpus,photocraft-io/corpus,photocraft-io/heif --test panic_hunt --release --locked --offline -j 4 -- --ignored --test-threads=1`: passed. The first runner invocation incorrectly requested nonexistent `photocraft-engine/heif` and failed before tests ran; the corrected command and both logs are retained.
- `cargo xtask perf --quick`: passed; no baseline update. Existing unmeasured/over-budget scenarios remain documented by the tool.
- Release `pdf_web_smoke.wasm`, processed with wasm-bindgen 0.2.129 and executed in Node.js: passed. Import/preview/OCG inspection reject safely without trapping; flat and OCG PDF export survive.
- Fresh `layered_interop_fixture` plus `scripts/qualify_layered_interop.py`: passed for three OCG PDFs, six 8/16/32-bit PSD/PSB files and two complex PSD/PSB files. Photoshop/Acrobat execution remains NOT RUN.

Final bounded export rerun: 2048-squared high-entropy RGBA OCG PDF, 12,591,160 bytes, 547.7 ms, sampled peak 70,877,184 bytes. A cancellation requested after quarter-export returned in 2.57 ms (87.60 ms total); this is export cancellation, not interruption of a real PDF page renderer. Historical earlier measurements above are retained, and differing loaded-host timings are not treated as a regression or improvement claim.

### Native test build and final visual evidence

`cargo build -p photocraft --release --locked --offline -j 4` passed (release LTO disabled and codegen-units 16, as above). Copied the resulting native GUI into the new isolated folder:

`E:/PROJECTS/AIOS/projects/codex-c-resident/2026-10-09/make/work/binder21-layered-test/app/photocraft.exe`

SHA-256: `612259B4E51DDEE9F5EC5009AD8868420C5B1B7808F89836A5CEAAFF33DDAF92`.

Portable marker and settings are beside that executable on E:. Three synthetic `.pcraft` documents live in `binder21-layered-test/samples`: Blue, Orange and Green study. Each has text, eight leaf layers and nested groups; these are small qualification fixtures, not personal documents. The actual Windows window opened all three tabs, showed the editing stack and replied to authenticated `ui.inspect`, `ui.menu.list` and `ui.screenshot`. Its menu includes Export All Tabs to PDF, and `notices` was empty. The screenshot shows type/vector/mask/group/effect/adjustment entries. No missing font/asset or startup error was recorded.

The process subsequently exited; a later control connection was refused. Its saved UI state and logs contain no crash report, but the reason for closure was not established. The app is **not claimed to remain running**. No automatic relaunch or interaction with an installed app was performed. Binder selection/Shift behavior is verified by widget tests and the final offscreen binder, not a full live-window export test; ambient file commands were not routed around the control protocol's security gate.

Final `ui-final` evidence includes 48 PNGs. Inspected the Layers picker and paired binder at 800x600 points/2x and the picker at 1000x750/1x. Content scrolls and primary actions fit. Queue plus first offscreen frame measured 324.5 / 1022.2 ms at 1000x750 (1x/2x), and 233.6 / 691.3 ms at 800x600 (1x/2x). These include harness rendering on a loaded host, not physical input-to-photon display latency. Earlier queue-only timings are retained separately.

Final selector commands: `cargo test -p photocraft-ui-egui --test pdf_workflow --release --locked --offline -j 4 -- --test-threads=1` (11 passed) and `cargo test -p photocraft-ui-egui --lib pdf_import_ui --release --locked --offline -j 4 -- --test-threads=1` (3 passed). Final native capture generator: `cargo build -p photocraft-ui-egui --example pdf_import_demo --release --locked --offline -j 4`, then run that release example with the E: `ui-final` evidence folder.

`cargo clippy -p photocraft-ui-egui --all-targets --locked --offline -j 4 -- -D warnings` and `cargo check -p photocraft-ui-egui --target wasm32-unknown-unknown --locked --offline -j 4` both passed after the final selector changes. No public PR or upstream branch was modified.
