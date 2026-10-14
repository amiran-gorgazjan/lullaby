"""Deterministic, explicitly parameterized synthetic measurement signals."""
from __future__ import annotations
import numpy as np

VERSION = 2
SEED = 20261013
LEAD = 0.5
ACTIVE = 6.0
TAIL = 1.5


def db(value):
    return 10 ** (value / 20)


def rms_scale(x, level=-18):
    return x * (db(level) / max(float(np.sqrt(np.mean(x * x))), 1e-30))


def harmonic(t, fundamental, rate, kind="saw"):
    result = np.zeros_like(t)
    for k in range(1, min(256, int(rate * .45 / fundamental)) + 1):
        if kind in ("square", "triangle") and k % 2 == 0:
            continue
        amplitude = 1 / k
        if kind == "triangle":
            amplitude = (-1) ** ((k - 1) // 2) / k ** 2
        result += amplitude * np.sin(2 * np.pi * fundamental * k * t)
    return rms_scale(result)


def noise(rate, samples, pink=False, seed=SEED):
    x = np.random.Generator(np.random.PCG64(seed)).standard_normal(samples)
    if pink:
        f = np.fft.rfftfreq(samples, 1 / rate)
        weights = np.zeros_like(f)
        weights[f > 0] = 1 / np.sqrt(np.maximum(f[f > 0], 20))
        x = np.fft.irfft(np.fft.rfft(x) * weights, n=samples)
    return rms_scale(x)


def peak_eq(x, rate, frequency, q, gain):
    # Precisely defined, zero-phase FFT magnitude of an RBJ peaking EQ.
    # This fixture generator is not intended to imitate FL Studio's EQ.
    a = 10 ** (gain / 40)
    w = 2 * np.pi * frequency / rate
    alpha = np.sin(w) / (2 * q)
    b = np.array([1 + alpha * a, -2 * np.cos(w), 1 - alpha * a])
    den = np.array([1 + alpha / a, -2 * np.cos(w), 1 - alpha / a])
    z = np.exp(-2j * np.pi * np.fft.rfftfreq(x.size))
    response = np.abs((b[0] + b[1] * z + b[2] * z * z) / (den[0] + den[1] * z + den[2] * z * z))
    return np.fft.irfft(np.fft.rfft(x) * response, n=x.size)


def specs():
    rows = []
    def add(name, kind, **params):
        rows.append(dict(id=name, kind=kind, **params))
    for f in [100, 250, 500, 1056, 3200, 8000, 1056.37, 1054.6875]:
        add(f"sine_{str(f).replace('.', 'p')}hz", "sine", frequency=f)
    for kind in ["saw", "square", "triangle"]:
        for f in [110, 220, 440]:
            add(f"{kind}_{f}hz", kind, frequency=f)
    add("silence", "silence")
    add("impulses", "impulses")
    add("white_noise", "noise", pink=False)
    add("pink_noise", "noise", pink=True)
    for f, q, gain in [(1056, 8, 6), (1056, 8, 12), (1056, 8, 18), (1056, 1, 18), (250, 8, 18), (3200, 8, 18), (8000, 8, 18)]:
        add(f"pink_peak_{f}hz_q{q}_plus{gain}db", "eq_noise", frequency=f, q=q, gain=gain)
    add("multi_resonance", "multi_resonance", peaks=[[180, 6, 10], [1056, 10, 18], [3200, 7, 12], [8000, 5, 8]])
    add("two_close_tones", "close_tones", frequencies=[1000, 1056, 1112], amplitudes=[.15, 1, .15])
    add("equal_multitone", "close_tones", frequencies=[100, 250, 500, 1056, 3200, 8000], amplitudes=[1] * 6)
    add("tone_bursts", "bursts", frequencies=[120, 1056, 6000], period=1.0, on=.25)
    add("level_steps", "level_steps", levels_dbfs=[-48, -36, -24, -12], frequency=1056)
    add("moving_resonance", "moving", low=300, high=6000)
    add("vibrato_resonance", "vibrato", frequency=1056, vibrato_hz=5, vibrato_cents=35)
    add("synthetic_vocal", "vocal", fundamental=165, formants=[[650, 3, 14], [1056, 8, 18], [2800, 5, 12]])
    add("synthetic_chord", "chord", fundamentals=[130.8128, 164.8138, 195.9977])
    for kind in ["asymmetric", "left_only", "mid_side", "antiphase", "uncorrelated"]:
        add(f"stereo_{kind}", "stereo", stereo_kind=kind)
    add("sidechain_key", "key", frequency=3200, period=1.0, on=.35)
    add("sidechain_main", "sidechain_main")
    for level in [-48, -36, -24, -18, -12, -6]:
        add(f"level_{abs(level)}dbfs", "calibration_level", level_dbfs=level)
    add("surround_identification", "surround", channels=16)
    add("resonant_harmonics", "resonant_harmonics", fundamental=132,
        extra_partials=[[1056, .25], [3200, .12], [8000, .06]])
    add("shape_sequence", "resonant_harmonics", fundamental=132,
        extra_partials=[[1056, .25], [3200, .12], [8000, .06]], active_seconds=16)
    return rows


def generate(spec, rate):
    active_seconds = spec.get("active_seconds", ACTIVE)
    n = round(active_seconds * rate)
    t = np.arange(n) / rate
    kind = spec["kind"]
    mono = None
    if kind == "silence":
        mono = np.zeros(n)
    elif kind == "sine":
        mono = rms_scale(np.sin(2 * np.pi * spec["frequency"] * t))
    elif kind in ("saw", "square", "triangle"):
        mono = harmonic(t, spec["frequency"], rate, kind)
    elif kind == "resonant_harmonics":
        mono = harmonic(t, spec["fundamental"], rate) * .25
        for frequency, amplitude in spec["extra_partials"]:
            mono += amplitude * np.sin(2 * np.pi * frequency * t)
        mono = rms_scale(mono)
    elif kind == "noise":
        mono = noise(rate, n, spec["pink"])
    elif kind == "eq_noise":
        mono = rms_scale(peak_eq(noise(rate, n, True), rate, spec["frequency"], spec["q"], spec["gain"]))
    elif kind == "multi_resonance":
        mono = noise(rate, n, True)
        for f, q, gain in spec["peaks"]:
            mono = peak_eq(mono, rate, f, q, gain)
        mono = rms_scale(mono)
    elif kind == "close_tones":
        mono = rms_scale(sum(a * np.sin(2 * np.pi * f * t) for f, a in zip(spec["frequencies"], spec["amplitudes"])))
    elif kind in ("bursts", "key"):
        freqs = spec.get("frequencies", [spec.get("frequency", 3200)])
        gate = (t % spec["period"] < spec["on"]).astype(float)
        mono = rms_scale(sum(np.sin(2 * np.pi * f * t) for f in freqs)) * gate
        mono += noise(rate, n) * db(-24)
    elif kind == "level_steps":
        mono = np.sin(2 * np.pi * spec["frequency"] * t) * np.sqrt(2)
        levels = np.array(spec["levels_dbfs"])
        mono *= 10 ** (levels[np.minimum((t / (ACTIVE / len(levels))).astype(int), len(levels) - 1)] / 20)
    elif kind == "moving":
        f = spec["low"] * (spec["high"] / spec["low"]) ** (t / ACTIVE)
        mono = .14 * np.sin(2 * np.pi * np.cumsum(f) / rate) + noise(rate, n, True) * .15
    elif kind == "vibrato":
        f = spec["frequency"] * 2 ** (spec["vibrato_cents"] * np.sin(2 * np.pi * spec["vibrato_hz"] * t) / 1200)
        mono = .14 * np.sin(2 * np.pi * np.cumsum(f) / rate) + noise(rate, n, True) * .15
    elif kind == "vocal":
        mono = harmonic(t, spec["fundamental"], rate)
        for f, q, gain in spec["formants"]:
            mono = peak_eq(mono, rate, f, q, gain)
        mono = rms_scale(mono) * (.2 + .8 * np.sin(np.pi * (t % 1.2) / 1.2) ** 2)
        mono += noise(rate, n) * .12 * ((t % 1.2) > .95)
    elif kind == "chord":
        mono = rms_scale(sum(harmonic(t, f, rate) for f in spec["fundamentals"]))
    elif kind == "impulses":
        mono = np.zeros(n)
        for second in [.5, 1.5, 3, 4.5]:
            mono[round(second * rate)] = .5
    elif kind == "calibration_level":
        mono = np.sin(2 * np.pi * 1056 * t) + noise(rate, n, True) * .1
        mono = rms_scale(mono, spec["level_dbfs"])
    elif kind == "sidechain_main":
        mono = rms_scale(sum(np.sin(2 * np.pi * f * t) for f in [250, 1056, 3200, 6000]) + noise(rate, n, True))
    elif kind == "stereo":
        background = noise(rate, n, True) * .15
        a = .16 * np.sin(2 * np.pi * 1056 * t) + background
        b = .08 * np.sin(2 * np.pi * 3200 * t) + background
        st = spec["stereo_kind"]
        if st == "asymmetric":
            data = np.stack([a, b])
        elif st == "left_only":
            data = np.stack([a, np.zeros(n)])
        elif st == "antiphase":
            data = np.stack([a, -a])
        elif st == "mid_side":
            data = np.stack([a + b, a - b])
        else:
            data = np.stack([a, .16 * np.sin(2 * np.pi * 3200 * t) + noise(rate, n, True, SEED + 1) * .15])
    elif kind == "surround":
        freqs = [1056, 3200, 1800, 80, 720, 2400, 4800, 6000, 1400, 3600, 4000, 8000, 5200, 9000, 7000, 10000]
        data = np.stack([(.08 + .05 * np.sin(2 * np.pi * (.5 + c / 10) * t) ** 2) * np.sin(2 * np.pi * freqs[c] * t)
                         + noise(rate, n, True, SEED + c) * .1 for c in range(spec["channels"])])
    else:
        raise ValueError(kind)
    if mono is not None:
        data = np.stack([mono, mono])
    # Only the boundaries are faded: interior burst/level transitions stay exact.
    fade = max(1, round(.005 * rate))
    window = np.sin(np.linspace(0, np.pi / 2, fade)) ** 2
    data[:, :fade] *= window
    data[:, -fade:] *= window[::-1]
    data = np.pad(data, ((0, 0), (round(LEAD * rate), round(TAIL * rate))))
    if np.max(np.abs(data)) >= 1:
        raise ValueError(f"Synthetic input clips: {spec['id']} at {rate}")
    return data.astype('<f4')
