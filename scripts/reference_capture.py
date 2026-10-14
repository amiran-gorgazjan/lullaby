#!/usr/bin/env python3
"""Generate at most 25 local Soothe3 reference cases, with native Delta audio."""
from __future__ import annotations

import argparse
import csv
import datetime
import hashlib
import json
import platform
import plistlib
import shutil
import struct
import subprocess
from pathlib import Path

import numpy as np
from reference import plan, signals

ROOT = Path(__file__).resolve().parents[1]
MIN_DELTA_DBFS = -65.0
MIN_DELTA_RELATIVE_DB = -40.0


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, allow_nan=False) + "\n")


def sha(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def read_state(path):
    text = path.read_bytes().decode("utf-8")
    state, end = json.JSONDecoder().raw_decode(text)
    return state, text[end:].encode("utf-8")


def wav_write(path, data, rate):
    """Interleaved IEEE float32 WAV. Preserve every sample without gain changes."""
    path.parent.mkdir(parents=True, exist_ok=True)
    channels, frames = data.shape
    payload = data.T.astype("<f4").tobytes()
    fmt = struct.pack("<HHIIHH", 3, channels, rate, rate * channels * 4, channels * 4, 32)
    body = b"WAVEfmt " + struct.pack("<I", len(fmt)) + fmt
    body += b"fact" + struct.pack("<II", 4, frames)
    body += b"data" + struct.pack("<I", len(payload)) + payload
    path.write_bytes(b"RIFF" + struct.pack("<I", len(body)) + body)


def active_slice(data, rate):
    end = data.shape[1] - round(signals.TAIL * rate) - round(.25 * rate)
    return data[:, round((signals.LEAD + .25) * rate):end]


def rms(data):
    return float(np.sqrt(np.mean(data.astype(float) ** 2)))


def level_db(value):
    return float(20 * np.log10(max(value, 1e-30)))


def audio_metrics(data, rate):
    active = active_slice(data, rate)
    hop = round(.1 * rate)
    return dict(active_rms=np.sqrt(np.mean(active.astype(float) ** 2, axis=1)).tolist(),
                active_rms_dbfs=level_db(rms(active)), peak=np.max(np.abs(data), axis=1).tolist(),
                envelope_hop_seconds=.1,
                rms_envelope=[np.sqrt(np.mean(data[:, p:p+hop].astype(float) ** 2, axis=1)).tolist()
                              for p in range(0, data.shape[1], hop)])


class Host:
    def __init__(self, plugin, executable):
        self.process = subprocess.Popen([str(executable), str(plugin)],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        text=True, bufsize=1)

    def command(self, job):
        self.process.stdin.write(json.dumps(job) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            raise RuntimeError(f"Reference host exited: {self.process.poll()}")
        response = json.loads(line)
        if not response["ok"]:
            raise RuntimeError(response["error"])
        return response["result"]

    def close(self):
        self.process.stdin.close()
        self.process.wait(timeout=30)


def render(host, job, work, name, dry_frames, channels):
    result = host.command({**job, "output": str(work / f"{name}.pcm")})
    raw = np.fromfile(work / f"{name}.pcm", dtype="<f4").reshape(channels, -1)
    if len(result["latency_changes"]) != 1:
        raise RuntimeError(f"Latency changed within {name}")
    latency = result["latency_samples"]
    aligned = raw[:, latency:latency+dry_frames].copy()
    if aligned.shape != (channels, dry_frames) or not np.isfinite(raw).all():
        raise RuntimeError(f"Invalid output shape/finite samples: {name}")
    return result, raw, aligned


def capture(args, host, info):
    output = args.output.resolve()
    cases = plan.build(info["parameters"])
    if args.limit is not None:
        if not 1 <= args.limit <= plan.MAX_CASES:
            raise ValueError("--limit must be between 1 and 25")
        cases = cases[:args.limit]
    write_json(output / "plan.json", cases)
    specs = {s["id"]: s for s in signals.specs()}
    work = output / "work"
    work.mkdir()
    plugin_info = plistlib.loads((args.plugin / "Contents/Info.plist").read_bytes())
    binary = args.plugin / "Contents/MacOS" / plugin_info["CFBundleExecutable"]
    metadata = dict(schema_version=2, created_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    plugin=plugin_info["CFBundleName"], plugin_version=plugin_info["CFBundleShortVersionString"],
                    plugin_format="VST3", plugin_binary_sha256=sha(binary), host_binary_sha256=sha(args.host),
                    host_source_sha256=sha(ROOT / "tools/reference-host/src/main.rs"),
                    numpy_version=np.__version__, signal_generator_version=signals.VERSION, seed=signals.SEED,
                    platform=platform.platform(), architecture=platform.machine(),
                    sample_format="IEEE float32 WAV; no normalization, dither, resampling or listening gain",
                    alignment="Remove the reported VST3 latency; preserve matching dry/wet/delta frame counts",
                    delta_source="A separate fresh Soothe3 instance with identical settings and automation, Delta on",
                    minimum_delta_dbfs=MIN_DELTA_DBFS, minimum_delta_relative_db=MIN_DELTA_RELATIVE_DB,
                    maximum_case_count=plan.MAX_CASES)
    config = Path.home() / "Library/Application Support/oeksound/soothe3/config.json"
    if config.exists():
        metadata["global_config"] = json.loads(config.read_text())
    write_json(output / "capture-environment.json", metadata)
    source_files = [ROOT / "tools/reference-host/src/main.rs", ROOT / "tools/reference-host/Cargo.toml",
                    ROOT / "tools/reference-host/Cargo.lock", Path(__file__).resolve(),
                    *sorted((ROOT / "scripts/reference").glob("*.py")), ROOT / "scripts/reference/requirements.txt"]
    source_manifest = []
    for source in source_files:
        relative = source.relative_to(ROOT)
        destination = output / "provenance" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
        source_manifest.append(dict(path=str(relative), sha256=sha(source)))
    write_json(output / "provenance/source-manifest.json", source_manifest)
    rows = []
    for case in cases:
        directory = output / "cases" / case["id"]
        directory.mkdir(parents=True)
        rate, channels = case["sample_rate"], case["channels"]
        spec = specs[case["signal"]]
        dry = signals.generate(spec, rate)[:channels]
        wav_write(directory / "dry.wav", dry, rate)
        padded = np.pad(dry, ((0, 0), (0, round(rate * .25))))
        padded.astype("<f4").tofile(work / "input.pcm")
        job = {**case, "input": str(work / "input.pcm"), "frames": padded.shape[1],
               "state_output": str(directory / "plugin.state")}
        if "sidechain_signal" in case:
            key = signals.generate(specs[case["sidechain_signal"]], rate)
            wav_write(directory / "sidechain.wav", key, rate)
            np.pad(key, ((0, 0), (0, round(rate * .25)))).astype("<f4").tofile(work / "key.pcm")
            job["sidechain_input"] = str(work / "key.pcm")
        result, raw, wet = render(host, job, work, "wet", dry.shape[1], channels)
        state, _ = read_state(directory / "plugin.state")
        write_json(directory / "plugin-state.json", state)
        delta_job = {**job, "settings": {**case["settings"], "Delta": "on"},
                     "state_output": str(directory / "delta.state")}
        delta_result, _, delta = render(host, delta_job, work, "delta", dry.shape[1], channels)
        delta_state, _ = read_state(directory / "delta.state")
        write_json(directory / "delta-state.json", delta_state)
        if delta_result["latency_samples"] != result["latency_samples"]:
            raise RuntimeError(f"Wet and Delta latency differ: {case['id']}")
        dry_rms = rms(active_slice(dry, rate))
        delta_rms = rms(active_slice(delta, rate))
        delta_dbfs, relative_db = level_db(delta_rms), level_db(delta_rms / max(dry_rms, 1e-30))
        if delta_dbfs < MIN_DELTA_DBFS or relative_db < MIN_DELTA_RELATIVE_DB:
            raise RuntimeError(f"Silent/insignificant native Delta: {case['id']}, {delta_dbfs:.2f} dBFS, {relative_db:.2f} dB relative")
        difference = (dry.astype(float) - wet).astype("<f4")
        reconstruction_error = float(np.max(np.abs(wet.astype(float) + delta - dry)))
        if case.get("delta_reconstructs_dry", True) and reconstruction_error > 3e-6:
            raise RuntimeError(f"Soothe Delta does not reconstruct dry: {case['id']}, {reconstruction_error}")
        for name, audio in [("raw", raw), ("wet", wet), ("delta", delta)]:
            wav_write(directory / f"{name}.wav", audio, rate)
        if not case.get("delta_reconstructs_dry", True):
            wav_write(directory / "difference.wav", difference, rate)
        record = {**case, "signal_recipe": spec, "frames": dry.shape[1], "host": result,
                  "delta_host": delta_result, "dry_metrics": audio_metrics(dry, rate),
                  "wet_metrics": audio_metrics(wet, rate), "delta_metrics": audio_metrics(delta, rate),
                  "delta_rms_dbfs": delta_dbfs, "delta_vs_dry_db": relative_db,
                  "wet_gain_db": level_db(rms(active_slice(wet, rate)) / dry_rms),
                  "dry_wet_delta_reconstruction_max_error": reconstruction_error,
                  "files": {name: f"{name}.wav" for name in ["dry", "raw", "wet", "delta"]}}
        if "sidechain_signal" in case:
            record["files"]["sidechain"] = "sidechain.wav"
        if not case.get("delta_reconstructs_dry", True):
            record["files"]["difference"] = "difference.wav"
        if channels > 2:
            energies = np.mean(active_slice(delta, rate).astype(float) ** 2, axis=1)
            selected = np.sort(np.argsort(energies)[-2:])
            record["preview_channels_one_based"] = (selected + 1).tolist()
            for name, audio in [("dry", dry), ("wet", wet), ("delta", delta)]:
                filename = f"preview-{name}-stereo.wav"
                wav_write(directory / filename, audio[selected], rate)
                record["files"][f"preview_{name}"] = filename
        record["sha256"] = {name: sha(directory / filename) for name, filename in record["files"].items()}
        record["state_sha256"] = {filename: sha(directory / filename) for filename in
                                  ["plugin.state", "plugin-state.json", "delta.state", "delta-state.json"]}
        write_json(directory / "capture.json", record)
        effective = {p["name"]: p["display"] for p in result["parameters"]}
        write_json(directory / "settings.json", dict(initial=case["settings"], automation=case.get("automation", []),
                                                     final_effective=effective, segments=case.get("segments", [])))
        (directory / "README.md").write_text(
            f"# {case['id']}\n\n{case['purpose']}\n\n"
            "- `dry.wav`: original synthetic input.\n"
            "- `wet.wav`: latency-aligned Soothe3 output.\n"
            "- `raw.wav`: uncompensated output, including host latency and flush.\n"
            "- `delta.wav`: actual Soothe3 Delta mode, aligned to the dry input.\n"
            "- `settings.json`: initial controls, automation timeline and final controls.\n"
            "- `capture.json`: full measurements, states, alignment and checksums.\n\n"
            f"Delta RMS: **{delta_dbfs:.2f} dBFS**, **{relative_db:.2f} dB relative to dry**. "
            f"Wet RMS change: **{record['wet_gain_db']:.2f} dB**. No file is loudness-normalized.\n\n"
            + ("`difference.wav` is the mathematical dry-minus-wet signal, including gain changes; "
               "it is distinct from Soothe's Delta monitoring in this gain-routing case.\n" if not case.get("delta_reconstructs_dry", True) else "")
            + (f"Stereo previews select channels {record['preview_channels_one_based']} without changing their levels; "
               "the full 16-channel WAVs are the test references.\n" if channels > 2 else ""))
        row = {k: record[k] for k in ["id", "purpose", "sample_rate", "channels", "wet_gain_db", "delta_rms_dbfs", "delta_vs_dry_db"]}
        row["path"] = f"cases/{case['id']}"
        rows.append(row)
        write_json(output / "index.json", rows)
        print(f"{len(rows)}/{len(cases)} {case['id']}: wet {row['wet_gain_db']:.2f} dB, native Delta {delta_dbfs:.2f} dBFS ({relative_db:.2f} dB relative)", flush=True)
    with (output / "summary.csv").open("w", newline="") as file:
        writer = csv.DictWriter(file, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    listing = "\n".join(f"| [{r['id']}]({r['path']}/) | {r['purpose']} | {r['delta_rms_dbfs']:.2f} |" for r in rows)
    (output / "README.md").write_text(
        f"# Soothe3 reference audio — {len(rows)} cases\n\n"
        "Start with **04_hard_depth_10** for the strongest contrast, then compare **01_soft_depth_3** "
        "with **02_soft_depth_10**. Open the WAVs directly or import them into a DAW.\n\n"
        "Every case folder is self-contained: dry, aligned wet, uncompensated raw, and actual Soothe3 Delta audio, "
        "plus settings and metadata. Most files are 8 s; the eight-shape sequence is 18 s. "
        "Levels are preserved. Validation fails if any active case has silent or insignificant Delta.\n\n"
        "| Case | Purpose | Delta RMS (dBFS) |\n| --- | --- | ---: |\n" + listing + "\n")
    shutil.rmtree(work)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--plugin", type=Path, default=Path("/Library/Audio/Plug-Ins/VST3/soothe3.vst3"))
    parser.add_argument("--host", type=Path, default=ROOT / "target/reference-host/debug/reference-host")
    parser.add_argument("--output", type=Path, default=ROOT / ".local/soothe3-capture")
    parser.add_argument("--inspect", action="store_true")
    parser.add_argument("--limit", type=int)
    parser.add_argument("--validate", action="store_true")
    parser.add_argument("--cross-host", action="store_true")
    args = parser.parse_args()
    if args.validate:
        from reference.validate import validate
        validate(args.output.resolve(), args.cross_host, args.plugin.resolve())
        return
    if (args.output / "cases").exists():
        raise RuntimeError("Choose an empty --output directory; existing cases are never silently mixed with new captures")
    args.output.mkdir(parents=True, exist_ok=True)
    host = Host(args.plugin.resolve(), args.host.resolve())
    try:
        info = host.command({"command": "inspect", "state_output": str((args.output / "factory-default.state").resolve())})
        write_json(args.output / "parameters.json", info)
        if not args.inspect:
            capture(args, host, info)
    finally:
        host.close()
    if not args.inspect:
        from reference.validate import validate
        validate(args.output.resolve(), args.cross_host, args.plugin.resolve())


if __name__ == "__main__":
    main()
