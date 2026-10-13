"""Minimal native CLAP host smoke test; Python standard library only.

Run after `cargo xtask bundle lullaby --release`. Exercises the actual shared
library, parameter events, state streams, latency, variable blocks and silence.
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
                ("get", F(B, P, U, C.POINTER(C.c_double))), ("to_text", P),
                ("from_text", P), ("flush", F(None, P, P, P))]


class State(C.Structure):
    _fields_ = [("save", F(B, P, P)), ("load", F(B, P, P))]


class Stream(C.Structure):
    _fields_ = [("ctx", P), ("transfer", F(C.c_int64, P, P, C.c_uint64))]


class Latency(C.Structure):
    _fields_ = [("get", F(U, P))]


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
        assert parameters[(b"", b"Bypass")].flags & (1 << 4)
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
        print(f"PASS: native CLAP loading, {len(ids)} unique parameters, bypass flag, parameter events, state recall, variable blocks, two latency modes; VST3 export present.")
    finally:
        plugin.destroy(ptr)
        entry.deinit()


if __name__ == "__main__":
    main()
