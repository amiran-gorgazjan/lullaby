# Offline detector and filter diagnostics

`lullaby-dsp` provides a small research Interface at the Engine seam behind the
`research-trace` feature. Default DSP and plugin builds contain none of its state
buffers, capture writes or accessors. This adds observability, not a new DSP model
or an audio parity improvement. No reference audio or measured response tables
are included.

Call `Engine::diagnostic_frame()` between sample calls to borrow the latest
completed analysis frame. Access does not allocate or consume `take_frame_ready`.
Read as often as useful, or skip frames entirely; the engine retains only one
frame. The borrow prevents mutation while the views are in use. `None` means no
hop has completed since construction/reset. Reset invalidates the previous trace;
the first subsequent frame replaces every exposed intermediate before it can be
read. No historical frames or unbounded queue are stored.

All positive-frequency bins include DC and Nyquist. The power prefix has one
additional entry and uses the recorded detector peak for normalization. The Interface returns actual
detector magnitude/reference and actual normalized-power prefix, prominence/activity, requested cut before linking,
linked cut, spread cut, weighted/capped target, smoothed reduction, and complex
natural-log filter. Five absent intermediate arrays per channel are preallocated
at construction; existing buffers are borrowed for the other stages. Activity is
unitless; cuts are positive dB. Peak and activity floor are the actual scalar
values used by the detector. No reconstruction is performed by the Interface.

In Soft mode, spread cut includes the Depth contour mapping after the maximum
kernel pass. Requested and linked cut remain the detector's positive dB requests.
For Depth below 10, the spread is mapped through reciprocal-gain excess before
band weighting, Max cut and final smoothing. The added dB cut is retained fully
through effective Detail 5, then blended continuously to zero at effective
Detail 10, using the existing per-bin Detail and low/high tilts. Depth 10 and
Hard retain their existing spread. The trace does not expose a separate array for the unmapped
maximum. This is an approximation of the Depth-dependent contour, not a claim
about the reference plugin's internal coordinate.

The complex logarithm is captured after minimum-phase cepstral conversion (or
before exponentiation for linear phase). `exp(log_filter)` reconstructs the
**designed** complex bin filter. WOLA synthesis, windowing, overlap and temporal
changes mean this is not a measured input/output transfer. The exporter labels
its reconstructed exponential separately from the actual retained logarithm.

`input_end_sample` is the exclusive count since reset at the completed hop;
`frame_index` starts at 1. The frame covers `[end - fft_size, end)`, with negative
startup indices representing zero padding. `valid_input_samples` distinguishes
startup from a full window. These are analysis input times; reported engine
latency has not been removed. Settings are copied at the actual frame, so changes
between hops do not relabel old data. Channel 0/1 means L/R, or M=(L+R)/2 and
S=(L-R)/2 in stereo M/S. Mono uses the left detector for both channels; channel
weighting still applies separately. Settings identify internal versus external
key routing. Bin `k` means `k * sample_rate / fft_size` Hz.

## Reusable bounded renderer/exporter

Run the example with a JSON job. Input, key and output are interleaved **stereo
f32 little-endian PCM**. Native reference-host probe files are planar and must
be converted losslessly first (example below). Mono
engines still use two values per input frame. Input files must have exactly the
requested number of frames. Output contains raw engine samples; append silence
to inputs when an output tail is needed, and remove only declared latency when
comparing aligned audio.

```sh
CARGO_HOME="$PWD/.cache/cargo" cargo run --release --locked --offline \
  -p lullaby-dsp --features research-trace --example diagnostic_trace -- job.json
```

```json
{
  "sample_rate": 48000,
  "input": "./main.pcm",
  "sidechain_input": "./key.pcm",
  "output": "./rendered.pcm",
  "trace_csv": "./trace.csv",
  "frames": 480000,
  "settings": {
    "depth": 10.0,
    "attack": 0.0,
    "release": 0.0,
    "max_cut_db": 24.0,
    "external_sidechain": true
  },
  "frequencies_hz": [120.0, 6000.0],
  "max_records": 10000,
  "every_n_frames": 1,
  "start_sample": 48000,
  "end_sample": 336000
}
```

Settings use the existing DSP serialization, units and defaults; 24.0 Max cut is
Off. Optional `automation` is an increasing list of `{ "frame": 1234,
"settings": { ... } }` entries. Each settings object is a complete replacement
with existing defaults for omitted fields, applied before that input sample.
This calls the standalone Engine directly; sample-exact settings replacement
can differ from the plugin wrapper's parameter smoothing/host segmentation.
Do not claim a native plugin replay from an automated Engine render alone.
Quality must stay fixed; changing it requires another engine/job. Optional
`mono` defaults to false.

Both output paths must be fresh: existing files (including hardlinks/symlinks
to inputs) are rejected without truncation.

For the existing planar host probes, this conversion only rearranges f32 bytes;
it performs no numeric conversion, resampling or normalization. Preserve both
source and converted hashes with the job. The inverse must match the source bytes.

```python
import numpy as np
from pathlib import Path
source = Path("input.pcm").read_bytes()
planar = np.frombuffer(source, dtype="<u4").reshape(2, -1)
interleaved = planar.T.copy().tobytes()
assert np.frombuffer(interleaved, dtype="<u4").reshape(-1, 2).T.copy().tobytes() == source
Path("main-interleaved.pcm").write_bytes(interleaved)
# Repeat for the key. This offline conversion loads the file into memory;
# the Rust renderer itself streams with bounded memory.
```

CSV rows contain frame/sample/channel attribution, requested frequency and actual
nearest FFT bin frequency, and every exposed stage. Frequency selection never
interpolates detector state. Row order is frame, channel, then requested frequency;
a row cap can end partway through a frame. `start_sample`/`end_sample` are inclusive
filters on frame-end times, and the stride is counted from frame 1. Hitting the
record cap stops export but continues the full bounded audio render. `max_records`
0 renders audio without trace rows. A JSON summary reports records, sample rate,
initial settings, event count and declared latency. Keep the original job beside
its results for the complete automation/settings provenance.

Bounds: at most 600 seconds of audio, 128 selected frequencies, 1,000,000 CSV rows,
100,000 automation events and 4 MiB job JSON. Only one PCM sample and one analysis
frame are processed at a time; input/output streams and manifest storage live
outside processing. File I/O and complex exponential reconstruction occur in the
offline caller, after `process_sample` returns. Enabled processing and borrowed
access remain allocation-free. This is an offline research tool, not a realtime
callback exporter or a DAW performance guarantee.
