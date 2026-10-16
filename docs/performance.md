# Measuring processing CPU

See [the first optimization round](performance-results.md) for measured results
and accepted/rejected hypotheses.

Use an optimized build on an otherwise idle machine. The probe measures the
actual `Engine` used by the plugin, including block settings updates, analysis,
filtering, metering and sample output. It excludes construction, FFT planning,
signal generation, disk writes and the GUI/host wrapper. This is a local DSP
comparison, not a prediction of a DAW's CPU meter or a cross-platform guarantee.

```sh
cargo build --release --locked -p lullaby-dsp --example performance
# Preserve the executable before changing the implementation:
cp target/release/examples/performance /tmp/lullaby-performance-before
# Rebuild after a change, then run the paired acceptance gate:
python3 scripts/performance.py \
  --baseline /tmp/lullaby-performance-before \
  --candidate target/release/examples/performance \
  --output target/performance-comparison --pairs 9
python3 -m unittest discover -s scripts -p 'test_performance.py'
```

The deterministic suite covers 16 workloads: default stereo at 48/96 kHz,
mono, strong hard-mode suppression with focused bands, independent/partial/full
stereo linking, external sidechain, M/S, both phase modes, band audition, all
quality modes, wide/narrow Detail, and changing parameters/routing. Each trial
warms up 8,192 samples and measures 24,000 samples in 128-sample host blocks.
Increase `--frames` for confirmation or long-tail investigations.

The runner alternates before/after process order and stores every trial. It
reports per-case ns/sample, elapsed time as a percentage of the audio duration,
and callback p50/p95/p99 durations. The equal-weight suite score is the geometric
mean of paired case ratios. A deterministic bootstrap over paired log ratios
provides 95% confidence intervals; these characterize repeatability on this
machine, not all machines. Independent measurements on other CPUs are useful.

Acceptance requires at least a 5% improvement in either the suite or default
48 kHz preset, with the entire confidence interval at least 2% better. A case
with a credible regression above 5% rejects the candidate. Borderline or noisy
results require a fresh run; do not select favorable trials. Normal unit tests
do not fail on wall-clock timings. Run comparisons sequentially, with no builds
or other benchmarks competing for CPU.

The same suite captures stereo outputs and final meters before timing. The
audio gate requires maximum absolute difference <= 0.00002 and RMS difference
<= 0.000001, including meter values, and rejects non-finite or truncated output.
This permits only small floating-point differences; it does not authorize
changed detector laws, reduced quality, skipped automation, latency changes,
approximate math or different audio behavior. Existing behavior, allocation,
diagnostic and VST3 integration tests remain required.

## Native VST3 wrapper comparison

The separate `vst3_performance` example loads a trusted native Lullaby shared
library through its exported VST3 factory. It measures only synchronous audio
callbacks, including the wrapper, with 64/128/512-sample blocks, 48/96 kHz and
all quality modes. Both builds use the same host executable. Input preparation,
activation, output capture and disk writes are outside timed callbacks. Reported
percentiles refer to real VST3 callbacks. The editor stays closed; FL Studio and
open-editor behavior still need a manual test.

```sh
cargo build --release --locked --example vst3_performance
cargo build --release --locked -p lullaby
python3 scripts/performance.py \
  --baseline target/release/examples/vst3_performance \
  --candidate target/release/examples/vst3_performance \
  --baseline-plugin /path/to/previous/Lullaby.vst3/Contents/MacOS/Lullaby \
  --candidate-plugin target/release/liblullaby.dylib \
  --output target/vst3-performance --pairs 9 --frames 96000
```

Pass the actual native library inside the platform bundle, not the bundle
directory. On Windows use its `.vst3` library; on Linux use its `.so` library.
Only load trusted builds: loading a native plugin executes its code. The same
paired statistical and audio gate applies, with seven VST3 workloads. Captures
contain stereo output, while the Engine probe additionally includes meters.
