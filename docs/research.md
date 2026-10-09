# Research and implementation boundary

Reviewed 9 October 2026. This project uses public descriptions as requirements,
then implements an independently designed algorithm. No Soothe binary, source,
factory preset, image, font, or interface asset was used.

## Published reference

The [Soothe3 manual](https://oeksound.com/manuals/soothe3/) describes adaptive soft
and level-dependent hard processing; depth/detail; frequency-dependent attack,
release and tilts; stereo/mid-side link and focus; maximum reduction; eight
weighting bands; sidechain and audition; phase and quality choices; mix/trims;
presets, A/B and undo. It also documents surround, grouped linking, and a
zero-latency mode at base sample rates. These are functional requirements,
not an algorithm specification.

The [product page](https://oeksound.com/plugins/soothe3/) describes automatic,
time-varying resonance reduction. The
[downloads page](https://oeksound.com/downloads/) lists macOS and Windows builds,
including VST3, AU on macOS, and AAX. Linux is an additional target for Lullaby.

Searches of the official site and public GitHub results for “soothe3 source code”
and “oeksound soothe3 source code” did not locate an official public source
release. The [vendor EULA](https://oeksound.com/eula/) identifies the software as
proprietary. This is a limited public search, not proof that source is unavailable
under a private arrangement. Lullaby makes no binary or sonic compatibility claim.

## Framework choice

[nice-plug](https://codeberg.org/RustAudio/nice-plug) supplies the CLAP/VST3 ABI,
host parameter persistence and editor integration. Its source and examples were
reviewed independently of the proprietary reference. Published crate versions
are pinned through `Cargo.lock`; the principal plugin and GUI adapters are pinned
in `Cargo.toml`. The project uses Rust 1.95 for egui 0.36 compatibility.

## What Lullaby implements

| Area | Implementation |
| --- | --- |
| Detector | Local spectral prominence against an amplitude-weighted reference, relative activity floor; additional absolute-level condition in hard mode |
| Suppression | Per-bin attack/release, bounded attenuation, continuously updated FFT filter |
| Phase | Real spectral gains, or causal real-cepstrum minimum-phase reconstruction per frame |
| Weighting | Eight editable bands: bell, two shelves, two cuts, parallel pass bands, rejection and tilt; per-band focus |
| Dynamics | Low/high tilts for detail, attack and release |
| Routing | Mono/stereo, L/R or M/S, continuously variable power-domain detector linking and focus |
| Monitoring | Latency-aligned mix, pre-mix wet trim, output gain, soft bypass, delta, band delta audition, external key listen |
| Quality | 2048-point FFT with 4/8/16 overlaps; 512-point low-latency mode |
| Interface | Original resizable editor; draggable nodes; reduction and detector traces; 73 host parameters |
| Presets | Six original factory starting points; JSON load/save; host session persistence; session A/B and undo/redo |
| Platforms | Native CLAP and VST3 builds and CI matrix for Windows, macOS and Linux |

## Explicit differences and remaining product work

This is an initial independent implementation, not a verified replacement for
Soothe3. No comparative listening or null tests against the original have been
performed. The detector equations, parameter scales and transfer curves are our
own. Minimum-phase reconstruction is embedded in a time-varying WOLA system;
it is not equivalent to a bank of continuously running analog-style notches.
Linear phase describes the per-frame filter, not a time-invariant overall system.

Version 0.1.1 recalibrates detection and soft-mode Depth after a user reported
excessive broadband reduction. The cause, synthetic measurements and regression
coverage are recorded in [DSP calibration](dsp-calibration.md). These measurements
validate Lullaby against its own intended behavior, not against Soothe.

Low latency is **512 samples**, not zero. Other modes are **2048 samples**; phase
choice does not change reported latency. FFT sizes stay fixed at all sample
rates, so frequency resolution changes with sample rate. Cutoff weighting is
soft, not a brick-wall guarantee. Max cut bounds the spectral filter gains, not
instantaneous waveform peaks. Detector linking uses average powers to avoid
phase cancellation. The displayed input trace is the linked detector spectrum.

Surround/group routing, AU/AAX export, proprietary preset import, preset search
and favorites, multi-node editing and automatic offline quality switching are
not implemented. A/B slots, quality lock and undo history are editor-session
state; the active sound settings are saved by the host. JSON presets contain
sound settings only. Quality changes ask the host to restart processing and can
briefly interrupt audio; a host that defers restarts keeps the previous latency
until reactivation. macOS bundles are locally ad-hoc signed, not notarized.
