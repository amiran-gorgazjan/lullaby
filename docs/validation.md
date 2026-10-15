# Validation

The accepted development baseline has been checked locally on **Apple Silicon
macOS with Rust 1.95.0**. The [private beta workflow](beta-builds.md) also runs
native automated checks on Windows x64, Linux x64, and both macOS architectures.
This record describes their scope; actual DAW compatibility still needs manual
testing of the distributed packages.

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

A Linux CI run that enabled offline DSP instrumentation throughout the plugin
reported 87,425 bytes of retained thread-local allocation balance after 48 editor
creations, above the 64 KiB guard. The preceding default-feature run passed the
same guard. This variability remains a follow-up investigation; it is not evidence
of an identified plugin leak or a resolved memory issue. The beta ships without
that instrumentation. CI retains default plugin memory checks and tests optional
instrumentation separately at the DSP boundary.

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

## Packaged beta checks

Every platform build runs Rust regression tests, packaging/licensing guards and
the native CLAP smoke test. macOS validation checks the universal binaries and
signatures, runs native AU processing/state checks and strict `auval`, then repeats
the native checks on Intel using the exact same ZIP built on Apple Silicon.
The final collection step accepts packages only after these jobs pass. Consult
the successful workflow run and each package's manifest for the actual source,
tool versions, runner image and checksums.

## Remaining release validation

Commercial DAW compatibility, accessibility, full CLAP/VST3 validators and
distribution signing need package-specific verification. The CI matrix provides
build/test automation; a successful workflow is not proof of successful DAW
installation or operation.

Rapid Detail and Detail-tilt automation can exceed the local deadline at 96 kHz,
Ultra quality and 64-sample blocks. Larger buffers provided more headroom in the
local stress checks. Do not claim universally glitch-free automation or infer
performance from allocation tests alone.

See [building and checks](building.md) for commands and
[release readiness](release-readiness.md) for compatibility and beta priorities.
