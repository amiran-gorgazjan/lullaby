"""Synthetic guards for comparable, fixed-coverage progress measurements."""
import copy
import math
import unittest

from reference.parity_score import compare_runs, freeze_cohort


class ParityScoreTests(unittest.TestCase):
    ids = ["quiet", "loud", "unsupported"]
    digest = "synthetic-contract"

    def summary(self):
        def row(name, reference, error):
            measurement = dict(reference_rms=reference, error_rms=error)
            return dict(id=name, status="rendered", failure=None, primary=measurement,
                        channels=[dict(channel=0, **measurement), dict(channel=1, **measurement)])
        return dict(contract_sha256=self.digest, full_corpus=True, cases=3, primary_passed=0,
                    cases_summary=[row("quiet", .001, .001), row("loud", .5, .25),
                                   dict(id="unsupported", status="failed", failure="unsupported layout")])

    def compare(self, baseline, candidate, cohort=None):
        return compare_runs(baseline, candidate, cohort or freeze_cohort(baseline, self.digest, self.ids), self.digest, self.ids)

    def test_equal_case_normalized_energy_and_absolute_rms(self):
        baseline = self.summary()
        result = self.compare(baseline, copy.deepcopy(baseline))
        self.assertTrue(result["valid"])
        self.assertEqual(result["baseline_score"]["equal_case_normalized_mse"], .625)
        self.assertAlmostEqual(result["baseline_score"]["equal_case_error_rms"], math.sqrt((.001**2+.25**2)/2))
        self.assertEqual(result["reduction_percent"], 0)
        self.assertEqual(result["coverage"]["candidate_rendered"], 2)
        self.assertIn("unsupported", result["coverage"]["candidate_failures"])

    def test_improvement_records_each_case_without_hiding_regression(self):
        baseline = self.summary()
        candidate = copy.deepcopy(baseline)
        candidate["cases_summary"][0]["primary"]["error_rms"] *= .5
        for channel in candidate["cases_summary"][0]["channels"]:
            channel["error_rms"] *= .5
        candidate["cases_summary"][1]["primary"]["error_rms"] *= 1.1
        for channel in candidate["cases_summary"][1]["channels"]:
            channel["error_rms"] *= 1.1
        result = self.compare(baseline, candidate)
        self.assertGreater(result["reduction_percent"], 0)
        self.assertEqual([row["outcome"] for row in result["cases"]], ["improved", "regressed"])

    def test_missing_render_invalidates_score_instead_of_changing_cohort(self):
        baseline = self.summary()
        candidate = copy.deepcopy(baseline)
        candidate["cases_summary"][0] = dict(id="quiet", status="failed", failure="crash")
        result = self.compare(baseline, candidate)
        self.assertFalse(result["valid"])
        self.assertIsNone(result["candidate_score"])
        self.assertIsNone(result["reduction_percent"])
        self.assertEqual(result["coverage"]["missing_cohort_cases"], ["quiet"])

    def test_missing_extra_duplicate_or_partial_case_membership_is_rejected(self):
        for alteration in ("missing", "extra", "duplicate", "partial"):
            candidate = self.summary()
            if alteration == "missing": candidate["cases_summary"].pop()
            if alteration == "extra": candidate["cases_summary"].append(dict(id="extra"))
            if alteration == "duplicate": candidate["cases_summary"][1] = candidate["cases_summary"][0]
            if alteration == "partial": candidate["full_corpus"] = False
            with self.subTest(alteration=alteration), self.assertRaises(ValueError):
                self.compare(self.summary(), candidate)

    def test_nonfinite_negative_boolean_or_inconsistent_measurements_are_rejected(self):
        for value in (float("nan"), float("inf"), -.1, True, "0.1"):
            candidate = self.summary()
            candidate["cases_summary"][0]["primary"]["error_rms"] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                self.compare(self.summary(), candidate)
        candidate = self.summary()
        candidate["cases_summary"][0]["channels"][1]["error_rms"] *= 2
        with self.assertRaisesRegex(ValueError, "Inconsistent"):
            self.compare(self.summary(), candidate)

    def test_contract_mismatch_and_cohort_tampering_are_rejected(self):
        baseline = self.summary()
        cohort = freeze_cohort(baseline, self.digest, self.ids)
        candidate = copy.deepcopy(baseline)
        candidate["contract_sha256"] = "other"
        with self.assertRaisesRegex(ValueError, "contract mismatch"):
            self.compare(baseline, candidate, cohort)
        cohort["case_ids"].remove("quiet")
        cohort["baseline_excluded_cases"]["quiet"] = "excluded"
        with self.assertRaisesRegex(ValueError, "Baseline no longer"):
            self.compare(baseline, baseline, cohort)

    def test_frozen_baseline_or_reference_changes_are_rejected(self):
        baseline = self.summary()
        cohort = freeze_cohort(baseline, self.digest, self.ids)
        baseline["cases_summary"][0]["primary"]["error_rms"] *= 2
        for channel in baseline["cases_summary"][0]["channels"]:
            channel["error_rms"] *= 2
        with self.assertRaisesRegex(ValueError, "measurements changed"):
            self.compare(baseline, baseline, cohort)
        candidate = self.summary()
        candidate["cases_summary"][0]["primary"]["reference_rms"] *= 2
        for channel in candidate["cases_summary"][0]["channels"]:
            channel["reference_rms"] *= 2
        with self.assertRaisesRegex(ValueError, "Reference/channel"):
            self.compare(self.summary(), candidate)

    def test_newly_supported_case_is_separate_from_fixed_score(self):
        baseline = self.summary()
        candidate = copy.deepcopy(baseline)
        candidate["cases_summary"][2] = copy.deepcopy(candidate["cases_summary"][0])
        candidate["cases_summary"][2]["id"] = "unsupported"
        result = self.compare(baseline, candidate)
        self.assertEqual(result["reduction_percent"], 0)
        self.assertEqual(result["coverage"]["candidate_rendered"], 3)
        self.assertEqual(result["newly_supported_cases"][0]["id"], "unsupported")

    def test_silent_reference_and_exact_zero_have_finite_json_scores(self):
        baseline = self.summary()
        baseline["cases_summary"][0]["primary"]["reference_rms"] = 0
        for channel in baseline["cases_summary"][0]["channels"]:
            channel["reference_rms"] = 0
        result = self.compare(baseline, baseline)
        self.assertTrue(math.isfinite(result["baseline_score"]["equal_case_normalized_mse"]))
        for row in baseline["cases_summary"][:2]:
            row["primary"]["error_rms"] = 0
            for channel in row["channels"]: channel["error_rms"] = 0
        result = self.compare(baseline, baseline)
        self.assertEqual(result["baseline_score"]["equal_case_normalized_mse"], 0)
        self.assertIsNone(result["baseline_score"]["normalized_error_db"])
        self.assertIsNone(result["reduction_percent"])


if __name__ == "__main__":
    unittest.main()
