"""Unmodified, sample-for-sample Delta comparison and diagnostic measurements.

Nothing here aligns, normalizes, resamples, or otherwise improves a candidate.
The primary null is always ``actual_delta - reference_delta`` at the host's
reported latency compensation. Gain and lag estimates are diagnostics only.
All audio arrays have shape (channels, frames). Decibel values for exact zero
are represented by None, with linear amplitudes and explicit status fields.
"""

import math

import numpy as np


SCHEMA_VERSION = 1
ENVELOPE_SECONDS = 0.01


def _db(value):
    value = float(value)
    return 20.0 * math.log10(value) if value > 0.0 else None


def _rms(data):
    # einsum avoids another complete squared-audio allocation for 16-channel cases.
    subscripts = "ij,ij->" if data.ndim == 2 else "i,i->"
    return math.sqrt(float(np.einsum(subscripts, data, data, dtype=np.float64)) / data.size) if data.size else 0.0


def _relative(error, reference):
    if error == 0:
        return None, "silent_error"
    if reference == 0:
        return None, "silent_reference"
    return _db(error / reference), "finite"


def _levels(reference, actual, dry, error):
    er, rr, ar, dr = (_rms(data) for data in (error, reference, actual, dry))
    peak = float(np.max(np.abs(error))) if error.size else 0.0
    relative, status = _relative(er, rr)
    dry_relative, dry_status = _relative(er, dr)
    return dict(exact_match=bool(np.array_equal(reference, actual)),
                error_rms=er, error_rms_dbfs=_db(er),
                error_relative_db=relative, error_relative_status=status,
                error_peak=peak, error_peak_dbfs=_db(peak),
                reference_rms=rr, reference_rms_dbfs=_db(rr),
                actual_rms=ar, actual_rms_dbfs=_db(ar),
                dry_rms=dr, dry_rms_dbfs=_db(dr),
                error_vs_dry_db=dry_relative, error_vs_dry_status=dry_status)


def _region(reference, actual, dry, error, rate, start, end, label):
    return dict(label=label, start_frame=int(start), end_frame=int(end),
                start_seconds=start / rate, end_seconds=end / rate,
                **_levels(reference[:, start:end], actual[:, start:end],
                          dry[:, start:end], error[:, start:end]))


def _envelope(reference, actual, dry, error, rate):
    width = max(1, round(ENVELOPE_SECONDS * rate))
    starts = np.arange(0, error.shape[1], width)
    counts = np.minimum(width, error.shape[1] - starts)
    # Sum channels only after the individual channel measurements are retained.
    def windows(data):
        sums = np.add.reduceat(np.square(data, dtype=np.float64), starts, axis=1)
        return np.sqrt(sums / counts)
    ref, act, source, err = (windows(data) for data in (reference, actual, dry, error))
    rows = []
    for i, start in enumerate(starts):
        error_rms = float(np.sqrt(np.mean(err[:, i] ** 2)))
        reference_rms = float(np.sqrt(np.mean(ref[:, i] ** 2)))
        relative, status = _relative(error_rms, reference_rms)
        channels = []
        for channel in range(reference.shape[0]):
            rr, ar, dr, er = (float(values[channel, i]) for values in (ref, act, source, err))
            channel_relative, channel_status = _relative(er, rr)
            channels.append(dict(channel=channel, reference_rms=rr, actual_rms=ar,
                                 dry_rms=dr, error_rms=er, reference_rms_dbfs=_db(rr),
                                 actual_rms_dbfs=_db(ar), dry_rms_dbfs=_db(dr),
                                 error_rms_dbfs=_db(er), error_relative_db=channel_relative,
                                 error_relative_status=channel_status))
        rows.append(dict(start_frame=int(start), end_frame=int(start + counts[i]),
                         start_seconds=float(start / rate), end_seconds=float((start + counts[i]) / rate),
                         error_rms=error_rms, error_rms_dbfs=_db(error_rms),
                         error_relative_db=relative, error_relative_status=status,
                         reference_rms=reference_rms,
                         actual_rms=float(np.sqrt(np.mean(act[:, i] ** 2))),
                         dry_rms=float(np.sqrt(np.mean(source[:, i] ** 2))),
                         reference_rms_dbfs=_db(reference_rms),
                         actual_rms_dbfs=_db(np.sqrt(np.mean(act[:, i] ** 2))),
                         dry_rms_dbfs=_db(np.sqrt(np.mean(source[:, i] ** 2))),
                         channels=channels,
                         channel_error_rms=err[:, i].tolist(),
                         channel_error_rms_dbfs=[_db(value) for value in err[:, i]]))
    worst_index = int(np.argmax(np.max(err, axis=0)))
    worst_channel = int(np.argmax(err[:, worst_index]))
    worst = {**rows[worst_index], "worst_channel": worst_channel,
             "worst_channel_error_rms_dbfs": _db(err[worst_channel, worst_index])}
    return rows, worst


def _spectrum(reference, actual, dry, error, rate):
    frames = reference.shape[1]
    frequencies = np.fft.rfftfreq(frames, 1.0 / rate)
    # One-sided Parseval weights yield RMS energy, including DC and Nyquist.
    weights = np.full(frequencies.size, 2.0 / frames ** 2)
    weights[0] = 1.0 / frames ** 2
    if frames % 2 == 0:
        weights[-1] = 1.0 / frames ** 2
    edges = [edge for edge in (0, 40, 80, 160, 315, 630, 1250, 2500, 5000, 10000)
             if edge < rate / 2]
    edges.append(rate / 2)
    bands = [dict(low_hz=float(low), high_hz=float(high), channels=[])
             for low, high in zip(edges, edges[1:])]
    error_power = np.zeros(frequencies.size)
    reference_power = np.zeros(frequencies.size)
    actual_power = np.zeros(frequencies.size)
    dry_power = np.zeros(frequencies.size)
    for channel in range(reference.shape[0]):
        spectra = [np.abs(np.fft.rfft(data[channel])) ** 2 * weights
                   for data in (reference, actual, error)]
        reference_power += spectra[0] / reference.shape[0]
        actual_power += spectra[1] / reference.shape[0]
        error_power += spectra[2] / reference.shape[0]
        dry_power += np.abs(np.fft.rfft(dry[channel])) ** 2 * weights / reference.shape[0]
        for index, band in enumerate(bands):
            low = np.searchsorted(frequencies, band["low_hz"], side="left")
            high = (frequencies.size if index == len(bands) - 1 else
                    np.searchsorted(frequencies, band["high_hz"], side="left"))
            rr, ar, er = (math.sqrt(float(np.sum(power[low:high]))) for power in spectra)
            relative, status = _relative(er, rr)
            band["channels"].append(dict(channel=channel, reference_rms=rr,
                                          actual_rms=ar, error_rms=er,
                                          error_rms_dbfs=_db(er), error_relative_db=relative,
                                          error_relative_status=status))
    for band in bands:
        rr, ar, er = (math.sqrt(np.mean([row[key] ** 2 for row in band["channels"]]))
                      for key in ("reference_rms", "actual_rms", "error_rms"))
        relative, status = _relative(er, rr)
        band.update(reference_rms=float(rr), actual_rms=float(ar), error_rms=float(er),
                    error_rms_dbfs=_db(er), error_relative_db=relative,
                    error_relative_status=status,
                    worst_channel=max(band["channels"], key=lambda row: row["error_rms"])["channel"])
    # Distinct peaks are easier to diagnose than neighboring FFT bins. Reference
    # peaks retain matched magnitudes even when the candidate null is silent.
    def peaks_for(power, maximum, candidate_count):
        count = min(power.size, candidate_count)
        candidates = np.argpartition(power, -count)[-count:]
        candidates = candidates[np.argsort(power[candidates])[::-1]]
        peaks = []
        for index in candidates:
            if power[index] <= 0:
                break
            frequency = float(frequencies[index])
            if any(abs(frequency - row["frequency_hz"]) < 20.0 for row in peaks):
                continue
            rr, ar, er, dr = (math.sqrt(values[index])
                              for values in (reference_power, actual_power, error_power, dry_power))
            relative, status = _relative(er, rr)
            peaks.append(dict(frequency_hz=frequency,
                              error_bin_rms_dbfs=_db(er), reference_bin_rms_dbfs=_db(rr),
                              actual_bin_rms_dbfs=_db(ar), dry_bin_rms_dbfs=_db(dr),
                              actual_vs_reference_db=_db(ar / rr) if rr else None,
                              reference_delta_vs_dry_db=_db(rr / dr) if dr else None,
                              actual_delta_vs_dry_db=_db(ar / dr) if dr else None,
                              error_relative_db=relative, error_relative_status=status))
            if len(peaks) == maximum:
                break
        return peaks
    error_peaks = peaks_for(error_power, 12, 512)
    reference_peaks = peaks_for(reference_power, 128, 4096)
    # Keep phase per channel: summing complex spectra across channels could
    # cancel opposite polarities and hide exactly the fault being diagnosed.
    # A second bounded FFT pass avoids retaining every channel's full complex
    # spectrum in memory merely to report these selected frequency bins.
    peak_rows = error_peaks + reference_peaks
    for row in peak_rows:
        row["channels"] = []
    for channel in range(reference.shape[0]):
        reference_fft = np.fft.rfft(reference[channel])
        actual_fft = np.fft.rfft(actual[channel])
        for row in peak_rows:
            index = round(row["frequency_hz"] * frames / rate)
            ref_bin, act_bin = reference_fft[index], actual_fft[index]
            rr = float(abs(ref_bin) * math.sqrt(weights[index]))
            ar = float(abs(act_bin) * math.sqrt(weights[index]))
            defined = rr > 0 and ar > 0
            ref_phase = float(np.angle(ref_bin)) if rr > 0 else None
            act_phase = float(np.angle(act_bin)) if ar > 0 else None
            difference = (math.atan2(math.sin(act_phase - ref_phase),
                                     math.cos(act_phase - ref_phase)) if defined else None)
            row["channels"].append(dict(channel=channel,
                                        reference_bin_rms_dbfs=_db(rr), actual_bin_rms_dbfs=_db(ar),
                                        actual_vs_reference_db=_db(ar / rr) if rr else None,
                                        reference_phase_radians=ref_phase,
                                        actual_phase_radians=act_phase,
                                        phase_defined=defined,
                                        phase_difference_radians=difference,
                                        phase_difference_degrees=math.degrees(difference) if defined else None))
    return bands, error_peaks, reference_peaks


def _gain_and_lag(reference, actual, error, rate, channel):
    ref = reference[channel].astype(np.float64, copy=False)
    act = actual[channel].astype(np.float64, copy=False)
    ref_energy = float(np.dot(ref, ref))
    act_energy = float(np.dot(act, act))
    cross = float(np.dot(ref, act))
    gain = cross / ref_energy if ref_energy else None
    correlation = cross / math.sqrt(ref_energy * act_energy) if ref_energy and act_energy else None
    gain_fit = dict(channel=channel, signed_gain=gain,
                    magnitude_db=_db(abs(gain)) if gain is not None else None,
                    polarity_inverted=bool(gain < 0) if gain is not None else None,
                    cosine_similarity=float(np.clip(correlation, -1, 1)) if correlation is not None else None,
                    residual_rms_dbfs=_db(_rms(act - gain * ref)) if gain is not None else None,
                    used_for_acceptance=False)
    # Estimate a lag on a bounded, energetic excerpt. The original null remains
    # untouched, even when this can explain a timing or reported-latency error.
    length = min(ref.size, max(1, round(2.0 * rate)))
    window = max(1, round(0.01 * rate))
    energies = np.add.reduceat(ref * ref, np.arange(0, ref.size, window))
    loudest = int(np.argmax(energies)) * window
    start = max(0, min(ref.size - length, loudest - length // 2))
    x, y = ref[start:start + length], act[start:start + length]
    lag = None
    if ref_energy and act_energy and not np.array_equal(x, y):
        size = 1 << (2 * length - 1).bit_length()
        correlation_samples = np.fft.irfft(np.fft.rfft(y, size) * np.conj(np.fft.rfft(x, size)), size)
        limit = min(length - 1, round(0.05 * rate))
        lags = np.arange(-limit, limit + 1)
        values = correlation_samples[lags % size]
        # Absolute correlation also diagnoses a simultaneous polarity reversal.
        best = int(np.argmax(np.abs(values)))
        peak_lag = int(lags[best])
        lag = dict(channel=channel, estimated_actual_delay_frames=peak_lag,
                   estimated_actual_delay_seconds=peak_lag / rate,
                   search_limit_frames=int(limit),
                   excerpt_start_frame=int(start), excerpt_frames=int(length),
                   absolute_correlation_peak=float(abs(values[best])),
                   used_for_acceptance=False,
                   caveat="Periodic signals can have ambiguous lag estimates; no alignment is applied.")
    elif ref_energy and act_energy:
        lag = dict(channel=channel, estimated_actual_delay_frames=0,
                   estimated_actual_delay_seconds=0.0, used_for_acceptance=False)
    return gain_fit, lag


def compare_audio(reference_delta, actual_delta, dry, sample_rate, segments=None):
    """Measure the unaltered Delta null and supporting failure diagnostics.

    ``segments`` accepts capture-style dictionaries containing ``label``,
    ``start_seconds`` and ``end_seconds``. Values must refer to the already
    latency-compensated files. A schema/shape/nonfinite error raises ValueError.
    This function supplies measurements only; the runner owns tolerances.
    """
    arrays = [np.asarray(data) for data in (reference_delta, actual_delta, dry)]
    if any(data.ndim != 2 for data in arrays):
        raise ValueError("Expected audio shaped (channels, frames)")
    if any(data.shape != arrays[0].shape for data in arrays[1:]):
        raise ValueError("Reference Delta, actual Delta and dry must have identical shapes")
    if not all(arrays[0].shape):
        raise ValueError("Audio must have at least one channel and one frame")
    if any(not np.issubdtype(data.dtype, np.number) or np.iscomplexobj(data) for data in arrays):
        raise ValueError("Audio must contain real numeric samples")
    if any(not np.isfinite(data).all() for data in arrays):
        raise ValueError("Audio contains NaN or infinite samples")
    if isinstance(sample_rate, bool) or not isinstance(sample_rate, (int, float)) or not math.isfinite(sample_rate) or sample_rate <= 0:
        raise ValueError("Sample rate must be finite and positive")
    reference, actual, source = arrays
    error = actual.astype(np.float64) - reference.astype(np.float64)
    primary = _levels(reference, actual, source, error)
    channels = [dict(channel=index, **_levels(reference[index], actual[index], source[index], error[index]))
                for index in range(reference.shape[0])]
    peak_channel, peak_frame = np.unravel_index(np.argmax(np.abs(error)), error.shape)
    worst_channel = max(channels, key=lambda row: row["error_rms"])["channel"]
    envelope, worst_window = _envelope(reference, actual, source, error, sample_rate)
    bands, frequencies, reference_frequencies = _spectrum(reference, actual, source, error, sample_rate)
    segment_rows = []
    for segment in segments or []:
        start_seconds, end_seconds = (float(segment[key]) for key in ("start_seconds", "end_seconds"))
        if not math.isfinite(start_seconds) or not math.isfinite(end_seconds) or start_seconds < 0 or end_seconds <= start_seconds:
            raise ValueError("Segment bounds must be finite, ordered and nonnegative")
        start, end = round(start_seconds * sample_rate), round(end_seconds * sample_rate)
        if start >= end or end > reference.shape[1]:
            raise ValueError("Segment bounds fall outside the comparison audio")
        segment_rows.append(_region(reference, actual, source, error, sample_rate,
                                    start, end, str(segment.get("label", "segment"))))
    source_peak = float(np.max(np.abs(source)))
    active_frames = np.flatnonzero(np.any(np.abs(source) > max(1e-12, source_peak * 1e-7), axis=0))
    tail = None
    lead = None
    if active_frames.size:
        tail_start = int(active_frames[-1]) + 1
        if tail_start < reference.shape[1]:
            tail = _region(reference, actual, source, error, sample_rate,
                           tail_start, reference.shape[1], "trailing dry silence")
        if active_frames[0] > 0:
            lead = _region(reference, actual, source, error, sample_rate,
                           0, int(active_frames[0]), "leading dry silence")
    gain_fit, lag = _gain_and_lag(reference, actual, error, sample_rate, worst_channel)
    return dict(schema_version=SCHEMA_VERSION, channels_count=reference.shape[0],
                frames=reference.shape[1], sample_rate=sample_rate,
                comparison="actual_delta - reference_delta; no fitted alignment or gain",
                primary=primary, channels=channels, spectrum_bands=bands,
                time_envelope=envelope, envelope_window_seconds=ENVELOPE_SECONDS,
                segments=segment_rows, tail=tail, lead=lead,
                diagnostics=dict(worst_channel=worst_channel, worst_window=worst_window,
                                 peak_sample=dict(channel=int(peak_channel), frame=int(peak_frame),
                                                  seconds=peak_frame / sample_rate,
                                                  signed_error=float(error[peak_channel, peak_frame])),
                                 worst_frequencies=frequencies,
                                 dominant_reference_frequencies=reference_frequencies,
                                 gain_fit=gain_fit, lag_estimate=lag))
