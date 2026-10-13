# Validation record

Local environment: Apple Silicon macOS, Rust 1.95.0. Checked 12 October 2026.

## Passed locally

- `cargo test --workspace --locked --offline`: **38 tests** pass. Sixteen DSP behavior
  tests cover all quality/phase combinations at unity; latency-aligned dry,
  bypass, sidechain listen and delta; resonance attenuation and maximum cut;
  soft-mode level invariance; hard-mode level dependence; sidechain triggering;
  linked stereo and mid/side focus; silence/nonfinite samples; reset; weighting
  masks; and JSON preset round trips/defaults.
- Four calibration regressions cover gentle defaults on harmonic stacks with
  fundamentals of 100/200/400 Hz at 44.1/48/96 kHz, both phases and normal/low-latency
  quality; broadband noise; Vocal Silk's local emphasis; and useful, monotonic
  soft-mode Depth response. All four reproduced the defect before the DSP fix.
- Eight additional tests cover the actual vocal pair at both phases and normal /
  low-latency quality; increasing resonance strength; evolving stereo chords with
  vibrato, noise and percussion; two simultaneous resonances; intermittent ringing
  and release recovery; sustained pure notes, a boosted fundamental and sparse
  two-tone context; a boosted quiet spectral gap; and measurement-render alignment.
  The vocal and increasing-strength regressions failed on 0.1.1 before the fix.
- One VST3 integration test exercises the exported factory, component/controller
  and audio processor: normalized Depth values 0/0.25/0.5 convert to 0/6/12, and
  stereo output matches the standalone DSP across varying block sizes. This
  uses the linked Rust library, not a dynamically loaded VST3 bundle.
- The additional real-time test observes the thread-local allocator across
  repeated FFT processing, automation, phase changes, band audition and reset:
  **zero allocations and deallocations** after engine construction.
- Five editor regressions cover rotary gestures at reference and minimum sizes,
  including an entire drag between redraws and motion coalesced with release;
  balanced host automation gestures; fine adjustment and typed milliseconds;
  percentage entry and Escape cancellation; graph band creation; factory preset
  history; and rendering the existing panels at minimum size.
- Three memory regressions run in the normal test suite. Test-only allocator
  accounting measures live Rust heap bytes (including reallocations and frees),
  rather than RSS or cumulative allocation traffic:
  - After warmup, 1,200 idle editor frames, including tessellation, retain **0
    bytes**. Another 1,200 frames with changing meters, parameter automation,
    resizing, all panels and channel curves retain **10,496 bytes**, below the
    256 KiB budget. The texture count remains exactly two and undo history stays
    at or below 64 entries.
  - Creating, rendering and dropping 48 headless editors retains **0 bytes**
    after warmup. Weak references verify that each editor's mock host, parameters
    and meters are released. The regression budget is 64 KiB.
  - Creating, activating, processing and destroying 48 VST3 instances across all
    four quality modes, three sample rates and varying block sizes retains **0
    bytes** after warmup. The 256 KiB budget counts all Rust worker threads;
    wrapper tests serialize to avoid unrelated concurrent allocations.
  These checks did not reproduce a leak. They cover Rust-owned memory, not
  native window or GPU resources, and do not establish the cause of the reported
  FL Studio memory spike. The user could no longer reproduce that spike. A short
  native CLAP preview check also showed steady resident memory; it is not an
  automated GPU-memory regression. No production behavior was changed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- `cargo fmt --all --check`: clean.
- `cargo xtask bundle lullaby --release --locked`: Apple Silicon VST3 and CLAP
  bundles generated and ad-hoc signed in `target/bundled/`.
- `python3 scripts/clap_smoke.py`: native CLAP entry/factory/lifecycle, 73 unique
  parameter IDs, host bypass flag, parameter events, partial-read/write state
  streams, state recall, variable blocks (1/31/128/257/512 samples), and normal /
  low-latency reconstruction passed. VST3 factory symbol is present.
- Redesigned editor inspected in the native Cocoa CLAP test host at 1152×768 and
  960×640. Verified Depth dragging, typed Attack entry, Vocal Silk's 3.2 kHz band,
  preset navigation, Undo/Redo and spectrum floor selection. The provided vocal
  resonance recording drives the live detector display through real processing,
  silently, without opening an audio device. Processing settings and all six
  frequency tilts remain reachable by scrolling at minimum size. The shared
  editor modules change presentation without modifying DSP or host parameters.
  Enabled band nodes remain visible with the panel closed. Peak reduction is
  retained numerically and separate channel reduction curves are available in
  the graph context menu; both channel weighting curves remain available.
- Both embedded fonts render in the native editor. Full OFL notices are packaged
  in VST3/CLAP resources and AU third-party notices, with a companion license file
  for single-file CLAP distributions. Resource changes are ad-hoc signed again.
- `cargo xtask bundle-au lullaby --release --locked --offline`: Apple Silicon
  **AUv2** component built with its CLAP embedded, version 0.1.2, and ad-hoc
  signatures verified recursively. AU identity is `aufx / Lulb / Llby`.
- The native AU smoke host loads the actual component without installation.
  Checks pass for 73 unique parameters, defaults, inactive parameter changes,
  state recall into a fresh instance, stereo reconstruction at 44.1/48/96 kHz,
  varying blocks, both latency modes, connected sidechain monitoring, active
  resonance suppression, and the Cocoa editor property. State recall and
  pre-activation Quality tests exposed wrapper defects fixed by the local patch.
- `python3 scripts/auval.py`: **AU VALIDATION SUCCEEDED** with Apple's
  `auval -v aufx Lulb Llby -strict`. The helper waited for component discovery and
  removed its temporary installation afterward. Three nonfatal wrapper warnings
  remain: the recommended Tail Time property is unsupported, AU preset-name
  metadata is not retained, and the generic factory exposes a MIDI selector for
  an `aufx` effect. Parameter/DSP state recall passes the separate smoke test.
  See [AU build details](audio-unit.md).

## Not yet validated

Windows and Linux native builds, Intel macOS/universal bundles, full commercial
DAW validation (including Logic and AU editor interaction), full CLAP/VST3 validator suites, accessibility, sustained CPU
benchmarks, and listening comparisons against Soothe3. The CI matrix is provided
but was not dispatched because this workspace has no remote repository.

The Cocoa test host is development-only and is not an audio application.
The Python smoke host exercises the binary ABI; it does not replace host-specific
testing. Its VST3 coverage is limited to the export symbol; the separate Rust
VST3 test covers parameter conversion and audio processing. The user loaded
0.1.0 in FL Studio and reported excessive suppression. Version 0.1.1 addressed
that defect, but real recordings exposed poor detection of stronger peaks.
Version 0.1.2 addresses the reproduced detection problem; a fresh FL Studio
listening check remains necessary. The supplied recordings were analyzed locally,
and the rendered examples have not been judged through a listening comparison.
No claims of sonic equivalence, zero-latency operation or surround support are made.
