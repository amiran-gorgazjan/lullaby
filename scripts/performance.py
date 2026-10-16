#!/usr/bin/env python3
"""Paired local CPU benchmark and baseline audio-equivalence gate; standard library only."""
import argparse
import array
import json
import math
import pathlib
import random
import statistics
import subprocess
import sys


def interval(ratios, seed=113):
    """Paired log-ratio bootstrap, geometric mean and a two-sided 95% interval."""
    logs = [math.log(x) for x in ratios]
    rng = random.Random(seed)
    draws = sorted(math.exp(statistics.mean(rng.choices(logs, k=len(logs)))) for _ in range(10000))
    return math.exp(statistics.mean(logs)), draws[250], draws[9749]


def evaluate(pairs):
    names = [c["case"] for c in pairs[0][0]["cases"]]
    by_case = {}
    aggregate = []
    for before, after in pairs:
        assert names == [c["case"] for c in before["cases"]] == [c["case"] for c in after["cases"]]
        aggregate.append(math.exp(statistics.mean(math.log(b["ns_per_sample"] / a["ns_per_sample"])
            for a, b in zip(before["cases"], after["cases"]))))
    for i, name in enumerate(names):
        ratios = [after["cases"][i]["ns_per_sample"] / before["cases"][i]["ns_per_sample"] for before, after in pairs]
        mean, low, high = interval(ratios)
        by_case[name] = {"ratio":mean,"ci95":[low,high],"improvement_percent":100 * (1-mean),
            "baseline_ns_per_sample":statistics.median(p[0]["cases"][i]["ns_per_sample"] for p in pairs),
            "candidate_ns_per_sample":statistics.median(p[1]["cases"][i]["ns_per_sample"] for p in pairs),
            "candidate_realtime_percent":statistics.median(p[1]["cases"][i]["realtime_percent"] for p in pairs)}
    mean, low, high = interval(aggregate)
    # A clear >=5% gain either across the suite or in the default preset, with
    # the entire interval at least 2% better; no credible >5% case regression.
    default = by_case["default_stereo_48k"]
    regressions = [name for name, c in by_case.items() if c["ratio"] > 1.05 and c["ci95"][0] > 1.02]
    gain = (mean <= .95 and high < .98) or (default["ratio"] <= .95 and default["ci95"][1] < .98)
    return {"aggregate_ratio":mean,"aggregate_ci95":[low,high],"aggregate_improvement_percent":100*(1-mean),
        "cases":by_case,"credible_regressions":regressions,"performance_pass":gain and not regressions}


def compare_audio(before, after):
    a, b = array.array("f"), array.array("f")
    a.frombytes(pathlib.Path(before).read_bytes()); b.frombytes(pathlib.Path(after).read_bytes())
    if len(a) != len(b) or not a:
        raise ValueError("capture size mismatch")
    if not all(math.isfinite(x) for x in a) or not all(math.isfinite(x) for x in b):
        raise ValueError("non-finite capture")
    error = [x-y for x, y in zip(a,b)]
    maximum = max(map(abs,error))
    rms = math.sqrt(sum(x*x for x in error)/len(error))
    # Captures also include dB meter values; keep all states tightly equivalent.
    return {"float_count":len(a),"max_abs_error":maximum,"rms_error":rms,"bit_identical":a.tobytes()==b.tobytes(),
        "audio_pass":maximum <= 2e-5 and rms <= 1e-6}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=pathlib.Path)
    parser.add_argument("--candidate", required=True, type=pathlib.Path)
    parser.add_argument("--baseline-plugin", type=pathlib.Path,
                        help="native library for a dynamic VST3 probe")
    parser.add_argument("--candidate-plugin", type=pathlib.Path)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    parser.add_argument("--pairs", type=int, default=9)
    parser.add_argument("--frames", type=int, default=24000)
    args = parser.parse_args()
    if args.pairs < 5:
        parser.error("at least five paired trials are required")
    if bool(args.baseline_plugin) != bool(args.candidate_plugin):
        parser.error("both plugin paths are required together")
    args.output.mkdir(parents=True, exist_ok=True)
    def run(exe, capture=None, plugin=None):
        cmd = [str(exe.resolve()),"--frames",str(args.frames)]
        if plugin: cmd += ["--plugin",str(plugin.resolve())]
        if capture: cmd += ["--capture",str(capture)]
        return json.loads(subprocess.check_output(cmd,text=True))
    # Unmeasured process warmups also capture deterministic outputs.
    run(args.baseline,args.output/"baseline.f32",args.baseline_plugin)
    run(args.candidate,args.output/"candidate.f32",args.candidate_plugin)
    pairs = []
    for i in range(args.pairs):
        if i % 2:
            after = run(args.candidate,plugin=args.candidate_plugin)
            before = run(args.baseline,plugin=args.baseline_plugin)
        else:
            before = run(args.baseline,plugin=args.baseline_plugin)
            after = run(args.candidate,plugin=args.candidate_plugin)
        pairs.append([before,after])
        print(f"completed paired trial {i+1}/{args.pairs}",file=sys.stderr,flush=True)
        (args.output/"trials.json").write_text(json.dumps(pairs,indent=2)+"\n")
    result = evaluate(pairs)
    result["audio"] = compare_audio(args.output/"baseline.f32",args.output/"candidate.f32")
    result["accepted"] = result["performance_pass"] and result["audio"]["audio_pass"]
    result["pairs"] = args.pairs
    result["frames"] = args.frames
    (args.output/"summary.json").write_text(json.dumps(result,indent=2)+"\n")
    print(json.dumps({k:v for k,v in result.items() if k != "cases"},indent=2))
    return 0 if result["accepted"] else 1


if __name__ == "__main__":
    sys.exit(main())
