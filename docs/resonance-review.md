# Strong-resonance detection review — 0.1.2

Reviewed 10 October 2026 using the supplied Janice singing recordings and
independent synthetic signals. Both source WAV files were moved, byte for byte,
to [`crates/lullaby-dsp/tests/fixtures`](../crates/lullaby-dsp/tests/fixtures/README.md).
Their checksums and the user's description of the EQ are recorded there. A
`.gitignore` exception includes these fixtures while keeping generated WAVs out
of source control.

## Cause

The 0.1.1 local reference was `sum(amplitude²) / sum(amplitude)`. That prevented
spectral valleys from causing excessive reduction, but a dominant peak also
dominated both sums. Raising the resonance raised its own reference almost as
much, so detected excess stopped growing and could decrease.

This was measurable in two independent cases. The +18 dB / 1056 Hz vocal EQ
produced only 0.10 dB of reduction in the 950–1150 Hz measurement band at default
Depth. On a dense harmonic stack, raising a harmonic from +12 to +18 dB reduced
the amount of suppression from about 1.05 to 0.88 dB. Both became failing
regression tests before the correction.

## Change

The detector now compares peaks with an envelope built from other peaks,
excluding the candidate resonance region. Quiet valleys do not determine the
reference, and a large peak cannot raise its own baseline. The excluded region
widens with lower Detail so several partials inside one EQ bump do not mask one
another. The strongest reference peak outside that region provides a conservative
background estimate; references are interpolated on a logarithmic frequency axis.

A harmonic-foundation allowance estimates a plausible fundamental from supported
upper partials with a 1/h amplitude-rolloff allowance. It uses those partials,
not the fundamental's own level: an artificially boosted fundamental remains
detectable. A lone sustained sine has no independent spectral context and is
left alone. Partial startup windows are also excluded from detection until the
engine has a complete analysis window. This does not add reported latency.

All storage is allocated at construction. Range-maximum queries use a segment
tree, keeping reference estimation bounded by O(N log N) rather than scanning
every neighboring peak for every bin. The audio-thread allocation test still
passes. Parameter IDs, preset format, soft-mode Depth mapping and maximum-cut
control are unchanged; saved settings intentionally sound different.

## Vocal measurements

Full-length 48 kHz files, normal quality, minimum phase, default settings apart
from the stated Depth. Output is aligned by the engine's reported latency.
Measurements integrate Hann-windowed spectral power from both channels; they
are independent of the displayed reduction meter. Negative values are cuts.

| Measurement | 0.1.1 | 0.1.2 |
| --- | ---: | ---: |
| Boosted vocal, Depth 6: 950–1150 Hz | −0.10 dB | −5.74 dB |
| Boosted vocal, Depth 12: 950–1150 Hz | −0.56 dB | −11.41 dB |
| Boosted vocal, Depth 6: 150–500 Hz | −0.03 dB | −0.74 dB |
| Original vocal, Depth 6: overall RMS | −0.05 dB | −1.41 dB |

The added EQ is detected much more strongly. The original vocal is still
processed: the new result is not a claim of transparency or an exact reversal
of the EQ. Relative to the original file, the supplied boosted file has about
13 dB more integrated power in 950–1150 Hz; its strongest local difference is
approximately 16 dB near 1055 Hz. The reported +18 dB EQ setting is a control
value, not the expected whole-band energy change. The pair also differs above
3 kHz, so it should not be treated as an exact known linear transfer function.

Reports: [0.1.1 baseline](measurements/vocal-0.1.1.csv) and
[0.1.2 output](measurements/vocal-0.1.2.csv). The baseline was rebuilt from commit
`e031dd3` in an isolated temporary crate using the same measurement helper.

```sh
cargo run -p lullaby-dsp --example vocal_calibration --release --locked -- target/review
```

The example prints measurements for both files, both phases, and Depth
6/12/18/24. The optional directory receives two minimum-phase renders of the
boosted vocal, at Depth 6 and 12. These are float WAVs at the original level,
without normalization or a limiter, so they can be compared in a DAW.

## Regression coverage

Eight new tests check:

- The supplied vocal receives more reduction at the added peak than the original,
  has useful additional suppression at Depth 12, and keeps distant bass changes
  limited. Both phases and normal/low-latency quality are covered.
- Suppression increases as a prominent harmonic is raised through +12/+18/+24/+30 dB.
- Narrow EQ peaks at occupied harmonics of a moving stereo chord are reduced.
  The source includes vibrato, changing harmonic balance, panning, noise and
  percussive bursts; multiple center frequencies and bandwidths are tested.
- Two separated resonances can be reduced together.
- Intermittent ringing is reduced and extra suppression releases after it stops.
- Sustained pure notes stay unchanged, while a boosted fundamental and a dominant
  peak on sparse two-tone material remain detectable.
- Boosting a quiet gap between notes does not force reduction of nearby notes.
- Zero-Depth measurement renders preserve every input sample with latency removed.
  The helper explicitly processes samples before discarding latency; Rust iterator
  `skip()` after `map()` can otherwise skip the stateful processing calls themselves.

Synthetic peaking filters use the equations in the
[W3C Audio EQ Cookbook](https://www.w3.org/TR/audio-eq-cookbook/), independently of
the production detector. They do not claim to reproduce FL Studio's exact EQ.
The existing bell test now checks a proportional increase in local reduction
instead of a fixed 0.75 dB increment; its minimum detection and distant-band
preservation checks remain. The full workspace has 30 passing tests.

## Limits

Natural formants, intended tonal emphasis and unwanted resonance are not perfectly
separable from a single recording. Closely spaced or broad spectral changes can
still be harder to distinguish. Reference selection and the harmonic allowance
are original heuristics, not a reproduction of Soothe's detector.

Resolution remains limited by FFT size. The first three positive-frequency FFT
bins are held at unity because their independent neighborhoods cannot be resolved;
low-latency mode is particularly conservative on bass at high sample rates. The
same quality/phase latency and time-varying minimum-phase limitations documented
in [research](research.md) still apply. These measurements and renders need a
fresh listening check in FL Studio; Windows/Linux native execution and commercial
DAW validation have not been performed in this review.
