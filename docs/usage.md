# Using Lullaby

1. Start with **Gentle start**. Raise **Depth**, then back it off until reduction
   is subtle. **Sharpness** controls the local spectral neighborhood used to find peaks.
2. Double-click the graph to add a weighting band; drag its node to change
   frequency and emphasis. Scroll over it to change width. Select a numbered
   band in **Spectral bands** in the footer to edit its shape, focus or enabled state.
   Enabled bands keep their weighting curves and nodes visible in the graph.
   A bell increases or decreases sensitivity locally; a flat curve still permits
   processing elsewhere. Use the cut or pass-band shapes to restrict the range.
3. **Delta** auditions what the filter removes. **Audition Δ** narrows this to
   the selected band. Both are measured before wet trim, mix and output gain.
4. Open **Processing** for mode, channels, quality, phase, wet trim, sidechain,
   frequency tilts and continuous stereo link/focus. **Link** blends independent and shared detection. **Focus** moves processing
   toward left/mid (−1) or right/side (+1). Use **M/S** for stereo material.
5. Route a DAW sidechain and enable **External SC** to trigger from that input.
   An unconnected or silent external key produces no suppression.
6. **Mix** blends aligned dry audio with trimmed wet audio. **Bypass** retains
   latency and ignores all gain/monitoring controls for comparison.

Drag knobs vertically or horizontally; hold Shift for fine changes. Scroll or use
arrow keys on a focused knob, double-click (or Ctrl/Cmd-click) to reset, and click
a value to type it. Enter or clicking away applies an entry; Escape cancels it.
Sharpness and Mix entries use percentages, matching their readouts.

The graph shows the detector spectrum, average suppression and an estimated
filtered spectrum. The latter reflects displayed reduction, including Mix, but
excludes wet trim, output gain, bypass and monitoring. With external sidechain
enabled it represents the key signal. It is not a measurement of final output.
The graph's top-right menu changes its display floor. Peak reduction is shown
numerically; right-click the graph to show separate channel reduction curves.

Use the preset menu or arrows for six starting points. The save icon opens
**Presets & history**, which saves/loads JSON at
an absolute path you enter; Save overwrites that file. A/B and Undo/Redo last for
the editor session. Your DAW saves all active sound settings. Quality is locked
across preset changes by default; disable the lock to recall preset quality.

| Quality | FFT / hop | Latency at 48 kHz |
| --- | --- | --- |
| Normal | 2048 / 512 | 2048 samples / 42.67 ms |
| High | 2048 / 256 | 2048 samples / 42.67 ms |
| Ultra | 2048 / 128 | 2048 samples / 42.67 ms |
| Low latency | 512 / 128 | 512 samples / 10.67 ms |

Quality changes require host reactivation. Minimum and linear phase use the
same latency. Low-latency mode trades frequency resolution for responsiveness.
Use moderate depth and slower release if aggressive settings sound unstable.

