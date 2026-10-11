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

The local reservoir currently retains sampled spatial color but uses a global pickup
and depletion amount. Local pigment exchange and 16/32/64/sparse comparisons remain
NOT IMPLEMENTED. Splatter is functionally VERIFIED with performance PARTIAL. Palette knife, functional palette/rack, AI result comparison,
guidance and provider integration remain NOT IMPLEMENTED in this phase. Advanced
bristle simulation is BLOCKED by the unmet fast-mode performance qualification gate.

Repository contributions are MIT OR Apache-2.0; third-party artwork needs permissive
licenses and attribution. No third-party artwork, model weights or provider SDK is
introduced here. Before publishing advanced palette, custom-bristle or AI components,
report their final code/asset/model licensing and contribution implications and obtain
the owner's explicit release approval. Local development does not authorize uploading
document pixels to an external provider, publishing those components or merging main.
