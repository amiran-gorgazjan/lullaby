# Building and installing Lullaby

Install Rust through rustup. The repository selects Rust **1.95.0** automatically.
Windows needs the MSVC C++ build tools; macOS needs Xcode command-line tools.
On Debian/Ubuntu, install the GUI development libraries:

```sh
sudo apt-get install libx11-dev libxcursor-dev libx11-xcb-dev libxcb1-dev libxcb-icccm4-dev libxcb-dri2-0-dev libgl1-mesa-dev libxkbcommon-dev libxkbcommon-x11-dev
```

Build VST3 and CLAP on the target operating system:

```sh
cargo xtask bundle lullaby --release --locked
```

Find `Lullaby.vst3` and `Lullaby.clap` in `target/bundled/`. Copy the **whole bundle**,
including its resources, to a plugin directory scanned by the DAW:

| OS | VST3 | CLAP |
| --- | --- | --- |
| Windows | `C:\Program Files\Common Files\VST3` | `C:\Program Files\Common Files\CLAP` |
| macOS | `~/Library/Audio/Plug-Ins/VST3` | `~/Library/Audio/Plug-Ins/CLAP` |
| Linux | `~/.vst3` | `~/.clap` |

Keep `FONT-LICENSES.txt` and `LULLABY-LICENSES.txt` with distributions, including
single-file CLAP builds. Release archives must also include the complete applicable
third-party notices; generation of those notices remains a release task.
See [licensing](licensing.md).

Builds target the current machine's architecture. For universal macOS VST3/CLAP
bundles, install both Rust targets, then run:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo xtask bundle-universal lullaby --release --locked
```

For macOS Audio Unit builds, also install CMake **3.27+** and Python **3.9+**:

```sh
cargo xtask bundle-au lullaby --release --locked
```

This also builds VST3/CLAP and produces a self-contained
`target/bundled/Lullaby.component`. Copy the whole component to
`~/Library/Audio/Plug-Ins/Components/`, restart the DAW and rescan if necessary.
Add `--universal` for Intel + Apple Silicon, or `--offline` once dependencies
have been cached. See [Audio Unit building and validation](audio-unit.md).

After replacing an installed plugin, restart the DAW and check the version under
**Processing** to make sure it loaded the intended build. Keep the previous
installer or bundle available when reviewing an existing session.

## Development checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo xtask bundle lullaby --release --locked
python3 scripts/clap_smoke.py
```

Use `python` on Windows. The smoke test defaults to the current OS's release
library and also accepts a binary path. It exercises the actual CLAP binary;
the Rust VST3 integration tests exercise the exported factory and processing API.
These checks complement manual DAW testing; see [validation](validation.md).
