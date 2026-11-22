"""Minimal native CLAP host smoke test; Python standard library only.

Run after `cargo xtask bundle lullaby --release`. Exercises the actual shared
library, parameter text and events, state streams, latency and variable blocks.
This is not a substitute for a full CLAP validator or interactive DAW testing.
"""
import ctypes as C
import math
from pathlib import Path
import sys

P = C.c_void_p
U = C.c_uint32
B = C.c_bool
F = C.CFUNCTYPE
PARAM_IS_STEPPED = 1 << 0
PARAM_IS_BYPASS = 1 << 4
PARAM_IS_AUTOMATABLE = 1 << 5


class Version(C.Structure):
    _fields_ = [("major", U), ("minor", U), ("revision", U)]


class Entry(C.Structure):
    _fields_ = [("version", Version), ("init", F(B, C.c_char_p)),
                ("deinit", F(None)), ("factory", F(P, C.c_char_p))]


class Factory(C.Structure):
    _fields_ = [("count", F(U, P)), ("descriptor", F(P, P, U)),
                ("create", F(P, P, P, C.c_char_p))]


class Plugin(C.Structure):
    _fields_ = [("desc", P), ("data", P), ("init", F(B, P)),
                ("destroy", F(None, P)), ("activate", F(B, P, C.c_double, U, U)),
                ("deactivate", F(None, P)), ("start", F(B, P)),
                ("stop", F(None, P)), ("reset", F(None, P)),
                ("process", F(C.c_int32, P, P)),
                ("extension", F(P, P, C.c_char_p)), ("main_thread", F(None, P))]


class Host(C.Structure):
    _fields_ = [("version", Version), ("data", P), ("name", C.c_char_p),
                ("vendor", C.c_char_p), ("url", C.c_char_p), ("host_version", C.c_char_p),
                ("extension", F(P, P, C.c_char_p)), ("restart", F(None, P)),
                ("request_process", F(None, P)), ("callback", F(None, P))]


class Audio(C.Structure):
    _fields_ = [("data32", C.POINTER(C.POINTER(C.c_float))), ("data64", P),
                ("channels", U), ("latency", U), ("constant_mask", C.c_uint64)]


class Process(C.Structure):
    _fields_ = [("time", C.c_int64), ("frames", U), ("transport", P),
                ("inputs", C.POINTER(Audio)), ("outputs", C.POINTER(Audio)),
                ("input_count", U), ("output_count", U), ("events_in", P), ("events_out", P)]


class Events(C.Structure):
    _fields_ = [("ctx", P), ("size", F(U, P)), ("get", F(P, P, U))]


class OutputEvents(C.Structure):
    _fields_ = [("ctx", P), ("push", F(B, P, P))]


class Header(C.Structure):
    _fields_ = [("size", U), ("time", U), ("space", C.c_uint16),
                ("type", C.c_uint16), ("flags", U)]


class ParamEvent(C.Structure):
    _fields_ = [("header", Header), ("id", U), ("cookie", P), ("note", C.c_int32),
                ("port", C.c_int16), ("channel", C.c_int16), ("key", C.c_int16),
                ("value", C.c_double)]


class ParamInfo(C.Structure):
    _fields_ = [("id", U), ("flags", U), ("cookie", P), ("name", C.c_char * 256),
                ("module", C.c_char * 1024), ("min", C.c_double),
                ("max", C.c_double), ("default", C.c_double)]


class Params(C.Structure):
    _fields_ = [("count", F(U, P)), ("info", F(B, P, U, C.POINTER(ParamInfo))),
                ("get", F(B, P, U, C.POINTER(C.c_double))),
                ("to_text", F(B, P, U, C.c_double, C.POINTER(C.c_char), U)),
                ("from_text", F(B, P, U, C.c_char_p, C.POINTER(C.c_double))),
                ("flush", F(None, P, P, P))]


class State(C.Structure):
    _fields_ = [("save", F(B, P, P)), ("load", F(B, P, P))]


class Stream(C.Structure):
    _fields_ = [("ctx", P), ("transfer", F(C.c_int64, P, P, C.c_uint64))]


class Latency(C.Structure):
    _fields_ = [("get", F(U, P))]


def check_parameter_text(params, ptr, parameters):
    def display(info, value):
        text = C.create_string_buffer(128)
        assert params.to_text(ptr, info.id, value, text, len(text)), (info.name, value)
        return text.value

    def parse(info, text):
        value = C.c_double()
        if params.from_text(ptr, info.id, text, C.byref(value)):
            return value.value
        return None

    for info, value, expected_text, tolerance in (
        (parameters[(b"", b"Depth")], 0.32147, b"3.21", 0.0005),
        (parameters[(b"", b"Max cut")], 0.5, b"12.00 dB", 1e-6),
        (parameters[(b"", b"Max cut")], 1.0, b"Off", 1e-6),
        (parameters[(b"Band 1", b"Frequency")], None, b"100.0 Hz", 1e-6),
        (parameters[(b"Band 8", b"Frequency")], None, b"12.0 kHz", 1e-6),
    ):
        if value is None:
            value = info.default
        text = display(info, value)
        assert text == expected_text, (info.name, text, expected_text)
        parsed = parse(info, text)
        assert parsed is not None and math.isclose(parsed, value, abs_tol=tolerance), (
            info.name, text, parsed
        )

    unreadable = []
    for name, labels in (
        (b"Mode", (b"Soft", b"Hard")),
        (b"Channels", (b"L/R", b"M/S")),
        (b"Phase", (b"Minimum", b"Linear")),
        (b"Quality", (b"Normal", b"High", b"Ultra", b"Low latency")),
    ):
        info = parameters[(b"", name)]
        assert (info.min, info.max, info.default) == (0, len(labels) - 1, 0), name
        assert info.flags & PARAM_IS_STEPPED, name
        assert bool(info.flags & PARAM_IS_AUTOMATABLE) == (name != b"Quality"), name

        for value, label in enumerate(labels):
            numeric = parse(info, f" {value} ".encode())
            assert numeric is not None and math.isclose(numeric, value, abs_tol=1e-6), (
                name, value, numeric
            )
            text = display(info, value)
            assert text == label, (name, value, text, label)
            for editable_text in (text, b" " + text.swapcase() + b"\t"):
                parsed = parse(info, editable_text)
                if parsed is None or not math.isclose(parsed, value, abs_tol=1e-6):
                    unreadable.append(
                        f"{name.decode()} {editable_text.decode()!r}: expected {value}, got {parsed}"
                    )
        assert parse(info, b"not a parameter value") is None, name
    assert not unreadable, "CLAP parameter text did not round-trip:\n" + "\n".join(unreadable)


def main():
    filename = {"darwin": "liblullaby.dylib", "win32": "lullaby.dll"}.get(sys.platform, "liblullaby.so")
    path = Path(sys.argv[1] if len(sys.argv) > 1 else Path("target/release") / filename).resolve()
    lib = C.CDLL(str(path))
    # Both format entry points must exist; full VST3 validation is separate.
    assert getattr(lib, "GetPluginFactory")
    entry = Entry.in_dll(lib, "clap_entry")
    assert entry.init(str(path).encode())
    factory_ptr = entry.factory(b"clap.plugin-factory")
    assert factory_ptr
    factory = C.cast(factory_ptr, C.POINTER(Factory)).contents
    assert factory.count(factory_ptr) == 1
    restart_requests = []
    host = Host(Version(1, 2, 5), None, b"Lullaby smoke host", b"Lullaby", b"", b"1.0",
                F(P, P, C.c_char_p)(lambda *_: None),
                F(None, P)(lambda _: restart_requests.append(True)),
                F(None, P)(lambda _: None), F(None, P)(lambda _: None))
    ptr = factory.create(factory_ptr, C.byref(host), b"audio.lullaby.resonance")
    assert ptr
    plugin = C.cast(ptr, C.POINTER(Plugin)).contents
    assert plugin.init(ptr)
    try:
        params = C.cast(plugin.extension(ptr, b"clap.params"), C.POINTER(Params)).contents
        parameters = {}
        ids = set()
        for i in range(params.count(ptr)):
            info = ParamInfo()
            assert params.info(ptr, i, C.byref(info))
            assert info.id not in ids
            ids.add(info.id)
            parameters[(bytes(info.module), bytes(info.name))] = info
        assert len(ids) == 73, f"Unexpected parameter count: {len(ids)}"
        assert parameters[(b"", b"Bypass")].flags & PARAM_IS_BYPASS
        check_parameter_text(params, ptr, parameters)
        outgoing = OutputEvents(None, F(B, P, P)(lambda *_: True))
        current_events = []
        incoming = Events(None, F(U, P)(lambda _: len(current_events)),
                          F(P, P, U)(lambda _, i: C.addressof(current_events[i])))

        def change(name, value):
            info = parameters[(b"", name.encode())]
            event = ParamEvent(Header(C.sizeof(ParamEvent), 0, 0, 5, 0), info.id, None,
                               -1, -1, -1, -1, value)
            current_events[:] = [event]
            params.flush(ptr, C.byref(incoming), C.byref(outgoing))
            current_events.clear()

        def get(name):
            value = C.c_double()
            assert params.get(ptr, parameters[(b"", name.encode())].id, C.byref(value))
            return value.value

        change("Depth", 0.0)
        assert get("Depth") == 0.0
        stepped_preset = {"Mode": 1, "Channels": 1, "Phase": 1, "Quality": 3}
        for name, value in stepped_preset.items():
            change(name, value)
            assert get(name) == value, name
        # State save, mutate, restore via deliberately short stream transfers.
        state = C.cast(plugin.extension(ptr, b"clap.state"), C.POINTER(State)).contents
        saved = bytearray()

        def write(_, data, size):
            length = min(size, 127)
            saved.extend(C.string_at(data, length))
            return length

        output_stream = Stream(None, F(C.c_int64, P, P, C.c_uint64)(write))
        assert state.save(ptr, C.byref(output_stream)) and saved
        # Continuous CLAP parameters in this framework are exposed normalized.
        change("Depth", 0.75)
        assert get("Depth") == 0.75
        for name in stepped_preset:
            change(name, 0)
            assert get(name) == 0, name
        cursor = 0

        def read(_, data, size):
            nonlocal cursor
            chunk = bytes(saved[cursor:cursor + min(size, 113)])
            C.memmove(data, chunk, len(chunk))
            cursor += len(chunk)
            return len(chunk)

        input_stream = Stream(None, F(C.c_int64, P, P, C.c_uint64)(read))
        assert state.load(ptr, C.byref(input_stream))
        assert get("Depth") == 0.0
        for name, value in stepped_preset.items():
            assert get(name) == value, name
            change(name, 0)

        for rate in (1234.5678, 7999.0, 384001.0, 768000.0, 0.0, math.nan, math.inf):
            assert not plugin.activate(ptr, rate, 1, 512), f"CLAP activated at {rate} Hz"
        for rate in (8000.0, 48000.25, 384000.0):
            assert plugin.activate(ptr, rate, 1, 512), f"CLAP rejected {rate} Hz"
            plugin.deactivate(ptr)

        latency = C.cast(plugin.extension(ptr, b"clap.latency"), C.POINTER(Latency)).contents
        for quality, expected in [(0, 2048), (3, 512)]:
            change("Quality", float(quality))
            assert plugin.activate(ptr, 48000.0, 1, 512)
            assert latency.get(ptr) == expected
            assert plugin.start(ptr)
            try:
                main_buffers = [(C.c_float * 512)() for _ in range(2)]
                side_buffers = [(C.c_float * 512)() for _ in range(2)]
                output_buffers = [(C.c_float * 512)() for _ in range(2)]
                pointers = [(C.POINTER(C.c_float) * 2)(*buffers) for buffers in [main_buffers, side_buffers, output_buffers]]
                inputs = (Audio * 2)(Audio(pointers[0], None, 2, 0, 0), Audio(pointers[1], None, 2, 0, 3))
                outputs = (Audio * 1)(Audio(pointers[2], None, 2, 0, 0))
                time = 0
                errors = []
                for block in range(120):
                    frames = [1, 31, 128, 257, 512][block % 5]
                    for i in range(frames):
                        main_buffers[0][i] = 0.2 * math.sin(math.tau * 1500.0 * (time + i) / 48000.0)
                        main_buffers[1][i] = main_buffers[0][i] * 0.5
                    process = Process(time, frames, None, inputs, outputs, 2, 1,
                                      C.addressof(incoming), C.addressof(outgoing))
                    assert plugin.process(ptr, C.byref(process)) != 0
                    for i in range(frames):
                        assert math.isfinite(output_buffers[0][i])
                        if time + i >= expected:
                            ideal = 0.2 * math.sin(math.tau * 1500.0 * (time + i - expected) / 48000.0)
                            errors.append(abs(output_buffers[0][i] - ideal))
                    time += frames
                assert max(errors) < 2e-5, max(errors)
                plugin.reset(ptr)
            finally:
                plugin.stop(ptr)
                plugin.deactivate(ptr)
        print(f"PASS: native CLAP loading, {len(ids)} unique parameters, parameter text round trips, "
              "bypass flag, parameter events, state recall, sample-rate limits, variable blocks, "
              "two latency modes; VST3 export present.")
    finally:
        plugin.destroy(ptr)
        entry.deinit()


if __name__ == "__main__":
    main()
