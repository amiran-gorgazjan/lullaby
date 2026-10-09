# Default suppression correction — 0.1.1

The reported muffled default was reproducible in the DSP. Two errors combined:

1. The detector averaged **decibels** across a spectral neighborhood. Deep gaps
   between ordinary harmonics pulled this reference down, making almost every
   harmonic look like an excessive peak. Even a smooth harmonic spectrum was
   heavily suppressed.
2. Soft-mode Depth multiplied this exaggerated excess by `depth * 0.16`. At the
   default of 6, that requested 96% of the excess in dB. Many bins hit the 12 dB
   limit, leaving little useful range for a bell to add emphasis. Vocal Silk's
   lower 8 dB cap then changed broad attenuation more than its 3.2 kHz bell did.

The VST3 host conversion was correct: normalized 0.25 becomes Depth 6. Changing
the host's slider mapping alone would not have repaired the detector.

## Changes

The local reference now uses `sum(amplitude²) / sum(amplitude)`. Empty spectral
gaps contribute very little, so ordinary harmonic spacing no longer causes
strong suppression. Prefix sums use double precision to preserve small treble
contributions after strong bass bins. Detection still works in dB relative to
this reference and keeps its silence/activity gates.

Soft-mode Depth now uses `(2^(depth / 6) - 1) / 4` as the excess multiplier. Depth
0/6/12/24 gives 0/0.25/0.75/3.75. Higher Depth also gradually lowers the prominence
threshold. This provides a gentle region and substantial range above it. The
per-frequency Detail tilt now affects that threshold as well as neighborhood
width. Hard mode keeps its additional absolute-level condition and its existing
Depth multiplier, with the corrected spectral reference and threshold.

The host range, parameter IDs and preset schema are unchanged. Existing saved
settings load, but their response intentionally changes. This is a calibration
of an independent algorithm, not a recovered proprietary transfer curve.

A bell weights sensitivity; it does not disable the rest of the spectrum. This
also matches the documented weighting concept in the
[Soothe2 manual](https://oeksound.com/manuals/soothe2/). The corrected default
leaves ordinary harmonics nearly unchanged while allowing the bell to emphasize
an added resonance. Cut/pass-band shapes can further restrict detection.

## Reproducible measurements

48 kHz, normal quality, minimum phase, settled output aligned by reported
latency. The balanced fixture is a 200 Hz harmonic stack with a smooth declining
amplitude envelope. The resonant fixture adds 12 dB to its 3.2 kHz harmonic.
Results below are measured output changes, not meter readings or DAW listening
judgments. Negative values mean attenuation.

| Measurement | 0.1.0 | 0.1.1 |
| --- | ---: | ---: |
| Balanced stack, Gentle start, overall RMS | −13.85 dB | ≈0.00 dB |
| Added resonance, Gentle start, 3.2 kHz | −15.27 dB | −1.04 dB |
| Added resonance, Vocal Silk, 3.2 kHz | −9.21 dB | −2.12 dB |
| White noise, Gentle start, overall RMS | −1.11 dB | −0.09 dB |

On the resonant fixture, switching from Gentle start to Vocal Silk now adds
about 1.08 dB of reduction at 3.2 kHz while changing the 400/800 Hz harmonics by
less than 0.01 dB. Linear phase gives about 1.00 versus 2.00 dB at 3.2 kHz;
low-latency minimum phase gives about 0.96 versus 1.91 dB.

CSV reports retain the measured RMS and probe amplitudes for both phases and
normal/low-latency quality: [before](measurements/calibration-0.1.0.csv),
[after](measurements/calibration-0.1.1.csv). Generate a current report with:

```sh
cargo run -p lullaby-dsp --example calibration --release --locked
```

Four new regressions failed with the old engine and pass with the correction.
Coverage also varies harmonic spacing and sample rate; the VST3 integration
test checks host conversion and sample output separately. The original sidechain
test now explicitly requests aggressive Depth instead of requiring the gentle
default to remove more than 3 dB from a pair of ordinary tones.

These are controlled synthetic checks. They do not establish transparency on
every instrument, match Soothe's sound, or replace listening in FL Studio. At
aggressive settings, time-varying minimum-phase WOLA can alter the measured
output more than the per-bin reduction limit; Max cut bounds filter-bin gains,
not total waveform attenuation. Comparative listening and broader musical
material remain useful calibration work.
