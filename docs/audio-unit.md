# Audio Unit builds

Lullaby builds a macOS **AUv2 effect** (`Lullaby.component`) using the MIT-licensed
[CLAP wrapper](https://github.com/free-audio/clap-wrapper) and Apple's
[AudioUnitSDK](https://github.com/apple/AudioUnitSDK). The component embeds the
Rust CLAP bundle, including its DSP and egui editor. A separate CLAP installation
is not required. This is AUv2, not an AUv3 app extension or an iOS build.

## Build and install

Requirements: macOS, the repository's Rust toolchain, Xcode command-line tools,
Git, CMake **3.27+**, Python **3.9+**, and the macOS `patch`, `lipo` and `codesign`
tools. For example, `brew install cmake python` supplies the extra build tools.

```sh
cargo xtask bundle-au lullaby --release --locked
```

The first build downloads three open-source repositories to `.cache/au/`.
Tags and exact commits are pinned in `packaging/au/dependencies.json`; altered
checkouts are rejected. Subsequent builds can use `--offline`. Cargo's lockfile
controls the Rust dependencies. No proprietary plugin or SDK is needed.

The command builds VST3/CLAP, compiles the C++ AU wrapper, embeds the CLAP, includes
license notices, signs the nested bundle and component ad hoc, then runs the AU
smoke test before publishing `target/bundled/Lullaby.component`. It does not
install anything. `CARGO_TARGET_DIR` is supported through Cargo metadata.

Copy the **entire** component to `~/Library/Audio/Plug-Ins/Components/`, restart
your DAW, and choose **Lullaby Audio → Lullaby** from its stereo Audio Unit effects.
In Logic, use Plug-in Manager's rescan if it has cached an earlier failed scan.
The current wrapper exposes stereo main input/output and stereo sidechain;
VST3/CLAP also expose mono configurations. AU mono-layout negotiation is not
implemented in this integration.

Native builds match the machine running Cargo. For a universal component:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo xtask bundle-au lullaby --release --locked --universal
```

Both the embedded Rust plugin and wrapper are built for both architectures.
The default deployment target is macOS 11.0. Local testing has covered Apple
Silicon only; Intel/universal output and actual Logic sessions still need host
testing. Ad-hoc signing is suitable for local development; public distribution
requires a Developer ID signing/notarization workflow, which is not included.

## Identity and compatibility

| Field | Value |
| --- | --- |
| AU type | `aufx` |
| AU subtype | `Lulb` |
| Manufacturer | `Llby` / Lullaby Audio |
| Bundle identifier | `audio.lullaby.resonance.au` |
| Version | Read from the Lullaby Cargo package |

Keep these identifiers stable for session recall. AU parameters retain the CLAP
IDs and value/text conversions; continuous values use CLAP's normalized ranges.
The native editor and JSON preset workflow are shared across formats. DAW session
chunks remain format-specific; changing an existing track from VST3 to AU is not
an automatic session migration.

The pinned wrapper needs a small, reviewed compatibility patch, kept in
`packaging/au/clap-wrapper.patch` and applied only to a build-directory source copy:

- Allow state save/load before AU initialization and after uninitialization.
- Flush parameter changes to the inactive CLAP, so pre-activation Quality and
  Depth settings reach DSP initialization and saved state.
- Refresh the AU parameter cache after loading state, propagate state failures,
  and release replaced preset names.

The native smoke test reproduced these lifecycle issues before the patch.
These changes affect the AU adapter; the Rust DSP is shared with the other formats.

## Validation

Every AU build runs `scripts/macos_au_smoke.mm` against the actual signed component.
It registers the component only inside its test process and checks 73 unique
parameters, defaults, pre-activation settings, save/load into a new instance,
stereo reconstruction at 44.1/48/96 kHz, blocks of 1/31/128/257/512 samples,
normal/low-latency modes, sidechain routing, active suppression, and the Cocoa
editor property. This requires macOS AudioComponent service access; a restrictive
process sandbox can prevent registration even when the bundle itself is valid.

To run Apple's full validator against a build without leaving it installed:

```sh
python3 scripts/auval.py
```

This helper temporarily copies the component to the user's Components folder,
runs `auval -v aufx Lulb Llby -strict`, then removes its temporary copy. It refuses
to overwrite an existing installed `Lullaby.component`. For an already installed
copy, run `auval -v aufx Lulb Llby -strict` directly. CI builds and smoke-tests AU
on macOS and also runs `auval` on its temporary runner installation.

See [the validation record](validation.md) for checks actually completed locally.
