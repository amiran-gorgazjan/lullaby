import importlib.util
import pathlib
import tempfile
import unittest
import array
import subprocess
import sys

spec = importlib.util.spec_from_file_location("performance", pathlib.Path(__file__).with_name("performance.py"))
performance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(performance)


class PerformanceTests(unittest.TestCase):
    def trials(self, ratios):
        def probe(values):
            return {"cases":[{"case":name,"ns_per_sample":value,"realtime_percent":value/100}
                for name,value in zip(["default_stereo_48k","sidechain"],values)]}
        return [[probe([100,100]),probe([100*a,100*b])] for a,b in ratios]

    def test_clear_gain_passes(self):
        self.assertTrue(performance.evaluate(self.trials([(.8,.85)]*9))["performance_pass"])

    def test_tiny_gain_rejected(self):
        self.assertFalse(performance.evaluate(self.trials([(.98,.99)]*9))["performance_pass"])

    def test_noisy_gain_rejected(self):
        self.assertFalse(performance.evaluate(self.trials([(.7,.7),(1.3,1.3)]*5))["performance_pass"])

    def test_credible_regression_rejected(self):
        result = performance.evaluate(self.trials([(.7,1.1)]*9))
        self.assertFalse(result["performance_pass"])
        self.assertEqual(result["credible_regressions"],["sidechain"])

    def test_audio_gate_rejects_changed_sound_and_nonfinite_values(self):
        with tempfile.TemporaryDirectory() as tmp:
            a,b = pathlib.Path(tmp)/"a",pathlib.Path(tmp)/"b"
            a.write_bytes(array.array("f",[.1,.2,.3]).tobytes())
            b.write_bytes(a.read_bytes())
            self.assertTrue(performance.compare_audio(a,b)["audio_pass"])
            b.write_bytes(array.array("f",[.1,.2,.31]).tobytes())
            self.assertFalse(performance.compare_audio(a,b)["audio_pass"])
            b.write_bytes(array.array("f",[float("nan"),.2,.3]).tobytes())
            with self.assertRaises(ValueError): performance.compare_audio(a,b)

    def test_native_probe_requires_both_plugin_paths(self):
        command = [sys.executable, str(pathlib.Path(performance.__file__)),
                   "--baseline", "unused", "--candidate", "unused", "--output", "unused",
                   "--baseline-plugin", "unused"]
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn("both plugin paths are required together", result.stderr)


if __name__ == "__main__":
    unittest.main()
