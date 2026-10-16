# CPU optimization round 1 — 2026-10-16

Five agents worked sequentially, sharing hypotheses, rejected experiments and
measurements. Three optimizations passed the predeclared impact, repeatability
and audio-equivalence gates. Rejected implementations and their temporary tests
were reverted. Release optimization settings, quality, latency, parameter laws
and smoothing remain unchanged.

## Agent decisions

Each result below compares against the latest accepted build, using nine
alternating paired trials. Positive percentages mean less processing time.
Acceptance requires >=5% suite or default-preset improvement, a 95% ratio
interval entirely below 0.98, no credible >5% workload regression, and the strict
audio gate described in [the measurement guide](performance.md).

| Agent | Hypothesis | Suite / default gain | Decision |
|---|---|---:|---|
| 1 | Reuse the main FFT for an identical internal detector | 3.43% / 3.62% | Rejected: too small |
| 2 | Skip exactly zero Gaussian-spread sources, with an early dense fallback | 10.78% / 29.14% | Accepted |
| 3 | Share conjugate filter gains; use real exponentiation in linear phase | 6.55% / 7.78% | Accepted |
| 4 | Share post-link envelopes and phase transforms only for exactly equal states | 4.23% / 9.50% | Accepted through default gate |
| 5 | Avoid copying Settings per sample; alternatively use power-of-two ring masks | -0.38% / 0.36%; 0.79% / 0.45% | Both rejected: insignificant |

Agent 2 also rejected independent SIMD reduction lanes and earlier sparse
implementations that slowed dense workloads. The retained implementation skips
only exact zero contributions and retains the original dense traversal. Agent 4
preserves separate detector contexts, channel weights and previous smoothing
states; equal current targets alone never authorize filter reuse.

Accepted implementation commits: `f8ce5f9`, `d245c35`, `ff8f657`.

## Final confirmation

The final comparison uses **19 paired trials and 96,000 measured samples per
case**, in addition to warmup. Both unchanged-build controls were correctly
rejected as improvements. No favorable trials were discarded.

The 16-workload Engine comparison against the 0.2.1 implementation measured
**20.07% less processing time**, with a 95% improvement interval of **17.62–22.29%**.
Default stereo at 48 kHz improved 39.57%; High 41.38%; Ultra 42.67%. No workload
had a credible mean-time regression. Dense Hard-mode gains are smaller than
default Soft-mode gains.

The separate native host loaded the actual released macOS 0.2.1 VST3 and the
new native build through the same VST3 interface. It measured synchronous audio
callbacks with the editor closed:

| Native VST3 workload | CPU reduction | 95% improvement interval |
|---|---:|---:|
| Default, 48 kHz, 128 samples | 40.85% | 39.55–42.04% |
| Default, 48 kHz, 64 samples | 40.56% | 39.19–41.90% |
| Default, 48 kHz, 512 samples | 40.98% | 39.66–42.23% |
| Default, 96 kHz, 128 samples | 33.12% | 31.72–34.40% |
| High, 48 kHz, 128 samples | 41.39% | 40.15–42.55% |
| Ultra, 48 kHz, 128 samples | 42.24% | 41.55–42.98% |
| Low latency, 48 kHz, 64 samples | 26.00% | 23.41–27.75% |
| Equal-weight geometric mean | **38.11%** | **37.35–38.77%** |

Reductions use paired geometric-mean ratios of elapsed processing durations as
a CPU-load proxy, not FL Studio's CPU meter or total system utilization.

Callback tails also improved: default 128-sample VST3 callbacks had median trial
p95 durations of 303.3 → 154.8 microseconds and p99 of 620.9 → 370.9 microseconds.
Neither probe found a credible p99 regression under the same >5%/lower-interval
screen. Percentiles are supportive evidence, not a hard real-time guarantee.

## Correctness and regressions

The longer captures contained 3,081,232 Engine audio/meter values and 1,344,000
native VST3 audio values. Maximum absolute difference was **5.96e-8** in both
probes; RMS differences were 3.89e-9 and 5.47e-9 respectively. The only retained
non-bit-identical change is floating-point roundoff from conjugate gain reuse.
Sparse traversal and linked sharing independently remain bit-identical to their
preceding accepted builds.

Six new Rust regressions compare optimized calculations with the original
arithmetic across sparse/dense ranges, phase changes, audition fades, routing,
channel focus, unequal release histories and reset. Validation passed:

- 68 default Rust workspace/all-target tests, including native VST3 integration,
  existing editor/memory checks and the real-time allocation/free guard.
- 55 DSP tests with diagnostics enabled, overlapping the default DSP tests.
- 101 Python tests, including six benchmark-runner/statistical-gate checks.
- Workspace Clippy with warnings denied, rustfmt and whitespace checks.

## Scope and reproducibility

Local measurement platform: macOS 26.6.2, ARM64; Rust 1.95.0 / LLVM 22.1.2.
Release profile remains Thin LTO, one codegen unit and stripped symbols. The
Engine baseline implementation is `eafcd76`; `668ede8` adds the unchanged
baseline benchmark. Final measured code is `807ccc2`.

The Engine comparison rebuilds the same source benchmark locally. The native
comparison uses the existing CI-built universal release's ARM64 slice against
the new local ARM64 library. Its build provenance differs, so the independent
Engine comparison supplies the controlled source-change measurement.

These results do not establish performance on Intel macOS, Windows or Linux,
or with an open editor. FL Studio still needs a manual check. The automation
workload covers depth, attack, band frequency, phase, linking and routing;
continuous Detail/tilt kernel automation is a separate future investigation.

Raw paired trials, captures, frozen executables, SHA-256 build metadata, callback
analysis and all five agent reports are retained locally under
`.local/performance-round-1/`. Its `memory.md` is the shared investigation log;
`progress.json` records completion. Large machine-specific artifacts are ignored
by Git. See [performance.md](performance.md) to reproduce either probe.
