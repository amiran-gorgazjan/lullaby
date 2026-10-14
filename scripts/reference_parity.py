#!/usr/bin/env python3
"""Run deliberately strict, local VST3 parity tests against reviewed Soothe3 audio.

This renders Lullaby, never Soothe3. The proprietary plugin is not needed once
the reviewed reference corpus exists. Missing features and files fail, not skip.
"""
from __future__ import annotations

import argparse
import datetime
import io
import json
import math
import shutil
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

import numpy as np

from reference.validate import read_wav
from reference_capture import ROOT, sha, wav_write, write_json

CONTRACT = ROOT / "scripts/reference/parity_contract.json"


def load_contract(path=CONTRACT):
    contract = json.loads(Path(path).read_text())
    ids = [case["id"] for case in contract["cases"]]
    if len(ids) != 25 or len(set(ids)) != 25:
        raise ValueError("The reviewed contract must contain exactly 25 distinct cases")
    return contract


def verify_corpus(corpus, contract):
    """Pin the reviewed inputs independently of mutable capture metadata."""
    for name, digest in [("index.json", contract["index_sha256"]),
                         ("capture-environment.json", contract["environment_sha256"])]:
        if sha(corpus / name) != digest:
            raise ValueError(f"Reviewed corpus checksum changed: {name}")
    expected = {entry["id"] for entry in contract["cases"]}
    actual = {path.name for path in (corpus / "cases").iterdir() if path.is_dir()}
    if actual != expected:
        raise ValueError(f"Corpus membership changed: missing={sorted(expected-actual)}, extra={sorted(actual-expected)}")


def load_case(corpus, entry):
    folder = corpus / entry["path"]
    if entry["path"] != f"cases/{entry['id']}":
        raise ValueError("Unexpected reference path")
    if sha(folder / "capture.json") != entry["capture_sha256"]:
        raise ValueError(f"Reviewed capture metadata changed: {entry['id']}")
    for source in entry["files"].values():
        if sha(folder / source["path"]) != source["sha256"]:
            raise ValueError(f"Reviewed WAV changed: {entry['id']}/{source['path']}")
    for name, digest in entry["states"].items():
        if sha(folder / name) != digest:
            raise ValueError(f"Reviewed state changed: {entry['id']}/{name}")
    record = json.loads((folder / "capture.json").read_text())
    audio = {}
    for name in ["dry", "wet", "delta", *(["sidechain"] if "sidechain" in entry["files"] else [])]:
        data, rate = read_wav(folder / entry["files"][name]["path"])
        if rate != record["sample_rate"] or data.shape != (record["channels"], record["frames"]) or not np.isfinite(data).all():
            raise ValueError(f"Invalid reference WAV: {entry['id']}/{name}")
        audio[name] = data
    if not np.any(audio["delta"]):
        raise ValueError(f"Reference Delta is silent: {entry['id']}")
    return record, audio


def host_commands(executable, plugin, jobs, directory, timeout):
    """A separate bounded process per case isolates crashes and plugin lifetime."""
    payload = "".join(json.dumps(job) + "\n" for job in jobs)
    (directory / "host-jobs.json").write_text(json.dumps(jobs, indent=2) + "\n")
    with (directory / "host-stderr.log").open("w") as log:
        process = subprocess.run([str(executable), str(plugin)], input=payload,
                                 text=True, stdout=subprocess.PIPE, stderr=log,
                                 timeout=timeout, check=False)
    (directory / "host-stdout.jsonl").write_text(process.stdout)
    if process.returncode:
        raise RuntimeError(f"VST3 host exited {process.returncode}; see host-stderr.log")
    responses = [json.loads(line) for line in process.stdout.splitlines() if line.strip()]
    if len(responses) != len(jobs):
        raise RuntimeError(f"Expected {len(jobs)} host responses, received {len(responses)}")
    errors = [response["error"] for response in responses if not response.get("ok")]
    if errors:
        raise RuntimeError("; ".join(errors))
    return [response["result"] for response in responses]


def align_output(path, result, channels, frames):
    if len(result["latency_changes"]) != 1:
        raise ValueError("Candidate latency must be stable throughout each render")
    latency = result["latency_samples"]
    raw = np.fromfile(path, dtype="<f4")
    if raw.size != result["frames"] * channels:
        raise ValueError("Candidate PCM dimensions do not match its host report")
    raw = raw.reshape(channels, -1)
    aligned = raw[:, latency:latency+frames].copy()
    if aligned.shape != (channels, frames) or not np.isfinite(raw).all():
        raise ValueError("Candidate output is truncated, has wrong dimensions, or contains nonfinite samples")
    return raw, aligned


def parameter_failures(result, expectations, delta):
    observed = result["parameters"]
    failures = []
    for selector, expected in expectations.items():
        if selector.startswith("#"):
            matches = [p for p in observed if p["id"] == int(selector[1:])]
        else:
            matches = [p for p in observed if p["name"] == selector]
        target = 1.0 if expected["name"] == "Delta" and delta else expected["plain"]
        if len(matches) != 1 or not math.isclose(matches[0]["plain"], target, rel_tol=2e-5, abs_tol=1e-4):
            failures.append(f"{selector}: expected physical {target}, observed {[p['plain'] for p in matches]}")
    return failures


def comparison_segments(record):
    if record.get("segments"):
        return record["segments"]
    duration = record["frames"] / record["sample_rate"]
    transitions = sorted({event["frame"] / record["sample_rate"] for event in record.get("automation", [])})
    boundaries = sorted({0.0, .5, duration-1.5, duration, *transitions})
    return [dict(label=f"{start:g}–{end:g}s", start_seconds=start, end_seconds=end)
            for start, end in zip(boundaries, boundaries[1:]) if end > start]


def gain_routing_metrics(dry, wet, delta, unity_wet, unity_delta, mix, wet_gain, output_gain):
    """Observed native monitoring contract for the reviewed mixed-gain case.

    The same input/settings rendered at unity gains supply the processing basis.
    This relation is independently checked against the Soothe3 reference pair.
    """
    expected_wet = output_gain*((1-mix)*dry.astype(float)+mix*wet_gain*unity_wet)
    expected_delta = output_gain*mix*wet_gain*unity_delta.astype(float)
    return dict(wet_gain_routing_peak=float(np.max(np.abs(wet-expected_wet))),
                delta_gain_routing_peak=float(np.max(np.abs(delta-expected_delta))),
                unity_reconstruction_peak=float(np.max(np.abs(unity_wet.astype(float)+unity_delta-dry))),
                wet_formula="output_gain * ((1-mix)*dry + mix*wet_gain*unity_wet)",
                delta_formula="output_gain * mix * wet_gain * unity_delta")


def execute_case(corpus, entry, output, plugin, executable, parameters, timeout):
    from reference.parity_adapter import plan_case
    from reference.parity_metrics import compare_audio

    directory = output / "cases" / entry["id"]
    directory.mkdir(parents=True)
    report = dict(id=entry["id"], reference_path=str((corpus / entry["path"]).resolve()),
                  status="failed", failure=None, parameter_failures=[])
    try:
        record, source = load_case(corpus, entry)
        mapping = plan_case(record, parameters)
        report["mapping"] = mapping
        rate, channels, frames = record["sample_rate"], record["channels"], record["frames"]
        report.update(sample_rate=rate, channels=channels, frames=frames)
        # Even approximate settings render real audio for diagnosis. Capability
        # issues remain unconditional failures in their own contract test.
        # Unsupported layouts are attempted unchanged: never downmix or create
        # a false 16-channel result from independent stereo runs.
        with tempfile.TemporaryDirectory(prefix="pcm-", dir=directory) as temporary:
            work = Path(temporary)
            padding = round(rate * .25)
            np.pad(source["dry"], ((0, 0), (0, padding))).astype("<f4").tofile(work / "dry.pcm")
            job = dict(channels=channels, frames=frames+padding, sample_rate=rate,
                       arrangement=record["arrangement"], block_sizes=record["block_sizes"],
                       process_mode=record["process_mode"], input=str(work / "dry.pcm"),
                       settings=mapping["host_settings"], automation=mapping["host_automation"])
            if "sidechain" in source:
                np.pad(source["sidechain"], ((0, 0), (0, padding))).astype("<f4").tofile(work / "sidechain.pcm")
                job["sidechain_input"] = str(work / "sidechain.pcm")
            jobs = [{**job, "output": str(work / "wet.pcm"), "state_output": str(directory / "lullaby-wet.state")},
                    {**job, "settings": {**job["settings"], "Delta": {"raw": 1.0}},
                     "output": str(work / "delta.pcm"), "state_output": str(directory / "lullaby-delta.state")}]
            reconstructs = record.get("delta_reconstructs_dry", True)
            if not reconstructs:
                unity_settings = {**job["settings"], "Mix": "1", "Wet trim": "0", "Output": "0"}
                jobs.extend([{**job, "settings": unity_settings, "output": str(work / "unity-wet.pcm")},
                             {**job, "settings": {**unity_settings, "Delta": {"raw": 1.0}},
                              "output": str(work / "unity-delta.pcm")}])
            responses = host_commands(executable, plugin, jobs, directory, timeout)
            wet_host, delta_host = responses[:2]
            report["wet_host"], report["delta_host"] = wet_host, delta_host
            if wet_host["latency_samples"] != delta_host["latency_samples"]:
                raise ValueError("Candidate wet and Delta latencies differ")
            _, wet = align_output(work / "wet.pcm", wet_host, channels, frames)
            _, delta = align_output(work / "delta.pcm", delta_host, channels, frames)
            report["parameter_failures"] = [*parameter_failures(wet_host, mapping["expected_parameters"], False),
                                            *parameter_failures(delta_host, mapping["expected_parameters"], True)]
            wav_write(directory / "lullaby-wet.wav", wet, rate)
            wav_write(directory / "lullaby-delta.wav", delta, rate)
            wav_write(directory / "delta-error.wav", (delta.astype(float)-source["delta"]).astype("<f4"), rate)
            wav_write(directory / "wet-error.wav", (wet.astype(float)-source["wet"]).astype("<f4"), rate)
            segments = comparison_segments(record)
            report["delta_comparison"] = compare_audio(source["delta"], delta, source["dry"], rate, segments)
            report["wet_comparison"] = compare_audio(source["wet"], wet, source["dry"], rate, segments)
            report["routing"] = dict(unity_reconstruction_applicable=reconstructs,
                                     reconstruction_peak=float(np.max(np.abs(wet.astype(float)+delta-source["dry"]))) if reconstructs else None,
                                     note="Native Delta is compared directly, including the mixed-gain case; it is never replaced by dry-minus-wet.")
            if not reconstructs:
                unity_wet_host, unity_delta_host = responses[2:]
                report["unity_wet_host"], report["unity_delta_host"] = unity_wet_host, unity_delta_host
                _, unity_wet = align_output(work / "unity-wet.pcm", unity_wet_host, channels, frames)
                _, unity_delta = align_output(work / "unity-delta.pcm", unity_delta_host, channels, frames)
                unity_expectations = {key: {**value, "plain": {"Mix": 1.0, "Wet trim": 0.0, "Output": 0.0}.get(value["name"], value["plain"])}
                                      for key, value in mapping["expected_parameters"].items()}
                report["parameter_failures"].extend([
                    *parameter_failures(unity_wet_host, unity_expectations, False),
                    *parameter_failures(unity_delta_host, unity_expectations, True)])
                mix = float(record["settings"]["Mix"])/100
                wet_gain, output_gain = (10**(float(record["settings"][key])/20) for key in ["Wet trim", "Out trim"])
                report["routing"].update(gain_routing_metrics(source["dry"], wet, delta, unity_wet, unity_delta,
                                                             mix, wet_gain, output_gain))
                basis_entry = next(case for case in load_contract()["cases"] if case["id"] == "02_soft_depth_10")
                _, reference_basis = load_case(corpus, basis_entry)
                if not np.array_equal(source["dry"], reference_basis["dry"]):
                    raise ValueError("The reviewed mixed-gain routing basis must use identical dry samples")
                proof = gain_routing_metrics(source["dry"], source["wet"], source["delta"],
                                             reference_basis["wet"], reference_basis["delta"], mix, wet_gain, output_gain)
                if max(proof[key] for key in ["wet_gain_routing_peak", "delta_gain_routing_peak", "unity_reconstruction_peak"]) > 3e-6:
                    raise ValueError("Mixed-gain routing oracle is inconsistent with the pinned Soothe3 references")
                report["routing"]["soothe_reference_proof"] = proof
                wav_write(directory / "lullaby-unity-wet.wav", unity_wet, rate)
                wav_write(directory / "lullaby-unity-delta.wav", unity_delta, rate)
            report["candidate_sha256"] = {name: sha(directory / name) for name in
                                           ["lullaby-wet.wav", "lullaby-delta.wav", "delta-error.wav", "wet-error.wav"]}
            report["status"] = "rendered"
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.TimeoutExpired) as error:
        report["failure"] = f"{type(error).__name__}: {error}"
    write_json(directory / "report.json", report)
    (directory / "README.md").write_text(
        f"# {entry['id']} parity result\n\n"
        f"Reference: {report['reference_path']}\n\n"
        "`lullaby-delta.wav` is the actual plugin Delta output. `delta-error.wav` is "
        "Lullaby Delta minus Soothe3 Delta, without level changes. Perfect parity makes it silent. "
        "`lullaby-wet.wav` and `wet-error.wav` support diagnosis. Only reported host latency is removed.\n\n"
        "`report.json` records parameters, unsupported features, per-channel errors, spectra, "
        "time windows and automation segments. Approximate mappings are explicit contract failures.\n"
        + (f"\nRender failed: {report['failure']}\n" if report["failure"] else ""))
    return report


def null_failures(comparison, limits):
    failures = []
    # Per-channel gates prevent a bad channel being averaged away in surround.
    for row in [comparison["primary"], *comparison["channels"]]:
        label = f"channel {row['channel']}" if "channel" in row else "all channels"
        for key in ["error_rms_dbfs", "error_peak_dbfs"]:
            if row[key] is not None and row[key] > limits[key]:
                failures.append(f"{label}: {key}={row[key]:.2f}, limit={limits[key]:.2f}")
        if row["reference_rms"] > 1e-12 and row["error_relative_db"] is not None and row["error_relative_db"] > limits["error_relative_db"]:
            failures.append(f"{label}: residual/reference={row['error_relative_db']:.2f} dB, limit={limits['error_relative_db']:.2f}")
    return failures


def level_failures(rows, tolerance, floor_dbfs):
    floor = 10 ** (floor_dbfs/20)
    failures = []
    for index, row in enumerate(rows):
        label = (f"{row['low_hz']:g}–{row['high_hz']:g} Hz" if "low_hz" in row else
                 f"{row['start_seconds']:g}–{row['end_seconds']:g} s" if "start_seconds" in row else f"row {index}")
        if "channel" in row:
            label += f", channel {row['channel']}"
        reference, actual = row["reference_rms"], row["actual_rms"]
        if reference <= floor:
            if actual > floor:
                failures.append(f"{label}: energy where reference is effectively silent")
        elif actual <= 0 or abs(20*math.log10(actual/reference)) > tolerance:
            difference = None if actual <= 0 else 20*math.log10(actual/reference)
            failures.append(f"{label}: level error {difference} dB (limit ±{tolerance})")
    return failures


def make_suite(reports, contract):
    suite = unittest.TestSuite()
    limits, supportive = contract["primary_limits"], contract["supportive_limits"]
    for report in reports:
        class Case(unittest.TestCase):
            data = report

            def comparison(self):
                self.assertEqual(self.data["status"], "rendered", self.data["failure"])
                return self.data["delta_comparison"]

            def test_delta_null(self):
                failures = null_failures(self.comparison(), limits)
                self.assertFalse(failures, "Lullaby Delta − Soothe3 Delta is not silent:\n" + "\n".join(failures))

            def test_control_equivalence(self):
                self.assertIn("mapping", self.data, self.data["failure"])
                issues = self.data["mapping"]["unsupported"]
                self.assertEqual(len(issues), 0, json.dumps(issues, indent=2))
                self.assertFalse(self.data["parameter_failures"], "\n".join(self.data["parameter_failures"]))

            def test_delta_level(self):
                rows = self.comparison()["channels"]
                errors = level_failures(rows, supportive["delta_level_error_db"], supportive["reference_floor_dbfs"])
                self.assertEqual(len(errors), 0, "\n".join(errors[:8]))

            def test_delta_spectral_shape(self):
                rows = [{**channel, "low_hz": band["low_hz"], "high_hz": band["high_hz"]}
                        for band in self.comparison()["spectrum_bands"] for channel in band["channels"]]
                errors = level_failures(rows, supportive["spectral_band_level_error_db"], supportive["reference_floor_dbfs"])
                self.assertEqual(len(errors), 0, "\n".join(errors[:8]))

            def test_delta_timing_envelope(self):
                rows = [{**channel, "start_seconds": window["start_seconds"], "end_seconds": window["end_seconds"]}
                        for window in self.comparison()["time_envelope"] for channel in window["channels"]]
                errors = level_failures(rows, supportive["envelope_level_error_db"], supportive["reference_floor_dbfs"])
                self.assertEqual(len(errors), 0, "\n".join(errors[:8]))

            def test_wet_null(self):
                self.comparison()
                failures = null_failures(self.data["wet_comparison"], limits)
                self.assertFalse(failures, "Wet output differs:\n" + "\n".join(failures))

            def test_native_delta_routing(self):
                self.comparison()
                self.assertFalse(self.data["parameter_failures"], "\n".join(self.data["parameter_failures"]))
                routing = self.data["routing"]
                if routing["unity_reconstruction_applicable"]:
                    self.assertLessEqual(routing["reconstruction_peak"], 3e-6,
                                         "Native wet + native Delta does not reconstruct the dry signal")
                else:
                    for key in ["wet_gain_routing_peak", "delta_gain_routing_peak", "unity_reconstruction_peak"]:
                        self.assertLessEqual(routing[key], 3e-6, f"Mixed-gain native routing differs: {key}")
        Case.__name__ = report["id"]
        Case.__qualname__ = report["id"]
        suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(Case))
    return suite


class RecordedResult(unittest.TextTestResult):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.records = []

    def addSuccess(self, test):
        super().addSuccess(test)
        self.records.append(dict(name=test.id(), status="passed"))

    def addFailure(self, test, error):
        super().addFailure(test, error)
        self.records.append(dict(name=test.id(), status="failed", message=self._exc_info_to_string(error, test)))

    def addError(self, test, error):
        super().addError(test, error)
        self.records.append(dict(name=test.id(), status="error", message=self._exc_info_to_string(error, test)))


def junit_report(result, output):
    root = ET.Element("testsuite", name="Soothe3 native Delta parity", tests=str(result.testsRun),
                      failures=str(len(result.failures)), errors=str(len(result.errors)), skipped="0")
    for row in result.records:
        case, name = row["name"].rsplit(".", 1)
        item = ET.SubElement(root, "testcase", classname=case, name=name)
        if row["status"] != "passed":
            ET.SubElement(item, "failure" if row["status"] == "failed" else "error").text = row["message"]
    ET.ElementTree(root).write(output / "junit.xml", encoding="utf-8", xml_declaration=True)


def source_provenance(output, plugin, executable):
    paths = [Path(__file__).resolve(), CONTRACT, ROOT / "tools/reference-host/src/main.rs",
             ROOT / "tools/reference-host/Cargo.lock", *sorted((ROOT / "scripts/reference").glob("parity*.py")),
             ROOT / "scripts/reference_capture.py", ROOT / "scripts/reference/validate.py",
             ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "crates/lullaby-dsp/Cargo.toml",
             *sorted((ROOT / "scripts/reference").glob("test_parity*.py")),
             *sorted((ROOT / "src").rglob("*.rs")), *sorted((ROOT / "crates/lullaby-dsp/src").rglob("*.rs"))]
    sources = {}
    for path in paths:
        relative = path.relative_to(ROOT)
        destination = output / "provenance" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, destination)
        sources[str(relative)] = sha(path)
    binary_hashes = {str(path.relative_to(plugin)): sha(path) for path in sorted(plugin.rglob("*")) if path.is_file()}
    if not binary_hashes:
        raise ValueError("Candidate VST3 bundle contains no files")
    return dict(sources=sources, candidate_bundle=str(plugin), candidate_bundle_sha256=binary_hashes,
                host=str(executable), host_sha256=sha(executable), python=sys.version, numpy=np.__version__,
                git_head=subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, default=ROOT / ".local/soothe3-capture")
    parser.add_argument("--plugin", type=Path, default=ROOT / "target/bundled/Lullaby.vst3")
    parser.add_argument("--host", type=Path, default=ROOT / "target/reference-host/debug/reference-host")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--case", action="append", dest="case_ids", help="Run only these exact case IDs (partial diagnostic run)")
    parser.add_argument("--list", action="store_true")
    parser.add_argument("--timeout", type=float, default=120, help="Maximum seconds per case host process")
    args = parser.parse_args()
    contract = load_contract()
    entries = contract["cases"]
    if args.case_ids:
        unknown = set(args.case_ids)-{entry["id"] for entry in entries}
        if unknown:
            parser.error(f"Unknown cases: {sorted(unknown)}")
        entries = [entry for entry in entries if entry["id"] in args.case_ids]
    if args.list:
        for entry in entries:
            print(entry["id"])
        return 0
    if not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("--timeout must be positive and finite")
    corpus, plugin, executable = args.corpus.resolve(), args.plugin.resolve(), args.host.resolve()
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    output = (args.output or ROOT / "target/reference-parity" / timestamp).resolve()
    if output.exists():
        parser.error("Choose a new --output directory; stale candidate audio is never reused")
    output.mkdir(parents=True)
    global_error = None
    try:
        verify_corpus(corpus, contract)
        provenance = source_provenance(output, plugin, executable)
        info = host_commands(executable, plugin, [{"command": "inspect"}], output, args.timeout)[0]
        write_json(output / "lullaby-parameters.json", info)
        write_json(output / "environment.json", dict(timestamp=timestamp, corpus=str(corpus), **provenance))
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.TimeoutExpired) as error:
        global_error = f"{type(error).__name__}: {error}"
    reports = []
    for entry in entries:
        if global_error:
            report = dict(id=entry["id"], status="failed", failure=global_error, parameter_failures=[])
        else:
            report = execute_case(corpus, entry, output, plugin, executable, info["parameters"], args.timeout)
        reports.append(report)
        primary = report.get("delta_comparison", {}).get("primary")
        if primary:
            print(f"{entry['id']}: residual {primary['error_rms_dbfs']} dBFS RMS, {primary['error_relative_db']} dB relative", flush=True)
        else:
            print(f"{entry['id']}: RENDER/REFERENCE FAILURE: {report['failure']}", flush=True)
    log = io.StringIO()
    result = unittest.TextTestRunner(stream=log, verbosity=2, resultclass=RecordedResult).run(make_suite(reports, contract))
    (output / "tests.txt").write_text(log.getvalue())
    junit_report(result, output)
    primary_tests = [row for row in result.records if row["name"].endswith(".test_delta_null")]
    summary = dict(schema_version=1, full_corpus=len(entries) == 25, cases=len(entries),
                   primary_delta_tests=len(primary_tests), primary_passed=sum(row["status"] == "passed" for row in primary_tests),
                   tests=result.testsRun, failures=len(result.failures), errors=len(result.errors), skipped=len(result.skipped),
                   passed=result.wasSuccessful(), contract_sha256=sha(CONTRACT), limits=contract["primary_limits"],
                   results=result.records, cases_summary=[dict(id=r["id"], status=r["status"], failure=r["failure"],
                       primary=r.get("delta_comparison", {}).get("primary"), unsupported=r.get("mapping", {}).get("unsupported", [])) for r in reports])
    write_json(output / "summary.json", summary)
    print(f"\n{len(entries)} cases, {len(primary_tests)} primary Delta tests: {summary['primary_passed']} passed. "
          f"{result.testsRun} total tests, {len(result.failures)} failures, {len(result.errors)} errors, {len(result.skipped)} skipped.")
    print(f"Reports and residual audio: {output}")
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    raise SystemExit(main())
