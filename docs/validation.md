# Validation

The accepted development baseline has been checked locally on **Apple Silicon
macOS with Rust 1.95.0**. This record describes the scope of those checks;
commercial beta packages still need testing on their target operating systems
and in actual DAWs.

## Automated regression coverage

The Rust suite covers:

- Latency-aligned reconstruction across quality and phase settings, dry/wet mix,
  bypass, Delta monitoring, trims and output gain.
- Suppression of synthetic resonances, harmonic and noisy inputs, moving stereo
  chords, simultaneous peaks, intermittent ringing and recovery.
- Depth, Detail, Attack, Release, frequency tilts, weighting bands and maximum
  cut semantics, including finite serialized state for the Off endpoint.
- Stereo linking/focus, mid/side operation, external sidechain and key monitoring.
- Silence, nonfinite inputs, reset, parameter sanitization and preset round trips.
- Exported VST3 factory, normalized parameter conversion, variable processing
  blocks, instance teardown and comparison with the standalone engine.
- Editor gestures, typed values, band editing, presets, history and proportional
  resizing.

Allocation tests measure zero audio-thread allocations/deallocations after engine
construction. Memory regressions exercise sustained headless editor rendering,
repeated editor creation/destruction and repeated VST3 instance lifecycles with
bounded live Rust heap usage. They do not measure all native window/GPU resources
or establish a cause for a host-specific memory spike.

Test inputs are generated in code. No external recordings are required or included.

The feature-gated [offline diagnostic interface](diagnostics.md) also has tests
for stage snapshots, bounded storage and reset behavior. It is absent from normal
plugin builds.

## Native macOS checks

Local VST3, CLAP and AUv2 bundles have been built and verified with strict recursive
code-signature checks. These are development ad-hoc signatures, not completed
commercial distribution signing or notarization.

`scripts/clap_smoke.py` loads the actual CLAP binary and exercises entry/factory
lifecycle, 73 unique parameter IDs, bypass/events, partial state streams, fresh
instance recall, variable block sizes and both latency modes. It also checks the
VST3 export symbol; it does not replace a complete dynamic VST3 validator.

Every AU build runs the native smoke host against the actual component. It checks
parameter/state lifecycle, 44.1/48/96 kHz stereo reconstruction, variable blocks,
both latency modes, sidechain monitoring, active suppression and the Cocoa editor
property. Apple's strict `auval` has passed locally. Existing nonfatal wrapper
warnings concern an unsupported recommended Tail Time property, preset-name
metadata and a generic MIDI selector on an `aufx` effect. See
[Audio Unit validation](audio-unit.md).

The editor has also been inspected in the development Cocoa CLAP host at its
default and minimum sizes. Headless tests and editor-property checks do not
replace sustained native editor interaction in a DAW.

## Remaining release validation

Windows and Linux execution, Intel/universal macOS packages, commercial DAW
compatibility, accessibility, full CLAP/VST3 validators and distribution signing
need package-specific verification. The CI matrix provides build/test automation;
a successful workflow is not proof of successful DAW installation or operation.

Rapid Detail and Detail-tilt automation can exceed the local deadline at 96 kHz,
Ultra quality and 64-sample blocks. Larger buffers provided more headroom in the
local stress checks. Do not claim universally glitch-free automation or infer
performance from allocation tests alone.

See [building and checks](building.md) for commands and
[release readiness](release-readiness.md) for compatibility and beta priorities.
