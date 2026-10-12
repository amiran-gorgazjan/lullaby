# Lullaby

A Rust resonance suppressor with an original DSP engine and interface, informed
by Soothe3's public feature descriptions. Builds **VST3 and CLAP** for **Windows,
macOS and Linux**, plus **AUv2** for **macOS**. This is an initial implementation with substantial functional
overlap, not a sonically verified clone. See [research and differences](docs/research.md).

## Build

Install Rust through rustup. The repository selects Rust **1.95.0** automatically.
Windows needs the MSVC C++ build tools; macOS needs Xcode command-line tools.
On Debian/Ubuntu, install the GUI development libraries first:

```sh
sudo apt-get install libx11-dev libxcursor-dev libx11-xcb-dev libxcb1-dev libxcb-icccm4-dev libxcb-dri2-0-dev libgl1-mesa-dev libxkbcommon-dev libxkbcommon-x11-dev
cargo xtask bundle lullaby --release --locked
```

The last command applies on every platform. Find `Lullaby.vst3` and `Lullaby.clap`
in `target/bundled/`. Copy the **whole bundle**, not just its internal binary,
to a plugin directory scanned by your DAW:

| OS | VST3 | CLAP |
| --- | --- | --- |
| Windows | `C:\Program Files\Common Files\VST3` | `C:\Program Files\Common Files\CLAP` |
| macOS | `~/Library/Audio/Plug-Ins/VST3` | `~/Library/Audio/Plug-Ins/CLAP` |
| Linux | `~/.vst3` | `~/.clap` |

Builds target the current machine's architecture. For a universal macOS bundle,
install both `aarch64-apple-darwin` and `x86_64-apple-darwin` with rustup, then run
`cargo xtask bundle-universal lullaby --release --locked` on macOS.

For **Audio Unit / Logic** builds, also install CMake 3.27+ and Python 3.9+, then run:

```sh
cargo xtask bundle-au lullaby --release --locked
```

This also builds VST3/CLAP and produces a self-contained
`target/bundled/Lullaby.component`. Copy the whole component to
`~/Library/Audio/Plug-Ins/Components/` and restart the host. Add `--universal` for
Intel + Apple Silicon (both Rust targets required), or `--offline` after the
dependencies have been cached. The AU currently exposes stereo main I/O with a
stereo sidechain. See [AU building and validation](docs/audio-unit.md).

## Use

Version **0.1.2** corrects strong resonances escaping detection, following the
0.1.1 fix for excessive default suppression. See the
[vocal-fixture measurements and explanation](docs/resonance-review.md).
Existing presets and sessions load, but their processing will sound different.
After replacing the installed bundle, restart your DAW and check the version
in the editor to ensure it has loaded the update.

1. Start with **Gentle start**. Raise **Depth**, then back it off until reduction
   is subtle. **Detail** controls the local spectral neighborhood used to find peaks.
2. Double-click the graph to add a weighting band; drag its node to change
   frequency and emphasis. Scroll over it to change width. Select a numbered
   band below the graph to edit its shape, focus or enabled state.
   A bell increases or decreases sensitivity locally; a flat curve still permits
   processing elsewhere. Use the cut or pass-band shapes to restrict the range.
3. **Delta** auditions what the filter removes. **Audition Δ** narrows this to
   the selected band. Both are measured before wet trim, mix and output gain.
4. **Link** blends independent and shared detection. **Focus** moves processing
   toward left/mid (−1) or right/side (+1). Use **M/S** for stereo material.
5. Route a DAW sidechain and enable **External SC** to trigger from that input.
   An unconnected or silent external key produces no suppression.
6. **Mix** blends aligned dry audio with trimmed wet audio. **Bypass** retains
   latency and ignores all gain/monitoring controls for comparison.

Use the preset menu for six starting points. **Preset file…** saves/loads JSON at
an absolute path you enter; Save overwrites that file. A/B and Undo/Redo last for
the editor session. Your DAW saves all active sound settings. Quality is locked
across preset changes by default; disable the lock to recall preset quality.

| Quality | FFT / hop | Latency at 48 kHz |
| --- | --- | --- |
| Normal | 2048 / 512 | 2048 samples / 42.67 ms |
| High | 2048 / 256 | 2048 samples / 42.67 ms |
| Ultra | 2048 / 128 | 2048 samples / 42.67 ms |
| Low latency | 512 / 128 | 512 samples / 10.67 ms |

Quality changes require host reactivation. Minimum and linear phase use the
same latency. Low-latency mode trades frequency resolution for responsiveness.
Use moderate depth and slower release if aggressive settings sound unstable.

## Validation

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo xtask bundle lullaby --release --locked
python3 scripts/clap_smoke.py
```

Use `python` on Windows. The smoke test defaults to the release library for the
current OS, or accepts a binary path. It checks native loading, both export
symbols, CLAP parameters/state, and latency-aligned processing with varying
block sizes. DSP tests cover reconstruction, suppression, stereo, sidechain,
monitoring, sanitization, presets and zero allocations during processing.
Calibration tests check gentle defaults on ordinary harmonics and noise, local
preset emphasis and progressive Depth response. A VST3 factory integration test
checks host parameter conversion and audio output against the standalone engine.
The supplied singing recordings live in `crates/lullaby-dsp/tests/fixtures/`.
Further tests cover increasing resonance strength, moving stereo chords,
simultaneous peaks, intermittent ringing, recovery, and sparse musical signals.
To reproduce the vocal report and export two listening renders, run:

```sh
cargo run -p lullaby-dsp --example vocal_calibration --release --locked -- target/review
```

The GitHub Actions workflow builds, tests and uploads bundles on all three OSes.
It must run in your repository before claiming Windows/Linux validation. Local
validation and host-testing limits are recorded in [validation](docs/validation.md).

## Source layout

- `crates/lullaby-dsp`: host-independent engine, weighting curves and tests.
- `src/lib.rs`: plugin ports, lifecycle, latency and nonblocking meters.
- `src/params.rs`: stable host parameter IDs and preset mapping.
- `src/editor.rs`: native egui editor and preset workflow.
- `scripts/clap_smoke.py`: native binary host smoke test.
- `packaging/au`, `scripts/build_au.py`: pinned AUv2 wrapper and macOS build path.
- `scripts/macos_au_smoke.mm`: native AU loading, state and audio regression tests.
- `xtask`: cross-platform bundle creation.

Project code is MIT licensed; dependencies retain their licenses. Lullaby is
unaffiliated with oeksound. Soothe is referenced only to explain the requested
feature research.
