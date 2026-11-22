# Lullaby patches to nice-plug 0.4.2

This crate comes from the crates.io archive with SHA-256
`3f47be74bed3e8aa17bb055d4ff89af1fb016aa0d2756cd9b6c029a3743676a5`
and upstream revision `91ba75d20c43ca0cd21afa802d0ae8c0511f48ed`.
The archive's normalized manifest, source and tests are retained. The ISC
license comes from the corresponding upstream revision.

- Match and report VST3 speaker arrangements using the same bus ordering and
  channel-to-speaker mapping. Leave the selected layout in place on rejection
  and reject negative bus indices before looking up bus metadata.
- Allow plug-ins to reject unsupported VST3 sample rates during processing
  setup. Check the host's original rate before replacing the last valid setup.
- Keep VST3 controller readback current while leaving processor changes to the
  host's sample-timed parameter events.
