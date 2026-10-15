# Commercial beta readiness

The beta should preserve Lullaby's current sound while testing installation,
session recall and operation in real hosts. Keep versioned packages and complete
third-party notices with each release. See [licensing](licensing.md),
[building](building.md) and [validation](validation.md).

## Existing session compatibility

Plugin identifiers and host parameter IDs are stable, but parameter ranges and
processing changed during development. Loading an older session is not a promise
of identical sound. Review existing sessions and re-save user presets before the
beta release; do not silently replace an installed version on another producer's
machine.

Current parameter units are:

| Control | Current meaning |
| --- | --- |
| Depth, Detail, Attack, Release | Unitless 0–10 |
| Band depth | Unitless −10–10 |
| Detail, Attack and Release frequency tilts | 0.1×–10×, neutral 1× |
| Maximum cut | 0–24; the 24 endpoint means Off |

Older normalized host state maps to the current ranges. In particular, Depth
previously spanned 0–24, band weight spanned −24–24 dB, timing controls displayed
milliseconds, and Maximum cut previously used a finite 0–36 dB range. Stable
legacy IDs such as `attack_ms`, `release_ms` and `band_depth_db` remain addressable,
but their names do not describe the current units.

Direct JSON presets use displayed DSP units. Older `attack_ms`/`release_ms` JSON
fields are not converted to the current unitless controls. Old signed tilt values
are not converted to multipliers, and out-of-range values sanitize to current
limits. Compatibility needs manual review rather than a sound-preserving
automatic conversion claim.

## Beta checks still needed

- Test Windows x64, Linux x64 and both macOS architectures in actual DAWs.
- Verify installation, host rescanning, session recall, automation, multiple
  instances and editor behavior at different display scales.
- Review CPU headroom at small buffers. Rapid Detail or Detail-tilt automation
  at 96 kHz, Ultra quality and 64-sample blocks has exceeded the local host
  deadline in stress measurements. Allocation-free processing does not ensure
  that every buffer deadline is met.
- Complete distribution signing, notarization where applicable, downloadable
  package verification and a clear rollback path.
- Complete the separate legal and asset-rights review before selling builds.
