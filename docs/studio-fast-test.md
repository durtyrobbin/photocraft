# Optimized studio and loaded-paint test

The first desktop test used a development executable with `--safe-gpu`. Its log confirmed Microsoft's CPU software renderer. Normal GPU rendering improved the interface, but the owner still found painting slow. The subsequent optimized executable used the NVIDIA GeForce RTX 2070 SUPER through Vulkan, with the GPU canvas active.

The owner then reported the optimized studio was "infinitely better" and requested local commits. The supplied [manual screenshot](studio-evidence/loaded-paint/owner-manual-test.png) shows **Rigger**, Mix 70%, and Sample All Layers enabled. This is manual acceptance of the optimized studio, not evidence that the owner used only the loaded-paint preset, nor a measured frame-rate or tablet-latency result.

## Try the same configuration

On Windows, from this checkout under `E:\PROJECTS\AIOS`, use the existing E:-based Cargo cache and keep `TEMP`, `TMP`, and `CARGO_TARGET_DIR` on E:. Build the optimized application:

```powershell
cargo build -p photocraft --bin photocraft --release --locked --offline -j2 --config profile.release.lto=false --config profile.release.codegen-units=16
./scripts/launch-studio-test.ps1
```

The launcher uses normal automatic GPU selection and a separate `target/studio-test-profile` containing settings, recovery files, presets, temporary data and paintings. `-CheckOnly` validates prerequisites without launching or writing; `-SafeGpu` explicitly selects the slower fallback. Existing preset libraries are never overwritten. This is an unpublished local test, not a distributable release package.

1. Add an empty layer and select Mixer Brush.
2. Choose **Hog Bristle**, fourth in the rack, or **Fast / Loaded bristle** in the brush picker. In this test profile, these select the same cheaper preset.
3. Choose **Pick up paint** on the Artist Palette and dip into paint, or Alt-click the palette.
4. Start with 64 px, then try 128–256 px. Paint and use Ctrl+Z to undo the stroke.

The preset carries sampled color using a textured tip: Wet/Mix 0%, Load 100%, Sample All Layers off, spacing 30%, automatic reload/clean off. It intentionally gives up continuous wet mixing. Repeated stamp shapes can be visible. Other rack brushes keep their existing wet behavior. To sample canvas colors across layers, enable Sample All Layers for Alt-click sampling, then disable it again before painting.

## Validation and limits

The [benchmark](studio-evidence/loaded-paint/benchmark.json) compares the same fixed textured tip on a 6000 × 4000 RGB8 document with three layers, actual palette deposit/pickup, 96 rapid mouse updates per stroke and three strokes per mode/size. It measures the CPU engine, not screen presentation. Another project was compiling during these measurements; treat the ratios as indicative rather than an uncontended performance gate.

| Diameter | Wet mean per update | Loaded-paint mean | Ratio |
|---|---:|---:|---:|
| 64 px | 4.40 ms | 1.28 ms | 3.44× |
| 256 px | 21.95 ms | 4.25 ms | 5.16× |
| 512 px | 41.43 ms | 10.70 ms | 3.87× |

All 18 strokes changed the document, preserved underlying source layers, matched live preview to commit, and restored exact Undo. The preset passed native store save/load. The 512 px loaded-paint worst p95 was 42.82 ms; large-brush smoothness remains unresolved. The [inspected live capture](studio-evidence/loaded-paint/loaded-paint-live.png) demonstrates the carried-color stamp appearance, using an offscreen CPU canvas rather than qualifying GPU frame time.

Reproduce the preset and engine experiment without modifying application settings:

```powershell
cargo run -p photocraft-engine --example loaded_paint_probe --release --locked --offline -j2 --config profile.release.lto=false --config profile.release.codegen-units=16 -- target/loaded-paint-experiment
```

The output contains `Presets/`, `loaded-bristle.json`, and `benchmark.json`. `--install-only` skips the benchmark and verifies the native preset store roundtrip. The committed preset package contains exactly the measured settings. The captured [launcher record](studio-evidence/loaded-paint/launcher-validation.json) preserves the original local paths and executable checksum as historical evidence.

No engine algorithm changed for this retest. Earlier phase qualification and its limits remain in [the validation ledger](creative-studio-validation.md). Advanced bristles remain blocked; real AI models and physical tablets remain unqualified. Local commit approval does not authorize publication or upstream merging.
