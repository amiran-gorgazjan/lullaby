"""Explicit, auditable Soothe3-to-Lullaby candidate control mapping.

This module does not assert that similarly named controls have the same DSP.
Uncalibrated conversions produce *exploratory* Lullaby renders and a mandatory
capability failure.  Callers must fail their support check whenever unsupported
is nonempty; waveform similarity must never turn that check into a pass.

The source controls are displayed physical values, not VST normalized values.
The only source normalized values accepted are the explicitly captured focus
ratios.  Duplicate Lullaby band titles are addressed by native-host ``#id``
selectors, rather than accidentally changing the first band eight times.
"""
from __future__ import annotations

import math
import re
from collections import defaultdict
from copy import deepcopy


SOURCE_DEFAULTS = {
    "Depth": "5", "Mode": "soft", "Detail": "5", "Attack": "5", "Release": "3",
    "Mix": "100", "Wet trim": "0", "Out trim": "0", "Delta": "off", "Bypass": "off",
    "Stereo link": "100", "Stereo mode": "L/R", "Stereo focus": {"raw": .5},
    "Sidechain": "off", "Sidechain listen": "off", "Max cut": "off",
    "Quality": "normal", "Low latency": "off", "Linear phase": "off",
    **{f"{control} tilt {side}": "1" for control in ("Detail", "Attack", "Release")
       for side in ("low", "high")},
    **{f"Band {i} enabled": "off" for i in range(1, 9)},
    **{f"Band {i} used": "off" for i in range(1, 9)},
    **{f"Band {i} focus": {"raw": .5} for i in range(1, 9)},
}

# Lullaby's order is from BandParams::snapshot, not Soothe3's order.
BAND_SHAPES = {"bell": 0, "low shelf": 1, "high shelf": 2, "low cut": 3,
               "high cut": 4, "bandpass": 5, "band reject": 6, "tilt": 7}


def _number(value):
    """Read displayed scalars, rejecting normalized source values and NaNs."""
    if isinstance(value, (dict, bool)):
        raise ValueError("A displayed physical scalar is required, not normalized data")
    token = re.fullmatch(r"\s*([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)\s*(?:dB|%|x|Hz|ms)?\s*",
                         str(value), re.IGNORECASE)
    if token is None:
        raise ValueError(f"Invalid displayed scalar: {value!r}")
    result = float(token.group(1))
    if not math.isfinite(result):
        raise ValueError("Non-finite control value")
    return result


def _bounded(value, minimum, maximum):
    result = _number(value)
    if not minimum <= result <= maximum:
        raise ValueError(f"Value {result} outside [{minimum}, {maximum}]")
    return result


def _on(value):
    if isinstance(value, bool):
        return value
    text = str(value).strip().lower()
    if text not in ("on", "off"):
        raise ValueError(f"Invalid switch: {value!r}")
    return text == "on"


def _focus(value):
    if isinstance(value, dict) and set(value) == {"raw"}:
        ratio = value["raw"]
        if isinstance(ratio, bool) or not isinstance(ratio, (int, float)) or not 0 <= ratio <= 1:
            raise ValueError("Focus ratio must be finite and in [0, 1]")
        return 2 * float(ratio) - 1
    if str(value).strip().lower() == "even":
        return 0.0
    match = re.fullmatch(r"\s*(\d+(?:\.\d+)?)\s*/\s*(\d+(?:\.\d+)?)\s*", str(value))
    if match:
        left, right = map(float, match.groups())
        if 0 <= left <= 100 and 0 <= right <= 100 and abs(left + right - 100) < 1e-6:
            return (right - left) / 100
    raise ValueError(f"Invalid captured focus ratio: {value!r}")


def plan_case(record, lullaby_parameter_info):
    """Plan native Lullaby wet/Delta rendering and mandatory support diagnostics.

    ``record`` is capture.json (or a case from reference.plan). Returned
    ``expected_parameters`` describe final physical ``plain`` values, for
    controller readback checks. The Delta render must override both its Delta
    setting and its expected Delta value to one. ``renderable`` is false for
    unsupported channel layouts or a missing real Delta control; never downmix
    a reference or synthesize a candidate Delta to work around these failures.
    """
    parameters = list(lullaby_parameter_info)
    by_name = defaultdict(list)
    for parameter in parameters:
        by_name[parameter["name"]].append(parameter)
    source = deepcopy(SOURCE_DEFAULTS)
    source.update(deepcopy(record.get("settings", {})))
    settings, automation, expected, unsupported, notes = {}, [], {}, [], []
    renderable = record.get("channels", 2) in (1, 2)

    def issue(name, value, code, reason, target=None, fallback=None, frame=None):
        item = {"source_parameter": name, "source_value": deepcopy(value),
                "code": code, "reason": reason}
        if target is not None:
            item["target_parameter"] = target
        if fallback is not None:
            item["exploratory_value"] = fallback
        if frame is not None:
            item["frame"] = frame
        unsupported.append(item)

    def parameter(title, band=None):
        matches = by_name.get(title, [])
        if band is None:
            if len(matches) != 1:
                raise ValueError(f"Expected one Lullaby {title!r} parameter; found {len(matches)}")
            return title, matches[0]
        if len(matches) != 8:
            raise ValueError(f"Expected eight Lullaby band {title!r} parameters; found {len(matches)}")
        selected = matches[band - 1]
        return f"#{selected['id']}", selected

    def emit(name, value, title, plain, *, band=None, text=None, frame=None):
        selector, info = parameter(title, band)
        # Public inspection provides physical range endpoints. Reject clamping.
        mapping = info.get("mapping", [])
        if mapping and all("plain" in p for p in (mapping[0], mapping[-1])):
            endpoints = [float(mapping[0]["plain"]), float(mapping[-1]["plain"])]
            if not min(endpoints) - 1e-6 <= plain <= max(endpoints) + 1e-6:
                raise ValueError(f"{title}={plain} outside inspected Lullaby range {endpoints}")
        if frame is not None and not info.get("flags", 1) & 1:
            raise ValueError(f"Lullaby parameter {title} does not permit automation")
        target_value = text if text is not None else format(plain, ".12g")
        expected[selector] = {"name": title, "plain": plain,
                              "plain_tolerance": max(1e-5, abs(plain) * 2e-6)}
        if frame is None:
            settings[selector] = target_value
        else:
            automation.append({"frame": frame, "parameter": selector, "value": target_value})
        return selector, target_value

    def convert(name, value, frame=None):
        def put(title, plain, *, band=None, text=None):
            return emit(name, value, title, plain, band=band, text=text, frame=frame)

        def uncalibrated(title, plain, reason, *, band=None):
            selector, candidate = put(title, plain, band=band)
            issue(name, value, "uncalibrated_scale", reason, selector, candidate, frame)

        if name == "Depth":
            # Intentionally physical text; source raw=.3 must not become Lullaby raw=.3 (7.2).
            uncalibrated("Depth", _bounded(value, 0, 10),
                         "Soothe3 Depth 0..10 and Lullaby Depth 0..24 have no measured equivalence; "
                         "candidate uses the same displayed number, not normalized position.")
        elif name == "Detail":
            uncalibrated("Detail", _bounded(value, 0, 10) / 10,
                         "Exploratory range mapping v/10; detector selectivity laws are not calibrated.")
        elif name in ("Attack", "Release"):
            bounds = (.1, 500) if name == "Attack" else (5, 2000)
            candidate = bounds[0] * (bounds[1] / bounds[0]) ** (_bounded(value, 0, 10) / 10)
            uncalibrated(name, candidate,
                         "Soothe3 uses a unitless 0..10 control; Lullaby uses milliseconds. "
                         "Candidate uses a fixed logarithmic interpolation across Lullaby's range; "
                         "this is not a measured timing equivalence.")
        elif name in ("Mix", "Stereo link"):
            put(name, _bounded(value, 0, 100) / 100)
        elif name in ("Wet trim", "Out trim"):
            put("Output" if name == "Out trim" else name, _bounded(value, -24, 24))
        elif name == "Max cut":
            if str(value).strip().lower() == "off":
                selector, candidate = put(name, 36)
                issue(name, value, "missing_unlimited_cut",
                      "Soothe3's unlimited reduction cannot be represented by Lullaby's finite 36 dB ceiling.",
                      selector, candidate, frame)
            else:
                put(name, _bounded(value, 0, 36))
        elif name in ("Bypass", "Delta", "Sidechain", "Sidechain listen"):
            target = {"Sidechain": "External sidechain", "Sidechain listen": "Listen to sidechain"}.get(name, name)
            enabled = _on(value)
            put(target, float(enabled), text="On" if enabled else "Off")
        elif name == "Mode":
            options = {"soft": (0, "Soft"), "hard": (1, "Hard")}
            ordinal, text = options[str(value).strip().lower()]
            put(name, ordinal, text=text)
        elif name == "Stereo mode":
            options = {"l/r": (0, "L/R"), "m/s": (1, "M/S")}
            ordinal, text = options[str(value).strip().lower()]
            put("Channels", ordinal, text=text)
        elif name == "Stereo focus":
            put(name, _focus(value))
        elif name in ("Quality", "Low latency"):
            quality = "low latency" if _on(source["Low latency"]) else str(source["Quality"]).lower()
            ordinal, text = {"normal": (0, "Normal"), "high": (1, "High"),
                             "ultra": (2, "Ultra"), "low latency": (3, "Low latency")}[quality]
            selector, candidate = put("Quality", ordinal, text=text)
            if name == "Low latency" and _on(value):
                issue(name, value, "nonzero_low_latency",
                      "Soothe3 reference reports zero latency; Lullaby Low latency retains an FFT delay.",
                      selector, candidate, frame)
        elif name == "Linear phase":
            enabled = _on(value)
            put("Phase", float(enabled), text="Linear" if enabled else "Minimum")
        elif re.fullmatch(r"(Detail|Attack|Release) tilt (low|high)", name):
            control, _, side = name.split()
            multiplier = _bounded(value, .1, 10)
            selector, candidate = put(f"{control} / {side}", math.log10(multiplier))
            if multiplier != 1:
                issue(name, value, "uncalibrated_tilt",
                      "Soothe3 frequency multipliers and Lullaby additive/exponential tilts differ; "
                      "candidate uses fixed log10(multiplier).", selector, candidate, frame)
        elif (match := re.fullmatch(r"Band ([1-8]) (.+)", name)):
            band, control = int(match.group(1)), match.group(2)
            if control in ("used", "enabled"):
                enabled = _on(source[f"Band {band} enabled"]) and _on(source[f"Band {band} used"])
                put("Enabled", float(enabled), band=band, text="On" if enabled else "Off")
            elif control == "frequency":
                put("Frequency", _bounded(value, 20, 20000), band=band)
            elif control == "shape":
                put("Shape", BAND_SHAPES[str(value).strip().lower()], band=band)
            elif control == "depth":
                uncalibrated("Weight", _bounded(value, -10, 10),
                             "Soothe3 band depth is unitless -10..10; Lullaby Weight is dB. "
                             "Same-number weighting is exploratory, not calibrated.", band=band)
            elif control == "q":
                q = _bounded(value, .1, 10)
                width = 2 * math.asinh(1 / (2 * q)) / math.log(2)
                candidate = max(.1, min(4.0, width))
                uncalibrated("Width", candidate,
                             "Candidate converts conventional Q to octave bandwidth (clamped to .1..4); "
                             "Lullaby's Gaussian/log-frequency weighting has no verified Q equivalence.", band=band)
            elif control == "focus":
                put("Focus", _focus(value), band=band)
            elif control == "channels" and str(value).strip().lower() == "all channels":
                notes.append(f"{name}: all channels is implicit in Lullaby's mono/stereo band.")
            else:
                issue(name, value, "missing_feature", "Lullaby does not expose this Soothe3 band feature.", frame=frame)
        else:
            issue(name, value, "missing_feature", "No audited Lullaby mapping exists for this control.", frame=frame)

    if not renderable:
        issue("Channel layout", record.get("channels"), "unsupported_layout",
              "Lullaby exposes mono/stereo only. Reference channel count and arrangement must be preserved.")

    def safe_convert(name, value, frame=None):
        try:
            convert(name, value, frame)
        except (KeyError, TypeError, ValueError) as error:
            issue(name, value, "unmappable_control", str(error), frame=frame)

    for name, value in source.items():
        safe_convert(name, value)
    # Every instance starts from explicit native monitoring/audition controls.
    for title, plain, text in (("Band audition", -1, "-1"),):
        try:
            emit(title, text, title, plain, text=text)
        except ValueError as error:
            issue(title, text, "unmappable_control", str(error))
    for event in record.get("automation", []):
        name, value = event["parameter"], deepcopy(event["value"])
        frame = event.get("frame")
        if frame is None and "seconds" in event:
            frame = round(event["seconds"] * record["sample_rate"])
        if isinstance(frame, bool) or not isinstance(frame, int) or frame < 0:
            issue(name, value, "invalid_automation", "Automation requires a nonnegative integer frame.")
            continue
        source[name] = value
        safe_convert(name, value, frame)
    if len(by_name.get("Delta", [])) != 1:
        renderable = False
    notes.insert(0, "Exploratory conversions are fixed across all cases, never fitted to reference output. "
                 "Any unsupported item is a mandatory failing capability assertion.")
    notes.append("Render native Lullaby Delta separately with Delta=On; do not substitute dry-minus-wet.")
    return {"host_settings": settings, "host_automation": automation,
            "expected_parameters": expected, "unsupported": unsupported,
            "notes": notes, "renderable": renderable}
