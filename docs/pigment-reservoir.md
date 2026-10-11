# Local pigment reservoir

Status: **PARTIAL — qualification in progress**. This is a bounded two-dimensional
paint model, not a chemical pigment or individual-bristle simulation.

Mixer Brush → Paint Load offers Uniform, 16×16, 32×32 and 64×64. Uniform preserves
existing preset/replay behavior. `brush.mixer.reservoirGrid` stores the choice in
the existing brush definition and preset format; omitted fields default to zero.
Resolution is captured when a stroke starts and never changes silently mid-stroke.

Each cell stores loaded RGBA, a local pickup color and remaining paint. The dab's
rotation, projection, aspect and flips map document pixels into tip-local cells.
The existing custom tip and texture determine contact. Canvas color is accumulated
per cell with coverage/alpha weighting in the document's native channel model.
Local pickup approaches that sampled color according to Wet; Mix blends it with
loaded paint, Flow controls deposition, and Load controls depletion. Cells under
weaker contact drain more slowly; untouched cells retain paint. This preserves
multiple simultaneous colors across the tip instead of averaging all pickup.

The implementation has at most 4,096 cells. It allocates bounded per-dab cell
scratch, plus the same existing brush footprint and transaction working tiles.
Grid changes between strokes resample carried cell state with nearest sampling.
Returning to Uniform discards the local grid and retains the compatibility amount
and pickup fields. A fresh canvas/palette sample or explicit load replaces the
old cells. Clean Pickup drops local pickup; Clean Brush empties the whole brush.
Reload/clean-after-stroke retain their existing user-controlled meanings.

Live display, prepared commit and recorded replay use the same kernel. All-layer
sampling composes the visible document immediately before each dab, including the
current target once. Source layers remain unchanged. Existing masks, transparency
lock and single-stroke Undo apply to the result. Selection coverage gates deposited
pixels; as in the existing mixer, pickup/depletion use brush contact before the
selection blend-back step. The physical paint preview is illustrative RGBA; the
actual deposition converts through the existing native-channel/color pipeline.

This is nearest-cell storage, so low resolutions can show block boundaries. The
64×64 result is a numerical comparison reference, not artistic ground truth.
Sparse zones and independent bristle physics are not implemented. Directional
texture uses the existing tip/texture/pose controls. No performance target passes
until the measured workload and presentation limits have been reported.

Reproduce the CPU matrix and synthetic fidelity images:

```powershell
cargo build -p photocraft-paint -p photocraft-engine -p photocraft-ui-egui --example pigment_probe --example pigment_quality --release --locked --offline -j2 --config profile.release.lto=false --config profile.release.codegen-units=16
target/release/examples/pigment_probe.exe 6000 64 rapid 32
target/release/examples/pigment_quality.exe <output-directory>
```

Repeat the probe for sizes 16/64/256/1024, slow/rapid travel and grids 0/16/32/64.
It uses a dense 24 MP three-layer fixture, 97 pressure samples, and validates exact
preview/commit, source preservation and Undo/Redo. Timings exclude construction;
process metrics include construction/validation. No physical pen/display or GPU
timing is inferred from these CPU measurements. The quality example exports four
target-only images of a synthetic multicolor stroke and reports RGBA RMSE versus
64, red/blue separation and local depletion. No private or third-party art is used.

## Measured local reservoir comparison

Release CPU engine, 6000×4000 dense three-layer fixture, 97 synthetic input samples. Each entry is mean / p95 / p99 update milliseconds. No display FPS or physical pen latency is inferred.

| Size / travel | Uniform | 16×16 | 32×32 | 64×64 |
|---|---|---|---|---|
| 16 / slow | 0.274 / 0.594 / 1.207 | 0.331 / 0.479 / 1.348 | 0.375 / 0.533 / 1.671 | 0.503 / 0.753 / 1.752 |
| 16 / rapid | 0.845 / 1.495 / 2.288 | 1.097 / 1.755 / 2.443 | 1.223 / 1.815 / 2.829 | 1.719 / 2.344 / 3.143 |
| 64 / slow | 0.609 / 0.987 / 1.688 | 0.689 / 1.126 / 1.678 | 0.725 / 1.210 / 1.883 | 0.864 / 1.467 / 2.591 |
| 64 / rapid | 2.272 / 2.777 / 3.946 | 2.599 / 3.185 / 4.434 | 2.716 / 3.333 / 4.486 | 3.194 / 3.835 / 5.141 |
| 256 / slow | 2.656 / 8.895 / 10.061 | 2.932 / 9.367 / 11.006 | 2.978 / 10.328 / 11.134 | 3.012 / 10.246 / 11.393 |
| 256 / rapid | 10.550 / 17.115 / 17.716 | 11.573 / 18.486 / 19.637 | 11.779 / 18.743 / 19.911 | 11.809 / 18.770 / 20.589 |
| 1024 / slow | 8.930 / 105.689 / 117.248 | 10.012 / 118.483 / 130.214 | 10.054 / 119.134 / 130.708 | 10.098 / 119.420 / 129.813 |
| 1024 / rapid | 33.938 / 107.620 / 117.428 | 38.574 / 121.646 / 132.452 | 38.370 / 120.440 / 132.844 | 38.378 / 121.193 / 128.898 |

All 32 trials pass exact preview/commit/source/Undo/Redo. Prepared commits range from 0.618 to 3.117 ms. Observed process peak: 253.26 MiB; host CPU 26.69% across eight logical processors, sampled at 100 ms. These process metrics include fixtures/validation and can miss short peaks.

| Grid | RGBA RMSE versus 64 | Red/blue separation | Remaining load min / max |
|---|---|---|---|
| 0 | 0.127355 | 0.0039 | Global load |
| 16 | 0.022250 | 0.5412 | 0.1766 / 1.0000 |
| 32 | 0.014600 | 0.5549 | 0.1766 / 1.0000 |
| 64 | 0.000000 | 0.5745 | 0.1766 / 1.0000 |

The synthetic fidelity fixture is not an artistic quality score. Higher separation means the red and blue regions remain more distinct in this particular stroke; 64 is a numerical reference, not ground truth. Inspect the target-only PNGs alongside these numbers.


## Native visual verification and current envelope

The four synthetic target-only PNGs were inspected. Uniform pickup averages the
bands toward violet; all three local grids retain separated red and blue regions.
The 16-cell result has visibly coarser transitions. 32 is a useful initial choice
for this fixture; this is not a universal artistic-quality ranking.

Actual 1600×1000 native egui screenshots show the 32×32 control selected, carried
multicolor paint in the load preview, and paint on a separate layer over preserved
source color bands. Pixel comparison of the 960×540 canvas crop finds **60,534
pixels changed before pointer release** and **zero live/committed differences**.
Offscreen rendering uses the Microsoft Basic Render Driver/CPU canvas, not a
qualified physical RTX/tablet/display pipeline.

At 64 px and rapid movement, local-grid p99 is 4.434/4.486/5.141 ms for 16/32/64.
At 256 px rapid movement, all local variants exceed 16.67 ms at p95; at 1024 px
their p99 reaches about 130 ms. Large tips therefore fail the responsiveness
target in this CPU workload. The smaller timings leave room for presentation but
do not prove display frame rates. Optional bristle simulation remains blocked.

## Qualification record

Full native regression: **1,222 engine PASS** (12 ignored), **90 paint PASS**,
**1,847 UI PASS** (10 ignored). This includes carried-cell replay independent of
later tool-load changes and the repaired compact rack menu workflow. UI unit tests
use release opt-level 1; engine/paint and benchmark/screenshot examples use
opt-level 3, LTO off, 16 codegen units, two compiler jobs. The native suite ran with
one test thread to avoid known WARP concurrency issues.

Adversarial command testing, layering, WASM compilation and parity generation pass.
Clippy initially reported an indexed-loop style issue; iterator-only cleanup leaves
arithmetic unchanged. All three kernel tests and all-target Clippy pass after that
cleanup. Original failed and successful logs are retained. Quick workspace performance
qualification remains a final task gate after the next painting optimization.

Functional reservoir behavior is VERIFIED in these synthetic/native checks.
Overall performance qualification is PARTIAL; the documented large-tip cases fail
responsiveness targets, and physical GPU/tablet/cross-platform display tests remain
unavailable. No sparse-zone or advanced-bristle implementation is claimed.
