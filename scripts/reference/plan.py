"""A deliberately bounded, 25-case diagnostic reference set."""

MAX_CASES = 25


def build(parameter_info=None):
    flat = {f"Band {i} enabled": "off" for i in range(1, 9)}
    base = {**flat, "Depth": "10", "Mode": "soft", "Detail": "5",
            "Attack": "5", "Release": "3", "Mix": "100",
            "Wet trim": "0", "Out trim": "0", "Delta": "off", "Bypass": "off"}
    cases = []

    def add(name, purpose, signal="resonant_harmonics", settings=None, **extra):
        cases.append(dict(id=f"{len(cases)+1:02d}_{name}", purpose=purpose,
                          signal=signal, settings={**base, **(settings or {})},
                          sample_rate=48000, channels=2, arrangement=3,
                          block_sizes=[256], process_mode=0, **extra))

    def event(second, parameter, value):
        return dict(frame=round(second * 48000), parameter=parameter, value=value)

    def band(**overrides):
        return {"Band 4 used": "on", "Band 4 enabled": "on", "Band 4 shape": "Bell",
                "Band 4 frequency": "3200", "Band 4 depth": "5", "Band 4 q": "2",
                **overrides}

    for mode in ["soft", "hard"]:
        for depth in [3, 10]:
            add(f"{mode}_depth_{depth}", f"{mode.title()} suppression at Depth {depth} on the same hot harmonics.",
                settings={"Mode": mode, "Depth": str(depth)})
    add("isolated_sine", "Response to a single 1056 Hz sinusoid.", "sine_1056hz")
    add("bandlimited_saw", "Preservation/suppression of an unmodified harmonic spectrum.", "saw_220hz")
    add("broad_detail_1", "Broad frequency selection on closely spaced peaks.", "two_close_tones", {"Detail": "1"})
    add("surgical_detail_10", "Narrow frequency selection on the same closely spaced peaks.", "two_close_tones", {"Detail": "10"})
    add("fast_timing", "Fast onset and recovery around repeated tone bursts.", "tone_bursts", {"Attack": "0", "Release": "0"})
    add("slow_timing", "Slow onset and recovery on the identical burst signal.", "tone_bursts", {"Attack": "10", "Release": "10"})
    add("frequency_tilts", "Contrasting low/high detail, attack and release multipliers.", "tone_bursts",
        {"Detail tilt low": ".1", "Detail tilt high": "10", "Attack tilt low": "10",
         "Attack tilt high": ".1", "Release tilt low": ".1", "Release tilt high": "10"})
    shapes = ["Low cut", "Low shelf", "Bell", "Bandpass", "Band reject", "Tilt", "High shelf", "High cut"]
    add("eight_band_shapes", "Eight shapes, each held for 2 seconds on the same repeated spectrum.",
        "shape_sequence", band(**{"Band 4 shape": shapes[0]}),
        automation=[event(.5 + 2*i, "Band 4 shape", shape) for i, shape in enumerate(shapes)],
        segments=[dict(start_seconds=.5+2*i, end_seconds=2.5+2*i, label=shape) for i, shape in enumerate(shapes)])
    add("bell_weighting_3200hz", "A bell increases sensitivity around 3.2 kHz.", settings=band())
    add("stereo_link_lr", "Linked L/R for the first half, independent L/R for the second.", "stereo_asymmetric",
        {"Stereo link": "100"}, automation=[event(3.5, "Stereo link", "0")])
    add("stereo_link_ms", "Linked mid/side followed by independent mid/side.", "stereo_mid_side",
        {"Stereo mode": "M/S", "Stereo link": "100"}, automation=[event(3.5, "Stereo link", "0")])
    add("global_and_band_focus", "Global focus changes sides, followed by per-band focus changes.", "stereo_asymmetric",
        {**band(), "Stereo link": "0", "Stereo focus": {"raw": 0}},
        automation=[event(2, "Stereo focus", {"raw": 1}), event(3.5, "Stereo focus", {"raw": .5}),
                    event(3.5, "Band 4 focus", {"raw": 0}), event(5, "Band 4 focus", {"raw": 1})])
    add("external_sidechain", "A separate, gated 3.2 kHz key drives suppression of the main input.", "sidechain_main",
        {"Sidechain": "on"}, sidechain_signal="sidechain_key")
    add("max_cut_3_to_12db", "Maximum reduction changes from 3 to 12 dB midway through.",
        settings={"Mode": "hard", "Max cut": "3"}, automation=[event(3.5, "Max cut", "12")])
    add("mix_and_gain_routing", "50% mix, +6 dB wet trim, -6 dB output trim; includes the mathematical difference too.",
        settings={"Mix": "50", "Wet trim": "6", "Out trim": "-6"}, delta_reconstructs_dry=False)
    add("linear_phase_high_quality", "Linear phase/high quality, compared with the normal minimum-phase baseline.",
        settings={"Linear phase": "on", "Quality": "high"})
    add("low_latency", "Zero/low-latency processing on the same diagnostic input.", settings={"Low latency": "on"})
    add("ultra_96khz", "Ultra quality at 96 kHz.", settings={"Quality": "ultra"})
    cases[-1]["sample_rate"] = 96000
    add("normal_44100hz", "Normal quality at 44.1 kHz.")
    cases[-1]["sample_rate"] = 44100
    add("surround_916", "16 distinct channels with group depth, linking, assignment and one bypassed channel.",
        "surround_identification", {**band(), "Band 4 channels": "surround", "Link all": "50",
                                     "Group link ceiling": "100", "Center depth": ".5", "Channel 4 enabled": "off"})
    cases[-1].update(channels=16, arrangement=50517759)
    add("depth_automation_3_to_10", "Depth changes from 3 to 10 at 3.5 seconds.",
        settings={"Depth": "3"}, automation=[event(3.5, "Depth", "10")])
    assert len(cases) == MAX_CASES
    return cases
