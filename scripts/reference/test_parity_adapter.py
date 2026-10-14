"""Adapter regressions use invented inventories, never proprietary binaries/audio."""
import math
import unittest

try:
    from . import plan
    from .parity_adapter import plan_case
except ImportError:
    import plan
    from parity_adapter import plan_case


def inventory():
    """Public parameter metadata matching the checked-in Lullaby declaration."""
    controls = [
        ("Depth", 0, 24), ("Detail", 0, 1), ("Attack", .1, 500), ("Release", 5, 2000),
        ("Max cut", 0, 36), ("Stereo link", 0, 1), ("Stereo focus", -1, 1),
        *[(f"{control} / {side}", -1, 1) for control in ("Detail", "Attack", "Release")
          for side in ("low", "high")],
        ("Mix", 0, 1), ("Wet trim", -24, 24), ("Output", -24, 24),
        ("Bypass", 0, 1), ("Delta", 0, 1), ("External sidechain", 0, 1),
        ("Listen to sidechain", 0, 1), ("Mode", 0, 1), ("Channels", 0, 1),
        ("Phase", 0, 1), ("Quality", 0, 3), ("Band audition", -1, 7),
        *[(title, minimum, maximum) for _ in range(8) for title, minimum, maximum in
          [("Enabled", 0, 1), ("Shape", 0, 7), ("Frequency", 20, 20000),
           ("Weight", -24, 24), ("Width", .1, 4), ("Focus", -1, 1)]],
    ]
    return [{"id": index + 100, "name": title, "flags": 0 if title in ("Quality", "Band audition") else 1,
             "mapping": [{"plain": minimum}, {"plain": maximum}]}
            for index, (title, minimum, maximum) in enumerate(controls)]


class AdapterTests(unittest.TestCase):
    def plan(self, settings=None, **extra):
        return plan_case({"settings": settings or {}, "sample_rate": 48000, "channels": 2, **extra}, inventory())

    def test_displayed_units_and_percentages_are_explicit(self):
        result = self.plan({"Depth": "3", "Mix": "50", "Stereo link": "25 %",
                            "Wet trim": "6 dB", "Out trim": "-6 dB"})
        self.assertEqual(result["host_settings"]["Depth"], "3")
        self.assertEqual(result["host_settings"]["Mix"], "0.5")
        self.assertEqual(result["host_settings"]["Stereo link"], "0.25")
        self.assertEqual(result["expected_parameters"]["Wet trim"]["plain"], 6)
        self.assertEqual(result["expected_parameters"]["Output"]["plain"], -6)
        self.assertNotIn("raw", result["host_settings"]["Depth"])

    def test_uncalibrated_controls_cannot_claim_support(self):
        result = self.plan({"Depth": "10", "Detail": "5", "Attack": "5", "Release": "3"})
        issues = {issue["source_parameter"]: issue for issue in result["unsupported"]}
        for title in ("Depth", "Detail", "Attack", "Release"):
            self.assertEqual(issues[title]["code"], "uncalibrated_scale")
            self.assertIn("exploratory_value", issues[title])
        self.assertEqual(result["expected_parameters"]["Detail"]["plain"], .5)
        self.assertAlmostEqual(result["expected_parameters"]["Attack"]["plain"], math.sqrt(.1 * 500))
        self.assertNotEqual(result["expected_parameters"]["Release"]["plain"], 3)

    def test_duplicate_band_names_select_the_requested_native_ids(self):
        result = self.plan({"Band 4 used": "on", "Band 4 enabled": "on", "Band 4 shape": "Bell",
                            "Band 4 frequency": "3200", "Band 4 depth": "5", "Band 4 q": "2"})
        metadata = inventory()
        enabled_ids = [f"#{p['id']}" for p in metadata if p["name"] == "Enabled"]
        self.assertEqual([result["host_settings"][key] for key in enabled_ids],
                         ["Off", "Off", "Off", "On", "Off", "Off", "Off", "Off"])
        frequency_id = [f"#{p['id']}" for p in metadata if p["name"] == "Frequency"][3]
        self.assertEqual(result["host_settings"][frequency_id], "3200")
        self.assertNotIn("Enabled", result["host_settings"])
        source_issues = {p["source_parameter"] for p in result["unsupported"]}
        self.assertIn("Band 4 depth", source_issues)
        self.assertIn("Band 4 q", source_issues)

    def test_shape_automation_uses_lullaby_enum_order_and_preserves_frames(self):
        case = plan.build()[11]
        result = plan_case(case, inventory())
        events = result["host_automation"]
        self.assertEqual([e["value"] for e in events], ["3", "1", "0", "5", "6", "7", "2", "4"])
        self.assertEqual([e["frame"] for e in events], [e["frame"] for e in case["automation"]])
        self.assertEqual(len({e["parameter"] for e in events}), 1)
        self.assertEqual(result["expected_parameters"][events[-1]["parameter"]]["plain"], 4)

    def test_focus_ratios_map_to_the_correct_side(self):
        result = self.plan({"Stereo focus": {"raw": 0}},
                           automation=[{"seconds": 2, "parameter": "Stereo focus", "value": {"raw": 1}}])
        self.assertEqual(result["host_settings"]["Stereo focus"], "-1")
        self.assertEqual(result["host_automation"], [{"frame": 96000, "parameter": "Stereo focus", "value": "1"}])
        self.assertEqual(result["expected_parameters"]["Stereo focus"]["plain"], 1)

    def test_delta_and_sidechain_use_real_plugin_monitor_controls(self):
        result = self.plan({"Delta": "on", "Sidechain": "on", "Sidechain listen": "off"})
        self.assertEqual(result["host_settings"]["Delta"], "On")
        self.assertEqual(result["host_settings"]["External sidechain"], "On")
        self.assertEqual(result["host_settings"]["Listen to sidechain"], "Off")
        self.assertEqual(result["host_settings"]["Band audition"], "-1")
        missing = [p for p in inventory() if p["name"] != "Delta"]
        self.assertFalse(plan_case({"settings": {}}, missing)["renderable"])

    def test_maximum_cut_off_is_a_failure_not_a_silent_finite_substitute(self):
        result = self.plan()
        self.assertEqual(result["host_settings"]["Max cut"], "36")
        self.assertTrue(any(p["code"] == "missing_unlimited_cut" for p in result["unsupported"]))
        limited = self.plan({"Max cut": "3"}, automation=[{"frame": 168000, "parameter": "Max cut", "value": "12"}])
        self.assertFalse(any(p["source_parameter"] == "Max cut" for p in limited["unsupported"]))
        self.assertEqual(limited["host_automation"][0]["value"], "12")

    def test_unknown_controls_and_normalized_depth_are_explicit_failures(self):
        result = self.plan({"Future feature": "on", "Depth": {"raw": .3}})
        issues = {p["source_parameter"]: p for p in result["unsupported"]}
        self.assertEqual(issues["Future feature"]["code"], "missing_feature")
        self.assertEqual(issues["Depth"]["code"], "unmappable_control")
        self.assertNotIn("Depth", result["host_settings"])

    def test_surround_is_never_downmixed_to_look_supported(self):
        case = plan.build()[23]
        original = dict(case)
        result = plan_case(case, inventory())
        self.assertFalse(result["renderable"])
        self.assertEqual(case, original)
        self.assertTrue(any(p["code"] == "unsupported_layout" for p in result["unsupported"]))
        self.assertTrue(any(p["source_parameter"] == "Band 4 channels" for p in result["unsupported"]))

    def test_all_twenty_five_cases_are_planned_and_have_support_failures(self):
        results = [plan_case(case, inventory()) for case in plan.build()]
        self.assertEqual(len(results), 25)
        self.assertEqual(sum(result["renderable"] for result in results), 24)
        self.assertTrue(all(result["unsupported"] for result in results))
        self.assertFalse(any(issue["code"] == "unmappable_control" for result in results for issue in result["unsupported"]))

    def test_malformed_inventory_and_values_do_not_silently_clamp(self):
        result = self.plan({"Mix": "101", "Attack": "nan", "Stereo focus": {"raw": float("nan")}})
        self.assertEqual(sum(p["code"] == "unmappable_control" for p in result["unsupported"]), 3)
        metadata = [p for p in inventory() if p["name"] != "Enabled"]
        missing = plan_case({"settings": {}}, metadata)
        self.assertTrue(any(p["source_parameter"] == "Band 1 enabled" for p in missing["unsupported"]))


if __name__ == "__main__":
    unittest.main()
