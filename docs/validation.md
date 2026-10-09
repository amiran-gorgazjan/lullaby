# Validation record

Local environment: Apple Silicon macOS, Rust 1.95.0. Checked 9 October 2026.

## Passed locally

- `cargo test --workspace --locked`: **22 tests** pass. Sixteen DSP behavior
  tests cover all quality/phase combinations at unity; latency-aligned dry,
  bypass, sidechain listen and delta; resonance attenuation and maximum cut;
  soft-mode level invariance; hard-mode level dependence; sidechain triggering;
  linked stereo and mid/side focus; silence/nonfinite samples; reset; weighting
  masks; and JSON preset round trips/defaults.
- Four calibration regressions cover gentle defaults on harmonic stacks with
  fundamentals of 100/200/400 Hz at 44.1/48/96 kHz, both phases and normal/low-latency
  quality; broadband noise; Vocal Silk's local emphasis; and useful, monotonic
  soft-mode Depth response. All four reproduced the defect before the DSP fix.
- One VST3 integration test exercises the exported factory, component/controller
  and audio processor: normalized Depth values 0/0.25/0.5 convert to 0/6/12, and
  stereo output matches the standalone DSP across varying block sizes. This
  uses the linked Rust library, not a dynamically loaded VST3 bundle.
- The additional real-time test observes the thread-local allocator across
  repeated FFT processing, automation, phase changes, band audition and reset:
  **zero allocations and deallocations** after engine construction.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- `cargo fmt --all --check`: clean.
- `cargo xtask bundle lullaby --release --locked`: Apple Silicon VST3 and CLAP
  bundles generated and ad-hoc signed in `target/bundled/`.
- `python3 scripts/clap_smoke.py`: native CLAP entry/factory/lifecycle, 73 unique
  parameter IDs, host bypass flag, parameter events, partial-read/write state
  streams, state recall, variable blocks (1/31/128/257/512 samples), and normal /
  low-latency reconstruction passed. VST3 factory symbol is present.
- Cocoa test-host editor opened and rendered. Manually verified factory preset
  selection changes the controls and curve, and Undo/Redo restores each state.
  Visual inspection caught and corrected slider overflow and unrounded values.

## Not yet validated

Windows and Linux native builds, Intel macOS/universal bundles, full commercial
DAW validation, full CLAP/VST3 validator suites, accessibility, sustained CPU
benchmarks, and listening comparisons against Soothe3. The CI matrix is provided
but was not dispatched because this workspace has no remote repository.

The Cocoa test host is development-only and is not an audio application.
The Python smoke host exercises the binary ABI; it does not replace host-specific
testing. Its VST3 coverage is limited to the export symbol; the separate Rust
VST3 test covers parameter conversion and audio processing. The user loaded
0.1.0 in FL Studio and reported excessive suppression. Version 0.1.1 addresses
the reproduced DSP defect; a fresh FL Studio listening check remains necessary.
No claims of sonic equivalence, zero-latency operation or surround support are made.
