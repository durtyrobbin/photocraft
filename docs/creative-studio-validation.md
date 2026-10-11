# Creative Painting Studio: validation ledger

This branch contains independently scoped local work. It is not a completed studio
or a release. Original PhotoCraft additions do not change Photoshop parity counts.

## Source and upstream coordination

The mixer foundation started from `29f1ff9e90f2581ea2964e3276d85c71ab7e79a0`.
The 2026-10-11 upstream refresh resolved to
`e44d96e1d4671b8d9fd64fbabdf88fa8abf0441e`. The original baseline remains pinned.
Upstream #2131 now supplies prepared commits for ordinary Brush/Pencil and should
be retained when this branch integrates current main. #3098, #2885 and #2379 contain
overlapping brush performance work; no speedups from those branches are attributed
to this work. PDF Binder #2011 and PDF integration #2751 are untouched.

Open #2878 supplies an `ImageProvider`/`HttpTransport` interface, bounded encoded
sRGB image/mask interchange and cancellable engine jobs. Its inspected head is
`4754d3c6edb9b37a7ad8516f15425abeca84987d`. #730 offers the OpenAI backend;
#2879 is dependent UI; #2995 and #2649 concern local segmentation. No provider or
network-generation functionality is added by this painting foundation. Later AI
result/guidance work must reconcile these existing contributions.

## Gate 0 fixture and measurement scope

`crates/engine/examples/painting_baseline.rs` is intentionally compatible with the
pinned original source. Copy this example into a detached checkout of the baseline
and build it there. `painting_live.rs` uses the same fixture and path for the new
live mixer. Do not compare the old full-stroke commit timer to a new individual
update as though they measure the same amount of work.

Each case is a fresh process with three layers: a uniform background, a fully
allocated synthetic RGBA8 source with varying RGB and alpha, and an empty paint
layer. Dimensions are 2048², 4096² and 8192². Diameters are 16, 64, 256 and 1024 px;
spacing is 25%, seed 42, pressure one, smoothing off. Each path has 97 points, with
4 px or 48 px horizontal travel per update and a small sinusoidal vertical motion.
The rapid path travels back and forth, retaining a bounded painted region while
generating many interpolated dabs. Fixture construction is outside engine timers.

The process runner records observed peak working set and host CPU utilization at
100 ms intervals. Those process metrics include fixture construction and validation;
brief peaks can be missed. JSON engine timers record begin, average/p50/p95/p99/max
input-update costs, completion, dab throughput and Undo/Redo. Exact Undo/Redo document
comparisons are outside those timers. These are CPU-engine measurements, not UI frame
times, OS pointer-to-display latency or tablet measurements. GPU utilization and
display timings remain explicitly null. No FPS performance gate passes from these
measurements alone.

Build configuration used on Windows: rustc 1.97.1, release, LTO disabled, 16 codegen
units, four compiler jobs. Intel i7-9700K, eight logical processors. Avoid concurrent
compilation or other benchmarks when collecting comparative timings.

```powershell
cargo build -p photocraft-engine --example painting_baseline --release --locked --offline -j4 --config profile.release.lto=false --config profile.release.codegen-units=16
cargo build -p photocraft-engine --example painting_live --release --locked --offline -j4 --config profile.release.lto=false --config profile.release.codegen-units=16
# One example case; repeat all dimension / diameter / speed combinations.
target/release/examples/painting_baseline.exe 4096 256 rapid mixer
target/release/examples/painting_live.exe 4096 256 rapid mixer
```

The original ordinary-stamp benchmark replays the command on release. It does not
include upstream's subsequently merged #2131 optimization. Its baseline must remain
labeled with its actual source revision.

## Mixer optimization

The original incremental implementation built a tiled RGBA32F sample Surface per
dab and allocated temporary pixel vectors in both sampling and spatial deposition.
The optimized path feeds the compositor's contiguous pixels into a native-channel
Region, borrows sampled slices, and reuses deposition scratch vectors. It preserves
the original arithmetic and full-resolution coverage; it does not reduce quality.
An independent frozen pre-optimization kernel checks exact pixels and reservoir
state across RGB/Gray/CMYK/Lab, 8/16/F32, alpha/no-alpha, spatial/uniform paint,
current/composite sampling and transparency lock.

The size envelope now accepts 1024 px tips with at most 4,194,304 pixels per dense
footprint. The 256-tile working-set and input/dab limits remain. Larger sizes fail
explicitly with a suggestion to choose Brush; there is no silent mid-stroke quality
change. Acceptance of a size is not a responsiveness claim.

## Dynamic tip editor

Open **Brush Settings → Design Tip…**, or `ui.set {"tipDesigner":true}`. Paint or
erase the mask, adjust its 32–512 px resolution, and use local Undo (16 snapshots).
Rotation, size, aspect, spacing, flow, noise, texture, scatter, pressure, shape
variation, direction/tilt/barrel angle and brush projection reuse existing engine
settings. Fixed spacing off uses the existing velocity/time spacing behavior.
The actual stroke preview and mask display cache their textures until inputs change.
Every mask cell participates in the editor's cache signature.

**Apply tip and paint** sends `tools.setBrush`; **Save brush preset** also sends the
existing `brush.presets.save`. The compact 16-bit GrayTile and existing preset format
are reused without a new competing preset format. Editor draft state has version 1.
Editing a tip changes no artwork pixels or document history. Resolution changes use
nearest sampling; imported rectangular tips retain their aspect inside a square mask.
Full texture and other advanced controls remain in the existing Brush Settings panel.
New labels use the localization system, with entries in all 16 translation catalogs.
The editor's native interaction, preset round trip and release-input regression are
included in the 1,831 passing UI unit tests from the pre-splatter checkpoint.

## Measured CPU matrix

All 96 baseline/live cases completed with exact Undo/Redo validation. The table gives
the worst mean/p95/p99 input-update times across the three canvas sizes and two
movement speeds in the new live implementation, in milliseconds.

| Diameter | Stamp mean / p95 / p99 | Mixer mean / p95 / p99 |
|---|---|---|
| 16 | 0.112 / 0.173 / 0.207 | 0.582 / 1.218 / 2.002 |
| 64 | 0.371 / 0.463 / 1.005 | 1.417 / 1.856 / 3.076 |
| 256 | 1.337 / 1.940 / 2.374 | 6.193 / 9.491 / 10.480 |
| 1024 | 4.654 / 24.169 / 38.489 | 20.845 / 108.355 / 162.528 |

Prepared mixer completion ranged from 0.674 to 3.451 ms. The original full-stroke
completion ranged from 9.451 to 4,085.397 ms; these completion measurements cover
different work because live rendering has moved that work into input updates.
Observed peak process working set was at most 400.6 MiB for the live matrix.
These results do not qualify 1024 px tips for the stated responsiveness targets.
Even the smaller engine-only timings do not prove display FPS or physical pen latency.

## Procedural splatter (functional checks VERIFIED; performance PARTIAL)

`paint.splatter` records a bounded seeded request as one document edit. Its settings
have version 1 and use the existing GrayTile format for a custom distribution.
The raster kernel uses native channels/depth, selection coverage and transparency
locks. It does not simulate fluid dynamics. Count is capped at 2,048, diameter at
128 px, distance at 1,024 px, bitmap dimensions at 512 square, and worst-case raster
work at 8,388,608 pixel visits. Oversized combinations return an actionable error.

Select **Flick / Splatter** in the Brush flyout. A click paints on press; enabling
gesture mode instead uses the drag's direction and distance on release. Peak recorded
pressure controls droplet count/contact, and gesture distance scales paint load.
The seed stays fixed until edited, so identical requests reproduce identical pixels.
Use **Design Tip…**, apply the drawn tip, then **Use tip distribution**. A custom
distribution replaces the fan spread/directional bias controls; angle and distance
still transform it. Secondary droplets may fall outside its primary distribution.
Paint deposition is independent of the optional animation and reduced-motion setting.
Configuration persists with existing UI state and is available through `ui.set`'s
`splatter` object. The optional feedback is an original geometric brush/finger drawing.

All eight focused tests pass, including native press/gesture/cancel events, deterministic
replay, source preservation, selections, masks, channels and locked/hidden targets.
The final broad unit run passes 1,211 engine, 87 paint and 1,834 UI tests (12 engine
and 10 UI tests ignored). Clippy, layering, WASM, parity and adversarial command
checks pass. The initial broad run exposed three inventory/translation assertions;
those failures are retained in the evidence and the repaired broad run passes.

The release `splatter_probe` uses a 6000×4000 document with background, dense RGBA8
source and empty target. Each row is 32 seeded bursts, each verified with exact
Undo/Redo and source preservation. The custom distribution is a 128² checker mask.
These CPU command timings exclude fixture construction and document comparison.

| Count / maximum diameter | Distribution | Mean / p95 / p99 ms |
|---|---|---|
| 160 / 10 px | Fan | 0.684 / 1.191 / 2.311 |
| 160 / 10 px | Custom | 1.288 / 1.972 / 2.936 |
| 512 / 32 px | Fan | 7.686 / 12.558 / 13.392 |
| 512 / 32 px | Custom | 8.942 / 10.191 / 42.771 |
| 2048 / 32 px | Fan | 27.082 / 27.894 / 28.524 |
| 2048 / 32 px | Custom | 29.101 / 30.663 / 30.941 |

All 192 Undo/Redo checks pass. Observed process peak working set was 135.31 MiB;
peak host CPU was 13.33% across eight logical processors, sampled every 100 ms.
The process ran for 3.50 s including fixtures and validation. Release build used two
compiler jobs, LTO disabled and 16 codegen units. Large bursts exceed 16.67 ms and
the custom medium case has a 42.77 ms outlier; no universal 60 FPS claim is made.
Pointer-to-display and GPU timings remain unmeasured. The native offscreen screenshot
shows click, directional gesture and custom-distribution paint on three separate layers.

```powershell
cargo build -p photocraft-paint -p photocraft-engine -p photocraft-ui-egui --example splatter_probe --example snapshot --release --locked --offline -j2 --config profile.release.lto=false --config profile.release.codegen-units=16
target/release/examples/splatter_probe.exe
```

## Functional and platform gates

### Palette knife (functional checks under qualification)

`paint.paletteKnife` uses a bounded 64² blade contact mask and the same incremental
native-channel mixer transaction. Width (1–1024 px), angle, contact aspect, softness,
seeded edge breakup, pressure contact/opacity, direction tracking and wet/mix/flow/load
are explicit versioned settings. Alt-click samples the blade footprint. Sample All
Layers defaults on; painting targets a separate selected layer. Clean disables automatic
reload, and both clean-pickup and reload options are available in the scrollable window.
No height-map or impasto shading is implemented.

All four native interaction checks pass: actual mixed preview, exact preview/commit,
sampling without paint, Escape/focus/tool cancellation and deferred release samples.
The 1440×900 native screenshot comparison reports 46,707 changed pixels while drawing
and zero difference between the live and committed canvas crop. Engine geometry and
prepared replay/Undo checks pass. One clean-brush assertion initially compared sparse
storage identity instead of pixels; its corrected pixel-value check now passes.
Clippy, layers, WASM and parity pass. The 24 MP fixed/directional blade benchmark is
recorded separately when it completes. Open upstream #3003 was inspected and does not
contain a palette-knife implementation; its vector/transform/picker work is untouched.
The broad checks pass 1,213 engine tests with the repaired assertion excluded from
that older binary. UI passes 1,836 tests but exposes two catalog checks: the README
tool delimiter and five shared wet-paint labels missing in translation catalogs.
Both source corrections are included; the next compiled suite must confirm them.

```powershell
cargo test -p photocraft-paint -p photocraft-engine -p photocraft-ui-egui --lib --release --locked --offline -j2 --config profile.release.lto=false --config profile.release.codegen-units=16 knife_ -- --test-threads=1
target/release/examples/knife_probe.exe 6000 64 rapid directional
```

The mixer foundation unit/integration checks, live/committed screenshots and exact
pixel comparison are described in `mixer-live.md`. The first broad native test run
hit `0xc0000005` in concurrent `arbitrary_rotation_preview` tests on WARP. Both tests
passed serially. This matches the independently reported upstream #3212 issue;
no driver settings or host configuration were changed. Offscreen evidence uses the
Microsoft Basic Render Driver, not a verified physical RTX path.

Relevant reproducible checks:

```powershell
cargo test -p photocraft-paint -p photocraft-engine -p photocraft-ui-egui --lib --tests -- --test-threads=1
cargo clippy -p photocraft-paint -p photocraft-engine -p photocraft-ui-egui --all-targets -- -D warnings
cargo xtask layers
cargo xtask wasm
cargo xtask parity
cargo test -p photocraft-engine --test panic_hunt -- --ignored
cargo xtask perf --quick
```

Native Linux/Wayland/X11, macOS, physical Wacom/Windows pen input, physical GPU
presentation and real pointer-to-display latency remain unqualified on this host.
WASM compile success is not browser interaction qualification.

## Remaining phases and publication

### Artist palette and persistent studio settings (PARTIAL: qualification pending)

The floating **Artist Palette** is shown with Mixer Brush and can be moved, resized
or hidden. Its 256² RGBA16 paint surface uses the existing stamp/mixer kernels,
with an oval contact mask and thumb hole. Deposit, Mix, spatial Pickup, Clear,
16-step local Undo, eight named paint presets and canvas-load deposition are engine
commands (`palette.*`). These edits do not change artwork or document Undo.
Drag the toolbar foreground chip onto the palette, or Alt-click the canvas and
choose **Deposit canvas load** to retain the sampled spatial variation.

Paint is stored in SDR sRGB. Document composite profiles are converted on deposit
and pickup through the existing CMS; HDR/out-of-gamut components are clipped to the
palette's 0..1 range. Palette display goes through the monitor transform. The wheel
extracts hue markers from actual paint and reports approximate complementary,
analogous, triadic and split-complementary relationships. RYB is an explicitly
approximate piecewise hue interpretation; it never changes chosen paint colors.

**Use current image as skin** captures the active image as bounded decorative artwork;
open a local photo/texture first. **Wood skin** restores the original procedural wood.
Artwork is separate from the paint surface and has no effect on mixing or pickup.
Native window placement, tool controls, palette paint/skin/presets, knife settings,
splatter settings and tip-editor drafts now use the existing preferences lifecycle.
Saving is deferred while the pointer is down and serializes only changed state.
This closes the earlier distinction between UI-state serialization and restart persistence.

New checks cover native controls, exact preset/restore paint, source preservation,
floating selections, linear-profile conversion, spatial load, hostile input and restart
round trips. Clippy passes; release runtime and visual checks are pending. The palette
probe reports CPU costs separately from document size and physical display latency.

The local reservoir now has explicit 16/32/64 cell modes with local pickup and
uneven depletion. See `pigment-reservoir.md` for the measured 32-case matrix,
visual comparison and native before/live/commit verification. Sparse zones are
not implemented; large-tip performance remains below target. Splatter is functionally VERIFIED with performance PARTIAL. Palette
knife, artist palette and brush rack are PARTIAL pending final qualification. AI result comparison,
guidance and provider integration remain NOT IMPLEMENTED in this phase. Advanced
bristle simulation is BLOCKED by the unmet fast-mode performance qualification gate.

Repository contributions are MIT OR Apache-2.0; third-party artwork needs permissive
licenses and attribution. No third-party artwork, model weights or provider SDK is
introduced here. Before publishing advanced palette, custom-bristle or AI components,
report their final code/asset/model licensing and contribution implications and obtain
the owner's explicit release approval. Local development does not authorize uploading
document pixels to an external provider, publishing those components or merging main.


## Phase G — Physical brush rack (qualification in progress)

Eight editable factory presets use existing `brush.presets.save` and `tools.setBrush`:
squirrel mop, sable round/flat, hog bristle, filbert, fan, rigger and synthetic round.
Sampled comb/fan contact masks and existing direction/pressure dynamics provide their
initial behavior; family names describe presets, not physical hair simulation.
Hover/focus extends original geometric artwork and shows name/use. Reduced motion,
keyboard-accessible named buttons, 0.5–2× artwork scaling, movable position relative
to the palette, selected family and per-slot custom artwork are persisted through
the existing preference service. `palette.capture` reads a bounded 256×256 composite
for artwork without changing the palette or document; it is not journaled.
Artwork uses document-to-sRGB then monitor conversion, preserves source aspect ratio
when displayed, and never influences brush contact. Settings validate all image
dimensions and encoded lengths before decoding. Original assets use the repository's
MIT OR Apache-2.0 licensing; publication approval remains pending.

Native interaction, hostile-input, restart persistence and 24 MP dense source
benchmarks are present. Status is PARTIAL until their execution and visual inspection
are recorded. No GPU, tablet, frame-rate or physical bristle qualification is claimed.

### E/F/G qualification update — 2026-10-11

Release (`opt-level=3`, LTO off, codegen-units=16): engine **1,218 PASS**,
paint **87 PASS**, UI **1,843 PASS / 1 FAIL / 10 ignored**. The one failure was
rack movement. A UI-only `opt-level=1` retest passed all three rack tests after
using native background dragging and testing the actual title-bar coordinates.
That test also verifies attachment when the palette moves. The added palette
mixing-drag test passed its single local Undo assertions, then exposed the same
movement issue. Palette positioning now uses the same repair; the final combined
movement/resize/compact-menu suite remains pending. Initial failed logs are retained.

Both actual native palette screenshots were rendered and visually inspected.
Paint deposits, mixed pigment, sampled paint on a separate document layer, wood
skin, custom document-image skin and hue markers are visible. Inspection exposed
rack controls covered by the palette; the rack now uses native Options and named
brush menus, leaving its normal height compact. Screenshot refresh is pending.
Saved-tip bitmap bounds are now checked before decoding, including on WASM.
No publication, contributor identity, model-quality or physical GPU/tablet claim.

### E/F/G measured acceptance record

The later combined run passes 1,218 engine and 87 paint tests. UI passes 1,845
tests with one rack menu-sequence test pending repair/retest, plus 10 ignored.
Engine/paint use release opt-level 3; the UI unit-test binary uses opt-level 1
to shorten iteration. Benchmarks and screenshot examples use opt-level 3.
Palette deposit, mixing gesture with one local Undo, resize, native window drag,
skin, spatial pickup, CMS, hostile input and restart persistence tests pass.
Rack selection, case-insensitive artist preset preservation, custom artwork and
hover pass before the menu-sequence failure. Earlier full rack drag/attachment
tests passed; the final compact-menu regression still needs confirmation.

All 12 knife trials use 6000×4000 dense three-layer documents and 97 input points.
Every trial verifies preview/commit, unchanged source and exact Undo/Redo.
Times below are input-update mean / p95 / p99 in milliseconds; they include
all interpolated dabs for each input point, not physical display latency.

| Width | Travel | Fixed blade | Direction-tracking blade |
|---|---|---|---|
| 64 | Slow | 4.813 / 7.315 / 9.650 | 4.776 / 6.943 / 10.058 |
| 64 | Rapid | 19.391 / 21.502 / 22.194 | 20.240 / 24.739 / 54.518 |
| 256 | Slow | 20.588 / 38.215 / 40.892 | 21.523 / 39.068 / 50.105 |
| 256 | Rapid | 84.414 / 98.211 / 101.236 | 84.439 / 98.010 / 185.130 |
| 1024 | Slow | 72.506 / 186.269 / 201.193 | 72.773 / 186.729 / 203.595 |
| 1024 | Rapid | 289.720 / 374.643 / 446.544 | 289.144 / 378.339 / 389.676 |

Prepared knife commits take 0.850–4.708 ms. Observed process peak is 366.89 MiB,
host CPU 38.20% on eight logical processors, sampled every 100 ms. These are
functionally valid strokes but **fail the responsiveness target** across much
of the tested envelope. Blade spacing and per-dab composition remain expensive.
No automatic quality change or performance PASS is claimed.

Palette command trials (128 samples each) report mean / p95 / p99 ms:
deposit 0.550 / 0.467 / 0.517; mix 0.531 / 0.590 / 0.723;
pickup 0.311 / 0.305 / 1.218. The mean can exceed p99 because the maximum cold
sample is outside that percentile. Snapshot serialization is 19.404 ms for
699,252 bytes; restore is 10.578 ms with exact paint and source preservation.
The palette surface is fixed at 256²; document dimensions do not imply a full
24 MP painting operation in these palette timings.

Rack defaults on the dense 24 MP document (97 rapid pressure samples) report:

| Family | Mean / p95 / p99 ms | Prepared commit ms |
|---|---|---|
| Squirrel mop | 3.975 / 6.359 / 7.398 | 0.969 |
| Sable round | 1.581 / 2.585 / 4.395 | 0.874 |
| Sable flat | 3.962 / 5.414 / 8.031 | 0.855 |
| Hog bristle | 5.210 / 7.770 / 9.526 | 0.916 |
| Filbert | 2.317 / 3.431 / 5.453 | 1.008 |
| Fan | 6.003 / 8.725 / 9.014 | 0.938 |
| Rigger | 1.584 / 2.491 / 3.327 | 0.935 |
| Synthetic round | 1.969 / 3.033 / 5.579 | 0.933 |

All rack trials pass exact preview/commit/source/Undo/Redo checks. Studio process
peak across palette, rack and UI probes is 249.20 MiB and host CPU 56.74%.
The native egui update probe uses a 1600×1100 viewport, warmed caches and 128
samples, without GPU raster/presentation or preference-file disk I/O:

| Custom artwork slots | Cached UI mean / p95 / p99 ms | Changed-settings serialization mean / p95 / p99 ms |
|---|---|---|
| 0 | 2.399 / 2.777 / 3.350 | 18.768 / 19.366 / 19.818 |
| 8 | 5.232 / 6.441 / 7.247 | 32.678 / 35.388 / 36.363 |

The expensive settings-change measurements prompted caching the unchanged paint
snapshot. Its separate remeasurement must be reported before claiming improvement.
Neither cached CPU UI updates nor command times establish presentation FPS.

Reproduce with release examples `knife_probe`, `palette_probe`, `rack_probe`
and `studio_ui_probe`; `rack_probe 0` through `rack_probe 7` selects each family.
Raw JSON, per-case binary SHA-256 and sampled process metrics are retained in the
task's `outputs/knife-benchmarks` and `outputs/studio-benchmarks` directories.
The actual native wood/custom-skin screenshots were refreshed after compacting
the rack controls, then visually inspected with every normal control accessible.

### Cached palette snapshot remeasurement

With unchanged paint snapshots cached by revision, the same release native UI
probe measures changed-settings mean / p95 / p99 of **2.171 / 2.548 / 2.682 ms**
with no custom artwork, versus 18.768 / 19.366 / 19.818 ms before; eight custom
artwork slots measure **15.193 / 17.111 / 18.192 ms**, versus
32.678 / 35.388 / 36.363 ms before. Cached ordinary UI updates remain
2.238 / 2.711 / 2.889 ms and 5.231 / 6.500 / 7.338 ms respectively.
The custom-artwork serialization cost remains material. Paint snapshots are
reused only while the palette revision is unchanged; changed paint still saves.
Raw before/after results and separate process/binary hashes are retained.

Final Phase G focused native rerun: **3 PASS, 0 FAIL** after reopening the native
Options menu following artwork selection. It covers pointer selection, keyboard
selection, hover, reduced motion, custom artwork, artist preset replacement,
case-insensitive preset lookup, title-bar drag, palette attachment, tip-editor
opening, source preservation and hostile artwork. The compact-menu failure was
in the test's assumption that an action menu stays open. Source improvements and
original failed logs remain independently recorded. The full subsequent suite
will include this corrected regression; no ignored tests were enabled or hidden.
