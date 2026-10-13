# Install Lullaby

Download the archive for your operating system from
[GitHub Releases](https://github.com/amiran-gorgazjan/lullaby/releases), then
extract it. Lullaby is free, MIT-licensed software. There is no activation.

Copy the **entire** `Lullaby.vst3` bundle and/or `Lullaby.clap` to a plugin folder:

| OS | VST3 | CLAP |
| --- | --- | --- |
| Windows (64-bit) | `C:\Program Files\Common Files\VST3` | `C:\Program Files\Common Files\CLAP` |
| macOS (Intel and Apple Silicon) | `~/Library/Audio/Plug-Ins/VST3` | `~/Library/Audio/Plug-Ins/CLAP` |
| Linux (x86-64, Ubuntu 24.04 or compatible) | `~/.vst3` | `~/.clap` |

For Audio Units on macOS, copy `Lullaby.component` to
`~/Library/Audio/Plug-Ins/Components/`. Create the folders if necessary, restart
your DAW, and rescan its plugins. Keep `LICENSE`, `FONT-LICENSES.txt` and
`THIRD-PARTY-NOTICES.txt` with your installed or redistributed plugins.

macOS bundles are ad-hoc signed, not Apple notarized. If macOS blocks a download
you trust, use its Privacy & Security settings to allow it. Your DAW and plugin
must run with matching architectures; the macOS archive contains both Intel
and Apple Silicon binaries.

Start with **Gentle start**, and adjust Depth while listening. See the
[usage guide](usage.md) for controls and
latency. Version 0.1.2 is an early release; please report host-specific issues
with your OS, DAW version and plugin format on
[GitHub Issues](https://github.com/amiran-gorgazjan/lullaby/issues).
