"""Capture health checks. Silent native Delta is a failure for every retained case."""
import json
import struct
from pathlib import Path

import numpy as np


def read_wav(path):
    contents = Path(path).read_bytes()
    if contents[:4] != b"RIFF" or contents[8:12] != b"WAVE":
        raise RuntimeError(f"Not a RIFF WAV: {path}")
    position, fmt, payload = 12, None, None
    while position + 8 <= len(contents):
        tag = contents[position:position+4]
        size = struct.unpack_from("<I", contents, position+4)[0]
        chunk = contents[position+8:position+8+size]
        if tag == b"fmt ":
            fmt = struct.unpack_from("<HHIIHH", chunk)
        if tag == b"data":
            payload = chunk
        position += 8 + size + size % 2
    if fmt is None or payload is None or fmt[0] != 3 or fmt[-1] != 32:
        raise RuntimeError(f"Expected IEEE float32 WAV: {path}")
    return np.frombuffer(payload, dtype="<f4").reshape(-1, fmt[1]).T.copy(), fmt[2]


def check_settings(record, directory, delta=False):
    state_name = "delta-state.json" if delta else "plugin-state.json"
    state = json.loads((directory / state_name).read_text())
    snapshot = state["snapshots"][state["current_snapshot"]]["params"]
    host = record["delta_host"] if delta else record["host"]
    observed = {p["name"]: p for p in host["parameters"]}
    expected = {**record["settings"], "Delta": "on" if delta else "off"}
    for event in record.get("automation", []):
        expected[event["parameter"]] = event["value"]
    for name, value in expected.items():
        if isinstance(value, dict):
            if abs(observed[name]["normalized"] - value["raw"]) > 1e-5:
                raise RuntimeError(f"Unapplied normalized setting: {record['id']}, {name}")
        else:
            try:
                numeric = float(value)
            except ValueError:
                if observed[name]["display"].casefold() != value.casefold():
                    raise RuntimeError(f"Unapplied enum setting: {record['id']}, {name}")
            else:
                if name in snapshot and abs(snapshot[name] - numeric) > max(.001, abs(numeric) * 1e-5):
                    raise RuntimeError(f"Unapplied numeric setting: {record['id']}, {name}")
    return len(expected)


def validate(directory, cross_host=False, plugin=None):
    from reference_capture import (MIN_DELTA_DBFS, MIN_DELTA_RELATIVE_DB,
                                   active_slice, level_db, rms, sha, write_json)
    directory = Path(directory)
    rows = json.loads((directory / "index.json").read_text())
    case_dirs = list((directory / "cases").iterdir())
    if not 1 <= len(rows) <= 25 or {p.name for p in case_dirs} != {r["id"] for r in rows}:
        raise RuntimeError("The corpus must contain only its indexed cases, with a maximum of 25")
    checks, setting_checks = [], 0
    for row in rows:
        folder = directory / row["path"]
        record = json.loads((folder / "capture.json").read_text())
        audio = {}
        for name, filename in record["files"].items():
            if sha(folder / filename) != record["sha256"][name]:
                raise RuntimeError(f"WAV checksum mismatch: {row['id']}/{filename}")
            data, rate = read_wav(folder / filename)
            if rate != record["sample_rate"] or not np.isfinite(data).all():
                raise RuntimeError(f"WAV rate/finite sample failure: {row['id']}/{filename}")
            audio[name] = data
        for filename, digest in record["state_sha256"].items():
            if sha(folder / filename) != digest:
                raise RuntimeError(f"State checksum mismatch: {row['id']}/{filename}")
        dry, wet, delta, raw = (audio[k] for k in ["dry", "wet", "delta", "raw"])
        if dry.shape != wet.shape or dry.shape != delta.shape or dry.shape != (record["channels"], record["frames"]):
            raise RuntimeError(f"Dry/wet/Delta shape mismatch: {row['id']}")
        latency = record["host"]["latency_samples"]
        if not np.array_equal(raw[:, latency:latency+dry.shape[1]], wet):
            raise RuntimeError(f"Raw/wet alignment mismatch: {row['id']}")
        rate = record["sample_rate"]
        delta_rms = rms(active_slice(delta, rate))
        relative = level_db(delta_rms / max(rms(active_slice(dry, rate)), 1e-30))
        if level_db(delta_rms) < MIN_DELTA_DBFS or relative < MIN_DELTA_RELATIVE_DB:
            raise RuntimeError(f"Silent/insignificant native Delta: {row['id']}")
        error = float(np.max(np.abs(wet.astype(float) + delta - dry)))
        if record.get("delta_reconstructs_dry", True) and error > 3e-6:
            raise RuntimeError(f"Native Delta reconstruction failed: {row['id']}, {error}")
        if "difference" in audio:
            expected = (dry.astype(float) - wet).astype("<f4")
            if not np.array_equal(expected, audio["difference"]):
                raise RuntimeError(f"Mathematical difference mismatch: {row['id']}")
        segment_checks = []
        for segment in record.get("segments", []):
            start = round((segment["start_seconds"] + .25) * rate)
            end = round((segment["end_seconds"] - .1) * rate)
            segment_dbfs = level_db(rms(delta[:, start:end]))
            if segment_dbfs < MIN_DELTA_DBFS:
                raise RuntimeError(f"Silent Delta segment: {row['id']}, {segment['label']}")
            segment_checks.append(dict(label=segment["label"], delta_rms_dbfs=segment_dbfs))
        setting_checks += check_settings(record, folder) + check_settings(record, folder, delta=True)
        checks.append(dict(case=row["id"], delta_rms_dbfs=level_db(delta_rms), delta_relative_db=relative,
                           native_delta_reconstruction_error=error,
                           native_delta_reconstructs_dry=record.get("delta_reconstructs_dry", True),
                           segments=segment_checks))
    independent = []
    repeat_error = None
    if cross_host:
        import pedalboard
        p = pedalboard.load_plugin(str(plugin))
        original = p.raw_state
        selected = ["01_soft_depth_3", "02_soft_depth_10", "04_hard_depth_10", "06_bandlimited_saw",
                    "20_linear_phase_high_quality", "22_ultra_96khz"]
        for row in rows:
            if row["id"] not in selected:
                continue
            folder = directory / row["path"]
            record = json.loads((folder / "capture.json").read_text())
            p.raw_state = original
            parameters = {v.name: v for v in p.parameters.values()}
            for item in record["host"]["parameters"]:
                if item["name"] != "Program":
                    parameters[item["name"]].raw_value = item["normalized"]
            dry, rate = read_wav(folder / "dry.wav")
            wet, _ = read_wav(folder / "wet.wav")
            padded = np.pad(dry, ((0, 0), (0, round(rate * .25))))
            rendered = p.process(padded, rate, buffer_size=256, reset=True)[:, :dry.shape[1]]
            error = float(np.max(np.abs(rendered - wet)))
            if error > 2e-6:
                raise RuntimeError(f"Independent VST3 host mismatch: {row['id']}, {error}")
            independent.append(dict(case=row["id"], max_error=error))
            if row["id"] == "02_soft_depth_10":
                repeated = p.process(padded, rate, buffer_size=256, reset=True)[:, :dry.shape[1]]
                repeat_error = float(np.max(np.abs(repeated - rendered)))
                if repeat_error > 2e-6:
                    raise RuntimeError(f"Repeatability failure: {repeat_error}")
        if not independent:
            raise RuntimeError("No compatible cross-host comparison cases")
    report = dict(schema_version=2, cases=len(rows), maximum_cases=25, checksums="passed",
                  wav_shapes_and_finite_samples="passed", raw_alignment="passed",
                  every_native_delta_non_silent="passed", requested_setting_checks=setting_checks,
                  checks=checks, independent_host_comparisons=independent, repeatability_max_error=repeat_error,
                  scope="Reference-file capture validation only; Lullaby parity tests remain a later phase")
    write_json(directory / "validation.json", report)
    print(f"Validated {len(rows)} cases: all native Deltas non-silent, {setting_checks} setting checks, "
          f"{len(independent)} independent-host comparisons.", flush=True)
    return report
