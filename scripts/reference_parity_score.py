#!/usr/bin/env python3
"""Freeze a full-run cohort, or compare unchanged-contract parity runs."""
import argparse
import json
from pathlib import Path

from reference.parity_score import compare_runs, freeze_cohort
from reference_capture import sha, write_json
from reference_parity import CONTRACT, load_contract


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    freeze = subcommands.add_parser("freeze", help="Freeze every rendered case from a complete baseline")
    freeze.add_argument("baseline", type=Path)
    freeze.add_argument("--output", type=Path, required=True)
    compare = subcommands.add_parser("compare", help="Score a candidate against the frozen baseline cohort")
    compare.add_argument("baseline", type=Path)
    compare.add_argument("candidate", type=Path)
    compare.add_argument("--cohort", type=Path, required=True)
    compare.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Choose a new output file; previous scores/cohorts are never overwritten")
    read = lambda path: json.loads(path.read_text())
    required_ids = [case["id"] for case in load_contract()["cases"]]
    try:
        baseline = read(args.baseline / "summary.json")
        if args.command == "freeze":
            result = freeze_cohort(baseline, sha(CONTRACT), required_ids)
        else:
            result = compare_runs(baseline, read(args.candidate / "summary.json"), read(args.cohort), sha(CONTRACT), required_ids)
        write_json(args.output, result)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.error(str(error))
    if args.command == "compare":
        print(json.dumps({key: result[key] for key in ("valid", "coverage", "baseline_score", "candidate_score", "reduction_percent")}, indent=2))
        return 0 if result["valid"] else 1
    print(f"Frozen {len(result['case_ids'])}/{len(required_ids)} cases: {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
