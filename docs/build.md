# Building Lullaby

Run these commands from the repository root.

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
stereo sidechain. See [AU building and validation](audio-unit.md).

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
Tests cover increasing resonance strength, moving stereo chords,
simultaneous peaks, intermittent ringing, recovery, and sparse musical signals.
The private singing recordings are excluded from the repository and its history.
If you have authorized copies, place them in `crates/lullaby-dsp/tests/fixtures/`
and enable the optional vocal regression and report:

```sh
cargo test -p lullaby-dsp --features vocal-fixtures --test resonance --locked
cargo run -p lullaby-dsp --example vocal_calibration --features vocal-fixtures --release --locked -- target/review
```

The GitHub Actions workflow builds, tests and packages all three OSes. Pushing a
version tag publishes release downloads only after every platform succeeds.
Archives include installation instructions, license notices and SHA-256 checksums.
Local validation and host-testing limits are recorded in [validation](validation.md).

## Source layout

- `crates/lullaby-dsp`: host-independent engine, weighting curves and tests.
- `src/lib.rs`: plugin ports, lifecycle, latency and nonblocking meters.
- `src/params.rs`: stable host parameter IDs and preset mapping.
- `src/editor.rs`: native egui editor composition and preset/history workflow.
- `src/editor/`: shared theme, rotary controls, switches, spectrum and panels.
- `assets/fonts/`: embedded fonts and their SIL Open Font License notices.
- `scripts/clap_smoke.py`: native binary host smoke test.
- `packaging/au`, `scripts/build_au.py`: pinned AUv2 wrapper and macOS build path.
- `scripts/macos_au_smoke.mm`: native AU loading, state and audio regression tests.
- `xtask`: cross-platform bundle creation.

Project code is MIT licensed; dependencies retain their licenses. Bundle resources
include the font notices; keep `FONT-LICENSES.txt` with single-file CLAP distributions.
Official release archives also contain `THIRD-PARTY-NOTICES.txt` for the locked
Rust dependencies and additional AU wrapper notices inside the component.
Lullaby is
unaffiliated with oeksound. Soothe is referenced only to explain the requested
feature research.
