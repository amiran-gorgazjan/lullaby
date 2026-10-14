"""Test the acceptance harness itself without licensed plugins or local audio."""
import copy
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

import numpy as np

# Both `unittest discover -s scripts` and `-s scripts/reference` are supported.
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from reference.parity_metrics import compare_audio
from reference_capture import sha, wav_write
from reference_parity import (align_output, comparison_segments, gain_routing_metrics, level_failures,
                              load_case, load_contract, make_suite,
                              null_failures, parameter_failures, verify_corpus)


class ParityRunnerTests(unittest.TestCase):
    def setUp(self):
        self.contract = load_contract()
        rate = 8000
        time = np.arange(rate) / rate
        self.audio = np.stack((.1*np.sin(2*np.pi*431*time), .05*np.sin(2*np.pi*873*time)))
        self.identity = compare_audio(self.audio, self.audio.copy(), self.audio, rate)

    def run_suite(self, reports):
        return unittest.TextTestRunner(stream=io.StringIO()).run(make_suite(reports, self.contract))

    def identity_report(self, case_id):
        return dict(id=case_id, status="rendered", failure=None,
                    mapping=dict(unsupported=[]), parameter_failures=[],
                    delta_comparison=self.identity, wet_comparison=self.identity,
                    routing=dict(unity_reconstruction_applicable=True, reconstruction_peak=0.0))

    def test_exact_identity_passes_all_primary_and_supportive_tests(self):
        reports = [self.identity_report(entry["id"]) for entry in self.contract["cases"]]
        suite = make_suite(reports, self.contract)
        primary = [test.id() for test in suite if test.id().endswith(".test_delta_null")]
        self.assertEqual(len(primary), 25)
        self.assertEqual(len(set(primary)), 25)
        result = self.run_suite(reports)
        self.assertTrue(result.wasSuccessful())
        self.assertEqual(result.testsRun, 175)
        self.assertEqual(result.skipped, [])

    def test_missing_or_unsupported_render_stays_a_failed_primary_test(self):
        report = dict(id="24_surround_916", status="failed", failure="unsupported 16-channel layout", parameter_failures=[])
        result = self.run_suite([report])
        self.assertFalse(result.wasSuccessful())
        failures = [test.id() for test, _ in result.failures]
        self.assertTrue(any(name.endswith(".test_delta_null") for name in failures))
        self.assertEqual(result.errors, [])
        self.assertEqual(result.skipped, [])

    def test_exact_null_cannot_hide_unimplemented_controls(self):
        report = self.identity_report("synthetic_identity")
        report["mapping"]["unsupported"] = [dict(code="uncalibrated_scale", reason="Unknown timing law")]
        result = self.run_suite([report])
        self.assertEqual(len(result.failures), 1)
        self.assertTrue(result.failures[0][0].id().endswith(".test_control_equivalence"))

    def test_zero_output_gain_shift_polarity_and_channel_swap_fail_null(self):
        for candidate in [np.zeros_like(self.audio), self.audio*1.01, -self.audio,
                          np.roll(self.audio, 1, axis=1), self.audio[::-1]]:
            with self.subTest(candidate_peak=float(np.max(np.abs(candidate)))):
                comparison = compare_audio(self.audio, candidate, self.audio, 8000)
                self.assertTrue(null_failures(comparison, self.contract["primary_limits"]))

    def test_per_channel_error_cannot_be_diluted_by_surround_channels(self):
        source = np.tile(self.audio[:1], (16, 1))
        candidate = source.copy()
        candidate[7] *= 1.006
        comparison = compare_audio(source, candidate, source, 8000)
        errors = null_failures(comparison, self.contract["primary_limits"])
        self.assertTrue(any("channel 7" in error for error in errors))

    def test_short_click_is_caught_even_if_full_file_rms_is_low(self):
        candidate = self.audio.copy()
        candidate[1, 1000] += .002
        comparison = compare_audio(self.audio, candidate, self.audio, 8000)
        errors = null_failures(comparison, self.contract["primary_limits"])
        self.assertTrue(any("error_peak_dbfs" in error for error in errors))

    def test_silent_reference_uses_absolute_limits_without_division_by_zero(self):
        zero = np.zeros_like(self.audio)
        self.assertFalse(null_failures(compare_audio(zero, zero, self.audio, 8000), self.contract["primary_limits"]))
        self.assertTrue(null_failures(compare_audio(zero, self.audio, self.audio, 8000), self.contract["primary_limits"]))

    def test_level_support_detects_added_energy_and_gain_error(self):
        self.assertTrue(level_failures([dict(reference_rms=0, actual_rms=.1)], 1, -80))
        self.assertTrue(level_failures([dict(reference_rms=.1, actual_rms=.12)], 1, -80))
        self.assertFalse(level_failures([dict(reference_rms=.1, actual_rms=.1)], 1, -80))

    def test_mixed_gain_routing_checks_native_delta_branch_not_just_readback(self):
        unity_wet, unity_delta = .7*self.audio, .3*self.audio
        mix, wet_gain, output_gain = .5, 10**(6/20), 10**(-6/20)
        wet = output_gain*((1-mix)*self.audio + mix*wet_gain*unity_wet)
        delta = output_gain*mix*wet_gain*unity_delta
        report = gain_routing_metrics(self.audio, wet, delta, unity_wet, unity_delta, mix, wet_gain, output_gain)
        self.assertLess(report["wet_gain_routing_peak"], 1e-12)
        self.assertLess(report["delta_gain_routing_peak"], 1e-12)
        # A native Delta branch omitting mix and wet trim must not pass just
        # because its switches and trims read back correctly.
        wrong = gain_routing_metrics(self.audio, wet, output_gain*unity_delta,
                                     unity_wet, unity_delta, mix, wet_gain, output_gain)
        self.assertGreater(wrong["delta_gain_routing_peak"], 3e-6)

    def test_alignment_uses_reported_latency_only_and_rejects_truncation(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "render.pcm"
            signal = np.arange(10, dtype="<f4")[None]
            signal.tofile(path)
            result = dict(frames=10, latency_samples=3, latency_changes=[dict(frame=0, samples=3)])
            _, aligned = align_output(path, result, 1, 5)
            np.testing.assert_array_equal(aligned, signal[:, 3:8])
            with self.assertRaises(ValueError):
                align_output(path, result, 1, 8)
            changed = {**result, "latency_changes": [dict(frame=0, samples=3), dict(frame=5, samples=4)]}
            with self.assertRaises(ValueError):
                align_output(path, changed, 1, 5)

    def test_parameter_readback_uses_ids_and_verifies_native_delta(self):
        host = dict(parameters=[dict(id=1, name="Enabled", plain=0), dict(id=2, name="Enabled", plain=1),
                                dict(id=3, name="Delta", plain=1)])
        expectations = {"#2": dict(name="Enabled", plain=1), "Delta": dict(name="Delta", plain=0)}
        self.assertFalse(parameter_failures(host, expectations, delta=True))
        self.assertTrue(parameter_failures(host, expectations, delta=False))
        self.assertTrue(parameter_failures(host, {"Enabled": dict(name="Enabled", plain=1)}, delta=True))

    def test_automation_diagnostics_preserve_requested_boundaries(self):
        record = dict(frames=384000, sample_rate=48000, automation=[dict(frame=168000)])
        segments = comparison_segments(record)
        self.assertIn(3.5, [segment["end_seconds"] for segment in segments])
        self.assertEqual(segments[0]["start_seconds"], 0)
        self.assertEqual(segments[-1]["end_seconds"], 8)

    def make_reference_fixture(self, root):
        folder = root / "cases/fixture"
        folder.mkdir(parents=True)
        data = np.full((2, 32), .1, dtype="<f4")
        files = {}
        for name in ("dry", "wet", "delta"):
            wav_write(folder / f"{name}.wav", data, 8000)
            files[name] = dict(path=f"{name}.wav", sha256=sha(folder / f"{name}.wav"))
        record = dict(id="fixture", channels=2, frames=32, sample_rate=8000)
        (folder / "capture.json").write_text(json.dumps(record))
        entry = dict(id="fixture", path="cases/fixture", capture_sha256=sha(folder / "capture.json"), files=files, states={})
        return folder, entry

    def test_wav_and_metadata_tampering_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            folder, entry = self.make_reference_fixture(root)
            record, _ = load_case(root, entry)
            self.assertEqual(record["frames"], 32)
            original = (folder / "delta.wav").read_bytes()
            (folder / "delta.wav").write_bytes(original[:-1]+bytes([original[-1] ^ 1]))
            with self.assertRaisesRegex(ValueError, "Reviewed WAV changed"):
                load_case(root, entry)
            (folder / "delta.wav").write_bytes(original)
            (folder / "capture.json").write_text('{}')
            with self.assertRaisesRegex(ValueError, "Reviewed capture metadata changed"):
                load_case(root, entry)

    def test_wrong_reference_shape_fails_even_with_updated_fixture_hash(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            folder, entry = self.make_reference_fixture(root)
            wav_write(folder / "delta.wav", np.zeros((1, 32)), 8000)
            entry["files"]["delta"]["sha256"] = sha(folder / "delta.wav")
            with self.assertRaisesRegex(ValueError, "Invalid reference WAV"):
                load_case(root, entry)

    def test_changed_corpus_membership_fails_instead_of_reducing_coverage(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "cases").mkdir()
            (root / "index.json").write_text('[]')
            (root / "capture-environment.json").write_text('{}')
            contract = dict(index_sha256=sha(root / "index.json"),
                            environment_sha256=sha(root / "capture-environment.json"), cases=[dict(id="required")])
            with self.assertRaisesRegex(ValueError, "Corpus membership changed"):
                verify_corpus(root, contract)
            (root / "cases/required").mkdir()
            verify_corpus(root, contract)
            (root / "cases/extra").mkdir()
            with self.assertRaisesRegex(ValueError, "Corpus membership changed"):
                verify_corpus(root, contract)

    def test_contract_cannot_drop_cases(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "contract.json"
            contract = copy.deepcopy(self.contract)
            contract["cases"].pop()
            path.write_text(json.dumps(contract))
            with self.assertRaisesRegex(ValueError, "exactly 25"):
                load_contract(path)


if __name__ == "__main__":
    unittest.main()
