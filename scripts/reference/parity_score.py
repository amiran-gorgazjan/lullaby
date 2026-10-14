"""Fixed-cohort progress scoring; never a replacement for strict null gates.

Each case has equal weight. A persisted cohort contains every successfully
rendered baseline case; failures remain explicit in full-corpus coverage. A
candidate losing any cohort case has no aggregate score. No fitted gain/lag is
used. Reference RMS below 1e-12 uses that fixed denominator to avoid NaN.
"""
import math


SCHEMA_VERSION = 1
REFERENCE_RMS_FLOOR = 1e-12


def _number(value, label):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
        raise ValueError(f"Invalid finite nonnegative measurement: {label}")
    return float(value)


def _case_measurement(row):
    primary = row["primary"]
    error = _number(primary["error_rms"], "error_rms")
    reference = _number(primary["reference_rms"], "reference_rms")
    normalized = (error / max(reference, REFERENCE_RMS_FLOOR)) ** 2
    channels = row["channels"]
    if not channels or [c["channel"] for c in channels] != list(range(len(channels))):
        raise ValueError(f"Invalid channel membership: {row['id']}")
    channel_scores = []
    for channel in channels:
        er = _number(channel["error_rms"], "channel error_rms")
        rr = _number(channel["reference_rms"], "channel reference_rms")
        channel_scores.append(dict(channel=channel["channel"], error_rms=er, reference_rms=rr,
                                   normalized_mse=(er / max(rr, REFERENCE_RMS_FLOOR)) ** 2))
    if not math.isfinite(normalized) or any(not math.isfinite(c["normalized_mse"]) for c in channel_scores):
        raise ValueError("Normalized residual overflow")
    # Aggregate and per-channel measurements must describe the same audio.
    for field, measured in (("error_rms", error), ("reference_rms", reference)):
        combined = math.sqrt(math.fsum(c[field] ** 2 for c in channels) / len(channels))
        if not math.isclose(combined, measured, rel_tol=1e-10, abs_tol=1e-15):
            raise ValueError(f"Inconsistent aggregate/channel {field}: {row['id']}")
    return dict(id=row["id"], normalized_mse=normalized, error_rms=error,
                reference_rms=reference, channels=channel_scores)


def _validate_summary(summary, contract_sha256, required_ids):
    if summary["contract_sha256"] != contract_sha256:
        raise ValueError("Acceptance contract mismatch")
    rows = summary["cases_summary"]
    ids = [row["id"] for row in rows]
    if len(set(ids)) != len(ids) or set(ids) != set(required_ids):
        raise ValueError("Full case membership mismatch; missing/extra/duplicate cases cannot be dropped")
    if not summary.get("full_corpus") or summary.get("cases") != len(required_ids):
        raise ValueError("Progress scoring requires a full-corpus run")
    measurements = {}
    failures = {}
    for row in rows:
        if row["status"] == "rendered":
            try:
                measurements[row["id"]] = _case_measurement(row)
            except (KeyError, TypeError, OverflowError) as error:
                raise ValueError(f"Invalid residual measurements for {row['id']}: {error}") from error
        elif row["status"] == "failed" and row.get("failure"):
            failures[row["id"]] = row["failure"]
        else:
            raise ValueError(f"Invalid render status/failure: {row['id']}")
    return measurements, failures


def freeze_cohort(summary, contract_sha256, required_ids):
    measurements, failures = _validate_summary(summary, contract_sha256, required_ids)
    if not measurements:
        raise ValueError("Cannot freeze an empty rendered cohort")
    return dict(schema_version=SCHEMA_VERSION, contract_sha256=contract_sha256,
                required_case_ids=list(required_ids),
                case_ids=[case for case in required_ids if case in measurements],
                baseline_excluded_cases=failures, reference_rms_floor=REFERENCE_RMS_FLOOR,
                baseline_measurements=[measurements[case] for case in required_ids if case in measurements])


def _validate_cohort(cohort, contract_sha256, required_ids):
    if (cohort.get("schema_version") != SCHEMA_VERSION or
            cohort.get("contract_sha256") != contract_sha256 or
            cohort.get("required_case_ids") != list(required_ids) or
            cohort.get("reference_rms_floor") != REFERENCE_RMS_FLOOR):
        raise ValueError("Fixed cohort contract/schema mismatch")
    ids = cohort["case_ids"]
    excluded = cohort["baseline_excluded_cases"]
    if (not ids or len(ids) != len(set(ids)) or set(ids) & set(excluded) or
            set(ids) | set(excluded) != set(required_ids)):
        raise ValueError("Fixed cohort membership mismatch")
    return ids


def _aggregate(rows):
    mean = math.fsum(row["normalized_mse"] for row in rows) / len(rows)
    absolute = math.sqrt(math.fsum(row["error_rms"] ** 2 for row in rows) / len(rows))
    channels = [dict(id=row["id"], **channel) for row in rows for channel in row["channels"]]
    worst = max(channels, key=lambda channel: channel["normalized_mse"])
    return dict(equal_case_normalized_mse=mean,
                normalized_error_db=10*math.log10(mean) if mean else None,
                equal_case_error_rms=absolute,
                error_rms_dbfs=20*math.log10(absolute) if absolute else None,
                worst_channel=worst, worst_absolute_channel=max(channels, key=lambda channel: channel["error_rms"]))


def compare_runs(baseline, candidate, cohort, contract_sha256, required_ids):
    ids = _validate_cohort(cohort, contract_sha256, required_ids)
    before, before_failed = _validate_summary(baseline, contract_sha256, required_ids)
    after, after_failed = _validate_summary(candidate, contract_sha256, required_ids)
    # Freeze records all baseline successes. It cannot be hand-selected later.
    if set(before) != set(ids) or before_failed != cohort["baseline_excluded_cases"]:
        raise ValueError("Baseline no longer matches frozen cohort")
    if [before[case] for case in ids] != cohort["baseline_measurements"]:
        raise ValueError("Baseline measurements changed after cohort freeze")
    for case in set(before) & set(after):
        if (before[case]["reference_rms"] != after[case]["reference_rms"] or
                [c["reference_rms"] for c in before[case]["channels"]] !=
                [c["reference_rms"] for c in after[case]["channels"]]):
            raise ValueError(f"Reference/channel measurement mismatch: {case}")
    missing = [case for case in ids if case not in after]
    rows = []
    for case in ids:
        old = before[case]
        new = after.get(case)
        difference = new["normalized_mse"]-old["normalized_mse"] if new else None
        rows.append(dict(id=case, baseline=old, candidate=new, normalized_mse_change=difference,
                         error_rms_change=new["error_rms"]-old["error_rms"] if new else None,
                         outcome="missing" if new is None else "improved" if difference < 0 else
                         "regressed" if difference > 0 else "unchanged"))
    result = dict(schema_version=SCHEMA_VERSION, valid=not missing,
                  contract_sha256=contract_sha256, cohort_case_ids=ids,
                  coverage=dict(required=len(required_ids), cohort=len(ids),
                                baseline_rendered=len(before), candidate_rendered=len(after),
                                candidate_cohort_rendered=len(ids)-len(missing), missing_cohort_cases=missing,
                                baseline_failures=before_failed, candidate_failures=after_failed),
                  strict_primary_passed=dict(baseline=baseline["primary_passed"], candidate=candidate["primary_passed"]),
                  cases=rows, baseline_score=_aggregate([before[case] for case in ids]),
                  candidate_score=None, normalized_mse_change=None, reduction_percent=None,
                  newly_supported_cases=[after[case] for case in required_ids if case in after and case not in before],
                  acceptance_note="Progress score does not change strict sample-level acceptance or control equivalence.")
    if not missing:
        result["candidate_score"] = _aggregate([after[case] for case in ids])
        old = result["baseline_score"]["equal_case_normalized_mse"]
        new = result["candidate_score"]["equal_case_normalized_mse"]
        result["normalized_mse_change"] = new-old
        result["reduction_percent"] = 100*(old-new)/old if old else None
    return result
