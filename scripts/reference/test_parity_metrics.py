"""Regression checks for the parity oracle; no plugin or captured files needed.

Run with: target/reference-venv/bin/python -m unittest discover \
    -s scripts/reference -p 'test_parity_metrics.py'
"""

import json
import math
import unittest

import numpy as np

try:
    from .parity_metrics import compare_audio
except ImportError:
    from parity_metrics import compare_audio


class ParityMetricsTests(unittest.TestCase):
    rate = 8000

    def setUp(self):
        time = np.arange(self.rate, dtype=np.float64) / self.rate
        self.reference = np.stack((0.1 * np.sin(2 * np.pi * 500 * time),
                                   0.05 * np.sin(2 * np.pi * 1500 * time)))
        self.dry = self.reference * 3

    def compare(self, actual, **kwargs):
        return compare_audio(self.reference, actual, self.dry, self.rate, **kwargs)

    def test_identical_nonzero_delta_has_exactly_silent_null(self):
        report = self.compare(self.reference.copy())
        primary = report["primary"]
        self.assertTrue(primary["exact_match"])
        self.assertEqual(primary["error_rms"], 0)
        self.assertEqual(primary["error_peak"], 0)
        self.assertIsNone(primary["error_rms_dbfs"])
        self.assertIsNone(primary["error_relative_db"])
        self.assertEqual(primary["error_relative_status"], "silent_error")
        self.assertTrue(all(channel["exact_match"] for channel in report["channels"]))
        self.assertEqual(report["diagnostics"]["worst_frequencies"], [])
        reference_peaks = report["diagnostics"]["dominant_reference_frequencies"]
        self.assertEqual(reference_peaks[0]["frequency_hz"], 500)
        self.assertEqual(reference_peaks[0]["actual_vs_reference_db"], 0)
        self.assertEqual(reference_peaks[0]["error_relative_status"], "silent_error")
        # This must never emit the invalid JSON tokens Infinity or NaN.
        json.dumps(report, allow_nan=False)

    def test_all_silence_is_well_defined_but_not_evidence_of_processing(self):
        zeros = np.zeros((1, 13))
        report = compare_audio(zeros, zeros, zeros, self.rate)
        self.assertTrue(report["primary"]["exact_match"])
        self.assertEqual(report["primary"]["reference_rms"], 0)
        self.assertIsNone(report["diagnostics"]["gain_fit"]["signed_gain"])
        self.assertIsNone(report["diagnostics"]["lag_estimate"])
        json.dumps(report, allow_nan=False)

    def test_nonzero_error_against_silent_reference_is_not_a_silent_null(self):
        zeros = np.zeros((1, 80))
        report = compare_audio(zeros, np.ones((1, 80)) * .1, zeros, self.rate)
        self.assertFalse(report["primary"]["exact_match"])
        self.assertAlmostEqual(report["primary"]["error_rms_dbfs"], -20)
        self.assertEqual(report["primary"]["error_relative_status"], "silent_reference")
        json.dumps(report, allow_nan=False)

    def test_polarity_inversion_has_plus_6db_relative_error(self):
        report = self.compare(-self.reference)
        self.assertAlmostEqual(report["primary"]["error_relative_db"], 20 * math.log10(2))
        self.assertTrue(report["diagnostics"]["gain_fit"]["polarity_inverted"])
        self.assertFalse(report["primary"]["exact_match"])
        self.assertGreater(report["primary"]["error_rms"], .05)

    def test_gain_difference_is_measured_without_normalization(self):
        report = self.compare(self.reference * .5)
        self.assertAlmostEqual(report["primary"]["error_relative_db"], 20 * math.log10(.5))
        self.assertAlmostEqual(report["diagnostics"]["gain_fit"]["signed_gain"], .5)
        self.assertFalse(report["primary"]["exact_match"])
        # The fitted residual is diagnostic only and cannot replace this error.
        self.assertGreater(report["primary"]["error_rms"], .02)
        self.assertFalse(report["diagnostics"]["gain_fit"]["used_for_acceptance"])

    def test_equal_magnitude_phase_error_is_reported_per_channel(self):
        time = np.arange(self.rate, dtype=np.float64) / self.rate
        reference = .1 * np.sin(2 * np.pi * 500 * time)
        actual = .1 * np.sin(2 * np.pi * 500 * time + .3)
        # Opposite phase shifts on different channels must remain separate.
        report = compare_audio(np.stack((reference, reference)),
                               np.stack((actual, .1 * np.sin(2 * np.pi * 500 * time - .3))),
                               np.stack((reference, reference)), self.rate)
        peak = report["diagnostics"]["dominant_reference_frequencies"][0]
        self.assertEqual(peak["frequency_hz"], 500)
        self.assertAlmostEqual(peak["actual_vs_reference_db"], 0, places=12)
        self.assertAlmostEqual(peak["channels"][0]["phase_difference_radians"], .3, places=12)
        self.assertAlmostEqual(peak["channels"][1]["phase_difference_radians"], -.3, places=12)
        self.assertAlmostEqual(peak["channels"][0]["phase_difference_degrees"], math.degrees(.3), places=10)
        self.assertTrue(peak["channels"][0]["phase_defined"])
        self.assertFalse(report["primary"]["exact_match"])
        self.assertGreater(report["primary"]["error_rms"], .02)
        json.dumps(report, allow_nan=False)

    def test_phase_at_a_silent_channel_is_undefined(self):
        reference = np.stack((self.reference[0], np.zeros(self.rate)))
        report = compare_audio(reference, reference, reference, self.rate)
        channel = report["diagnostics"]["dominant_reference_frequencies"][0]["channels"][1]
        self.assertFalse(channel["phase_defined"])
        self.assertIsNone(channel["phase_difference_radians"])
        self.assertIsNone(channel["phase_difference_degrees"])
        json.dumps(report, allow_nan=False)

    def test_timing_error_is_not_aligned_away(self):
        reference = np.zeros((1, 4000))
        reference[0, [700, 1100, 1701]] = [.2, .1, -.15]
        delayed = np.zeros_like(reference)
        delayed[:, 7:] = reference[:, :-7]
        report = compare_audio(reference, delayed, reference, self.rate)
        self.assertFalse(report["primary"]["exact_match"])
        self.assertAlmostEqual(report["primary"]["error_relative_db"], 20 * math.log10(math.sqrt(2)))
        self.assertEqual(report["diagnostics"]["lag_estimate"]["estimated_actual_delay_frames"], 7)
        self.assertFalse(report["diagnostics"]["lag_estimate"]["used_for_acceptance"])

    def test_swapped_channels_do_not_cancel_in_downmix(self):
        actual = self.reference[::-1].copy()
        # The stereo sums are identical; a downmixed oracle would miss the fault.
        np.testing.assert_array_equal(actual.sum(axis=0), self.reference.sum(axis=0))
        report = self.compare(actual)
        self.assertGreater(report["primary"]["error_rms"], .05)
        self.assertTrue(all(channel["error_rms"] > .05 for channel in report["channels"]))
        self.assertFalse(report["primary"]["exact_match"])

    def test_time_envelopes_preserve_channel_changes_with_equal_aggregate_energy(self):
        reference = np.stack((np.full(160, .1), np.full(160, .3)))
        actual = reference[::-1].copy()
        report = compare_audio(reference, actual, reference * 3, self.rate)
        for window in report["time_envelope"]:
            self.assertAlmostEqual(window["reference_rms"], window["actual_rms"])
            self.assertEqual(len(window["channels"]), 2)
            left, right = window["channels"]
            self.assertEqual(left["channel"], 0)
            self.assertAlmostEqual(left["reference_rms"], .1)
            self.assertAlmostEqual(left["actual_rms"], .3)
            self.assertAlmostEqual(left["error_rms"], .2)
            self.assertAlmostEqual(left["dry_rms"], .3)
            self.assertAlmostEqual(left["reference_rms_dbfs"], -20)
            self.assertAlmostEqual(left["actual_rms_dbfs"], 20 * math.log10(.3))
            self.assertAlmostEqual(right["reference_rms"], .3)
            self.assertAlmostEqual(right["actual_rms"], .1)
            self.assertAlmostEqual(right["error_rms_dbfs"], 20 * math.log10(.2))
        json.dumps(report, allow_nan=False)

    def test_one_bad_surround_channel_cannot_be_hidden_by_15_silent_channels(self):
        reference = np.zeros((16, 800))
        reference[11] = .1
        actual = reference.copy()
        actual[11] = 0
        report = compare_audio(reference, actual, reference, self.rate)
        self.assertAlmostEqual(report["channels"][11]["error_rms_dbfs"], -20)
        self.assertEqual(report["diagnostics"]["worst_channel"], 11)
        self.assertGreater(report["channels"][11]["error_rms"], report["primary"]["error_rms"])
        self.assertEqual(sum(not row["exact_match"] for row in report["channels"]), 1)

    def test_local_time_error_is_localized_and_segmented(self):
        actual = self.reference.copy()
        actual[1, 3200:3280] += .25
        report = self.compare(actual, segments=[dict(label="first half", start_seconds=0, end_seconds=.5),
                                               dict(label="second half", start_seconds=.5, end_seconds=1)])
        worst = report["diagnostics"]["worst_window"]
        self.assertEqual(worst["start_frame"], 3200)
        self.assertEqual(worst["end_frame"], 3280)
        self.assertEqual(worst["worst_channel"], 1)
        self.assertAlmostEqual(worst["worst_channel_error_rms_dbfs"], 20 * math.log10(.25))
        self.assertFalse(report["segments"][0]["exact_match"])
        self.assertTrue(report["segments"][1]["exact_match"])

    def test_tail_and_lead_errors_are_not_excluded(self):
        dry = np.zeros((1, 800))
        dry[:, 100:700] = .1
        reference = dry * .5
        actual = reference.copy()
        actual[0, 20] = .2
        actual[0, 750] = .3
        report = compare_audio(reference, actual, dry, self.rate)
        self.assertEqual(report["lead"]["end_frame"], 100)
        self.assertEqual(report["tail"]["start_frame"], 700)
        self.assertEqual(report["lead"]["error_peak"], .2)
        self.assertEqual(report["tail"]["error_peak"], .3)
        self.assertFalse(report["primary"]["exact_match"])

    def test_bands_preserve_parseval_energy_and_locate_frequency(self):
        actual = self.reference.copy()
        actual[0] += .03 * np.sin(2 * np.pi * 2000 * np.arange(self.rate) / self.rate)
        report = self.compare(actual)
        spectral_power = sum(row["error_rms"] ** 2 for row in report["spectrum_bands"])
        self.assertAlmostEqual(spectral_power, report["primary"]["error_rms"] ** 2, places=14)
        self.assertAlmostEqual(report["diagnostics"]["worst_frequencies"][0]["frequency_hz"], 2000)
        worst_band = max(report["spectrum_bands"], key=lambda row: row["error_rms"])
        self.assertEqual(worst_band["low_hz"], 1250)
        self.assertEqual(worst_band["high_hz"], 2500)

    def test_partial_envelope_window_and_short_fft_are_supported(self):
        reference = np.ones((1, 81)) * .1
        actual = reference.copy()
        actual[0, -1] += .2
        report = compare_audio(reference, actual, reference, self.rate)
        self.assertEqual(len(report["time_envelope"]), 2)
        self.assertEqual(report["time_envelope"][-1]["end_frame"], 81)
        self.assertAlmostEqual(report["time_envelope"][-1]["error_rms"], .2)
        json.dumps(report, allow_nan=False)

    def test_dc_and_nyquist_energy_is_not_double_counted(self):
        reference = np.zeros((1, 800))
        actual = .1 + .2 * (-1.0) ** np.arange(800)[None, :]
        report = compare_audio(reference, actual, reference, self.rate)
        spectral_power = sum(row["error_rms"] ** 2 for row in report["spectrum_bands"])
        self.assertAlmostEqual(spectral_power, .1 ** 2 + .2 ** 2, places=14)
        self.assertAlmostEqual(spectral_power, report["primary"]["error_rms"] ** 2, places=14)

    def test_float32_difference_is_computed_without_subtraction_rounding(self):
        reference = np.array([[1.0, 1e-9]], dtype=np.float32)
        actual = np.array([[1e-9, 1.0]], dtype=np.float32)
        report = compare_audio(reference, actual, reference, self.rate)
        expected_peak = float(reference[0, 0]) - float(reference[0, 1])
        self.assertEqual(report["primary"]["error_peak"], expected_peak)

    def test_invalid_inputs_fail_before_metrics(self):
        for actual in (np.zeros((1, 8000)), np.zeros(8000), np.full((2, 8000), np.nan),
                       np.full((2, 8000), np.inf), np.zeros((2, 8000), dtype=complex)):
            with self.subTest(shape=actual.shape, dtype=actual.dtype):
                with self.assertRaises(ValueError):
                    self.compare(actual)
        for rate in (0, -1, float("nan"), float("inf"), True):
            with self.subTest(rate=rate):
                with self.assertRaises(ValueError):
                    compare_audio(self.reference, self.reference, self.dry, rate)
        for segment in (dict(start_seconds=-.1, end_seconds=.5),
                        dict(start_seconds=0, end_seconds=2),
                        dict(start_seconds=.5, end_seconds=.5)):
            with self.subTest(segment=segment):
                with self.assertRaises(ValueError):
                    self.compare(self.reference, segments=[segment])


if __name__ == "__main__":
    unittest.main()
