# PDF Binder 2.0 engineering qualification

## Repository reconciliation (2026-10-11)

Working branch: `pdf-binder-2`. Initial integration commit:
`e50b2018258aff6318d7519294e41a541bea2da3`.

Parents: PR #2751 `c85f89d3f1e317227ec873e896b05b1cc4305334` and upstream main
`97136485f0743251ab330d096ca6c9d0f0f6f54c`. PR #2011 remains at
`0bacacf3e13988cd0cea1b4d19987c9cee5e5642`.
Ancestry checks confirmed original `6c7fea8`, `10e620d`, and `0bacacf` are all
ancestors of #2751. Both contributions were open and unmerged at reconciliation.
No new #2751 review comments were present. No existing branch has been overwritten.
The integration merge retains both histories and the union of both parents' language
catalog keys/values. Current main adds v0.7, updated craft-ui controls and docking,
and tab-strip changes; this work uses current Session document ordering.

## IMPLEMENTED AND VERIFIED (Windows native)

Stable DocId checkboxes, All/None, Shift ranges, keyboard Tab/Space, exact page count,
and disabled empty selection use the existing binder dialog. Selection survives
refresh and tab reordering without selecting new or replacement documents. The
mandatory three-tab fixture exports exactly the first two expected composites.
Six labels were added to all 16 catalogs without removing existing entries.

Immutable plans hold ordered document IDs, per-document mode and layer policy.
Flattening uses photocraft-compose and the shared photocraft-io::pdf_writer:
one whole-canvas page per selected tab, including multi-artboard documents.
Save As retains its existing artboard-per-page behavior. Hidden-layer overrides
apply only to cloned snapshots; source layer structure and visibility are retained.
Visible Smart Objects without rendered caches fail explicitly. Existing cached
appearances are used without fetching linked files.

Workers compose and flush temporary output with progress and cancellation. The UI
accepts the result before atomic replacement. Failed/cancelled exports retain tabs;
close-after-success retains new, changed, and busy documents. The final concurrency
regression covers an unfinished edit whose revision has not yet changed.
Existing PDF limits, Crop/Trim behavior and shared print writer are retained.
See [PDF support and compatibility matrix](pdf-support.md).

## Validation environment and commands

Executed on Windows x86_64 with rustc 1.97.1 (8bab26f4f 2026-07-14),
cargo 1.97.1 (c980f4866 2026-06-30). GPU tests ran serially with
PHOTOCRAFT_NO_SYSTEM_FONTS=1. All source, build, Cargo cache, temp and evidence
paths were on the verified E: project mapping. No installed executable was replaced.

For the commands below, `$packages` expands to:

```powershell
$packages = @('-p','photocraft-io','-p','photocraft-format','-p','photocraft-engine',
              '-p','photocraft-ui-egui','-p','photocraft','-p','photocraft-cli')
$release = @('--release','--locked','--offline','-j4',
             '--config','profile.release.lto=false',
             '--config','profile.release.codegen-units=16')
```

| Check | Result / qualification |
| --- | --- |
| `cargo test @packages --lib --tests --no-fail-fast @release --features photocraft-io/corpus,photocraft/heif -- --test-threads=1` | 3,924 passed, 2 failed, 32 ignored in 133 groups; corrected reruns below. This broad run was not rerun in full. |
| Final PDF safety/regression suites | 26 passed, 0 failed; 1 explicitly ignored benchmark (run separately). Binder 2.0: 12; existing binder: 5; UI PDF workflows: 9. |
| `cargo test @packages --lib --test pdf_workflow @release --features photocraft-io/corpus,photocraft/heif -- export_dialog field_tab binder --test-threads=1` | 14 UI unit tests and 1 integration test passed, including native checkbox keyboard focus and prevention of accidental Enter export. |
| `cargo test -p photocraft-io --test heif_unsupported @release -- --test-threads=1` | 1 passed in the correct HEIF-disabled configuration. |
| `cargo test -p photocraft-io --test heif --features heif,corpus @release -- --test-threads=1` | 5 passed in the correct HEIF-enabled corpus configuration. |
| `cargo clippy @packages --all-targets @release -- -D warnings` | Passed on final source; no lint exceptions. |
| `cargo fmt --all -- --check`; `git diff --check` | Passed on final source. |
| `target/release/xtask.exe layers` | 29 crates, no layering violations. |
| `target/release/examples/parity.exe --write docs/parity-checklist.md` | 630/630 live; unchanged floor and catalog. Binder remains an extension. |
| `target/release/xtask.exe wasm` | All 24 configured checks passed, including codecs with HEIF. |
| Final `cargo check --target wasm32-unknown-unknown -p photocraft-engine --locked --offline` | Passed on final source. |
| Compiled release `panic_hunt-*.exe --ignored --test-threads=1` | 1 passed, 20.73 seconds. |
| Actual wasm-bindgen / Node PDF smoke | Passed: all four import/preview entry points reject valid, malformed and hostile fixtures without trapping; existing document and PDF export survive. |

The broad run's HEIF-negative test saw HEIF enabled through the desktop dependency
while its local feature guard said disabled. The isolated negative test and explicit
HEIF+corpus positive tests above resolve this validation-configuration mismatch.
The new keyboard test initially opened the dialog before registering PhotoCraft's
semibold font; fixing test setup order made it pass. No rendering threshold or
product assertion was weakened. Existing PDF/import/Smart Object, crop/rotated
crop/undo, print, color/depth, IO corpus and atomic failure-injection tests passed
in the broad run. The historical drag-preview contrast failure did not reproduce
(all three drag-preview tests passed). Serial GPU qualification avoids a previously
observed parallel d3d10warp.dll crash; this change does not repair that driver.

Final PDF command:

```powershell
cargo test @packages --test pdf_binder2 --test pdf_export --test pdf_workflow @release --features photocraft-io/corpus,photocraft/heif -- --test-threads=1
```

New fixtures cover selection and invalid IDs, unsupported modes, hidden ancestors,
nested opacity/Multiply/masks/cached Smart Objects, empty/transparent canvases,
canvas-versus-artboard sizing, snapshot edits and pending edits, cancellation,
atomic destination preservation, safe closure, and bounded multi-tab export.
Decoded PDF pixels are compared to independently captured compositor results
at defined dimensions with a 0.035 per-channel tolerance after white compositing.

Web runtime command (matching wasm-bindgen 0.2.129):

```powershell
cargo build -p photocraft-io --example pdf_web_smoke --target wasm32-unknown-unknown @release
../wasm-bindgen-tools/wasm-bindgen-0.2.129-x86_64-pc-windows-msvc/wasm-bindgen.exe --target nodejs --out-dir target/binder2-web-smoke target/wasm32-unknown-unknown/release/examples/pdf_web_smoke.wasm
node -e "console.log(require('./target/binder2-web-smoke/pdf_web_smoke.js').pdf_web_smoke())"
```

## Native visual and bounded performance evidence

The current native snapshot example rendered all ten themes (Pro, Pro Medium,
Studio, Studio Light, Classic, Adwaita, Adwaita Dark, Solarized Dark, Breeze Light,
Breeze Dark) at both 800x600 and 1440x900, scale 1: 20 baseline captures. The
contact sheet and full-size Pro capture were inspected. Additional actual checkbox
and row-label clicks produced two selected pages; Select None produced zero pages
and disabled Export. No personal documents were used. Adapter: Microsoft Basic
Render Driver, DX12, CPU canvas with safe GPU (driver 10.0.26100.9278).

Example reproduction after building the snapshot example:

```powershell
target/release/examples/snapshot.exe --safe-gpu --size 1440x900 --scale 1 --script (Get-Content ../binder2-visuals/pro.json -Raw) --click-at 427,393 --out ../binder2-visuals/pro-selected.png
```

- 32 tabs, 8 layers each, 16x12 pixels: passed in 0.03 seconds; sampled process
  peak working set 76,603,392 bytes (10 ms sampling).
- Explicit ignored release benchmark, 6000x4000 solid white U8 RGB: existing
  export 882 ms; binder snapshot plus render 879 ms; PDF 94,871 bytes. Sampled
  process peak 69,902,336 bytes (50 ms sampling, both paths in one process).
  Other tests were running concurrently. This sparse, low-entropy fixture does
  not establish arbitrary-document throughput or memory scaling.

Local evidence is retained beside the checkout in `../binder2-*.log`, JSON summaries,
and `../binder2-visuals/`; these are local qualification artifacts, not bundled assets.

## IMPLEMENTED BUT NOT FULLY VERIFIED

Native qualification is Windows-only. No macOS/Linux CI, physical printer output,
interactive browser session, or independent third-party PDF viewer was exercised.
The WASM runtime smoke tests safe import/preview rejection and continued PDF export,
not browser filesystem binder support. Cancellation is checked between raster bands
and before accepting output; a band or filesystem flush is not preemptible. Resource
limits remain bounded, but the stress/benchmark fixtures do not represent complex
high-entropy production binders or guarantee responsiveness under every workload.

## Follow-up modes: PROPOSED / NOT IMPLEMENTED

Layers as Pages needs explicit rules for inherited visibility, masks, clipping bases,
adjustment dependencies, group isolation and bottom-to-top versus panel ordering.
OCG preservation needs shared-writer support, resource ownership, transparency/blend
interoperability and viewer/security evaluation. Neither mode is advertised as working.
The per-document plan already carries mode and layer policy for future overrides.

## Publication: BLOCKED pending maintainer coordination

A third overlapping PR has not been created. Do not supersede #2011 or push another
contributor's branch. The local enhancement is intended for coordinated integration
with the maintainer's chosen PDF branch.

## Proposed upstream summary

Extend the existing PDF binder dialog with stable-ID tab checkboxes, range selection,
page count and hidden-layer policy. Each selected tab exports one canvas-sized PDF
page using the existing compositor and shared writer. Save As keeps artboard splitting.
A cancellable snapshot job stages the file before atomic replacement and closes only
unchanged exported tabs after success. Source layers and the earlier PDF import,
Smart Object placement and Crop/Trim contributions are preserved.
