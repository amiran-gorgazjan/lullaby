# Singing regression fixtures

These private recordings were supplied on 10 October 2026 and analyzed locally.
They are **not distributed** in the public repository or release archives, and
are excluded from its Git history. They are not covered by the project's MIT
license. The ordinary test suite uses synthetic signals and needs no audio files.

If you have authorized local copies, place them in this directory and run:

```sh
cargo test -p lullaby-dsp --features vocal-fixtures --test resonance
cargo run -p lullaby-dsp --example vocal_calibration --features vocal-fixtures --release -- target/review
```

Both files are 48 kHz stereo, 16-bit PCM WAV, approximately 30.6 seconds long.

- `janice_singing_original.wav`: singing before the added EQ.
- `janice_singing_resonance.wav`: the same material with FL Studio Parametric EQ 2,
  a peaking band at **1056 Hz, +18 dB**, as reported by the user. The bandwidth
  was not supplied. This file has 231 additional sample frames at its end.

SHA-256 checksums:

```text
2f8e1fbfe4ab5646ad6d047f22d23fa19ed1aeef6ed946e15bf6149bb5e8473c  janice_singing_original.wav
3a549694394ff916e8b21a0975df255bfcaa1df3793cbe80c460a060ec9a9a53  janice_singing_resonance.wav
```

Do not add these files to Git or distribute them without appropriate permission.
