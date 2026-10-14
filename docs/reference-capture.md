# Compact Soothe3 reference capture

The local corpus contains **at most 25 diagnostic case folders** under
`.local/soothe3-capture/cases/`. Open their WAVs directly or import them into a DAW.
There is no review UI. These are actual renders of the normally licensed Soothe3
VST3, not Lullaby output or an approximation of Soothe3.

Start with `04_hard_depth_10` for a strong audible contrast, then compare
`01_soft_depth_3` with `02_soft_depth_10` on their identical dry input.

## Files in every case

- `dry.wav`: the original, deterministic synthetic input.
- `wet.wav`: Soothe3 output, aligned to the dry input using reported latency.
- `raw.wav`: uncompensated output with original host latency and flush.
- `delta.wav`: a real render of Soothe3 with its **Delta control enabled**,
  identical settings and automation, and the same latency compensation.
- `settings.json`: initial controls, automation timeline, final effective
  controls and labeled segments, where applicable.
- `capture.json`: sample rate, channels, recipe, latency, exact host parameters,
  per-channel RMS/peaks/envelopes, non-silence measurements and SHA-256 hashes.
- `plugin.state`, `delta.state`: exact states returned through the VST3 API.
  Their `*-state.json` counterparts expose the readable snapshot data.

All audio is IEEE float32 WAV with preserved levels. No normalization, listening
gain, dither, limiting or resampling is applied. Most files are 8 seconds: 0.5 s
silence, 6 s active signal and 1.5 s tail. The eight-shape sequence is 18 seconds.

The mixed-gain case additionally supplies `difference.wav`, the mathematical
`dry - wet` signal including gain changes. This is explicitly distinct from
Soothe3's Delta monitoring when mix and trim controls are active.

The surround case includes stereo previews selecting two channels with strong
Delta energy. They preserve those channels' levels and identify their indices.
The full multichannel WAVs remain the actual routing references.

## The 25 cases

| Cases | Diagnostic purpose |
| --- | --- |
| 01–04 | Soft/hard at Depth **3 and 10**, identical input with prominent harmonics |
| 05–06 | Isolated sine and band-limited saw |
| 07–08 | Detail 1 versus 10 on closely spaced peaks |
| 09–10 | Fast versus slow attack/release on bursts |
| 11 | Contrasting low/high detail, attack and release tilts |
| 12 | Eight band shapes, held for two seconds each in a labeled sequence |
| 13 | Bell weighting around 3.2 kHz |
| 14–16 | L/R and M/S linking, global and per-band stereo focus |
| 17 | Independent gated sidechain key |
| 18 | Maximum cut changing from 3 to 12 dB |
| 19 | Mix, wet trim and output trim routing |
| 20–23 | Linear phase/high quality, low latency, ultra/96 kHz, normal/44.1 kHz |
| 24 | 9.1.6 channel groups, linking, band assignment and channel bypass |
| 25 | Depth automation from 3 to 10 |

This is a compact diagnostic set, not an exhaustive parameter grid. Combined
cases and labeled transitions characterize several controls in one input.
UI workflows such as A/B and undo require separate interaction tests. The
[native Delta parity suite](reference-parity.md) compares all 25 cases with
Lullaby, with supporting diagnostics. DSP changes remain a later phase.

## Validation

Generation and validation fail if any retained case has a native Delta RMS
below **−65 dBFS**, or below **−40 dB relative to its dry input**. These thresholds
reject effectively silent captures; they are not Lullaby similarity tolerances.
Every band-shape segment must also have non-silent native Delta.

At unity trims and 100% mix, validation checks `wet + delta ≈ dry` to a maximum
sample error of `3e-6`. The mixed-gain case separately verifies its mathematical
difference. This guards against bypassed processing, missing Delta, wrong
alignment and mislabeled files. It also verifies requested settings, WAV
dimensions/rates, finite samples and checksums. `validation.json` records results.

Optional cross-host validation compares six static cases with Spotify
Pedalboard hosting the same Soothe3 VST3 and repeats one render. Those are
checks of the existing cases; they do not create additional case folders.

## Reproduce

The native host currently runs on macOS, uses public VST3 interfaces and requires
normal plugin license validation. It uses no audio device or DAW project.

```sh
python3 -m venv target/reference-venv
target/reference-venv/bin/python -m pip install -r scripts/reference/requirements.txt
cargo build --locked --manifest-path tools/reference-host/Cargo.toml --target-dir target/reference-host
target/reference-venv/bin/python -u scripts/reference_capture.py --output .local/soothe3-capture-new --cross-host
target/reference-venv/bin/python -u scripts/reference_capture.py --validate --cross-host
```

Choose an empty output directory. The generator refuses to mix a new compact
run with stale cases. `--limit` permits 1–25 cases; no option expands the set.
`--inspect` only exports parameter metadata. The environment manifest and
source snapshots preserve the exact plugin, host, generator and dependency
versions used. Global plugin preferences are recorded without changing them.

The audio remains local and ignored by Git. Private vocal recordings and
proprietary plugin binaries are not included in the corpus.
