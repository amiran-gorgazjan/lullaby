//! Native VST3 audio-callback probe; dynamically loads either comparison build.
//! No editor is opened. See docs/performance.md for scope and paired comparisons.
use libloading::Library;
use nice_plug::wrapper::vst3::vst3::{
    ComPtr, Interface,
    Steinberg::{Vst::*, *},
};
use serde_json::json;
use std::{ffi::c_void, hint::black_box, io::Write, ptr, time::Instant};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let value = |flag| {
        args.windows(2)
            .find(|a| a[0] == flag)
            .map(|a| a[1].as_str())
    };
    let plugin = value("--plugin")
        .expect("--plugin must name the native shared library inside a VST3 bundle");
    let frames = value("--frames")
        .map(|n| n.parse().unwrap())
        .unwrap_or(48000_usize);
    assert!(frames >= 4096);
    let mut capture = value("--capture").map(|p| std::fs::File::create(p).unwrap());
    // SAFETY: only explicitly selected trusted local Lullaby builds are loaded.
    // All COM objects and factory pointers are dropped before their library.
    let library = unsafe { Library::new(plugin) }.expect("load native VST3 library");
    let mut results = Vec::new();
    for (name, rate, block_size, quality) in [
        ("default_stereo_48k", 48000.0, 128, 0.0),
        ("default_64", 48000.0, 64, 0.0),
        ("default_512", 48000.0, 512, 0.0),
        ("default_stereo_96k", 96000.0, 128, 0.0),
        ("high_quality", 48000.0, 128, 1.0),
        ("ultra_quality", 48000.0, 128, 2.0),
        ("low_latency", 48000.0, 64, 3.0),
    ] {
        // SAFETY: buffers and pointer arrays outlive each synchronous callback;
        // the factory/interfaces follow the same ABI as tests/vst3_host.rs.
        let result = unsafe {
            probe(
                &library,
                name,
                rate,
                block_size,
                quality,
                frames,
                &mut capture,
            )
        };
        results.push(result);
    }
    println!("{}", json!({"schema":1,"cases":results}));
}

#[allow(clippy::too_many_arguments)]
unsafe fn probe(
    library: &Library,
    name: &str,
    rate: f64,
    block_size: usize,
    quality: f64,
    frames: usize,
    capture: &mut Option<std::fs::File>,
) -> serde_json::Value {
    // SAFETY: explicit trusted plugin, correctly typed exported factory. The
    // library remains loaded until every interface and factory is released.
    unsafe {
        let get_factory = library
            .get::<unsafe extern "system" fn() -> *mut c_void>(b"GetPluginFactory\0")
            .unwrap();
        let factory = ComPtr::<IPluginFactory>::from_raw(get_factory().cast()).unwrap();
        assert_eq!(factory.countClasses(), 1);
        let mut class: PClassInfo = std::mem::zeroed();
        assert_eq!(factory.getClassInfo(0, &mut class), kResultOk);
        let mut raw = ptr::null_mut();
        assert_eq!(
            factory.createInstance(
                class.cid.as_ptr(),
                IComponent::IID.as_ptr().cast(),
                &mut raw
            ),
            kResultOk
        );
        let component = ComPtr::<IComponent>::from_raw(raw.cast()).unwrap();
        assert_eq!(component.initialize(ptr::null_mut()), kResultOk);
        let controller = component.cast::<IEditController>().unwrap();
        let processor = component.cast::<IAudioProcessor>().unwrap();
        let mut found_quality = false;
        for i in 0..controller.getParameterCount() {
            let mut info: ParameterInfo = std::mem::zeroed();
            assert_eq!(controller.getParameterInfo(i, &mut info), kResultOk);
            let n = info
                .title
                .iter()
                .position(|&x| x == 0)
                .unwrap_or(info.title.len());
            if String::from_utf16_lossy(&info.title[..n]) == "Quality" {
                let normalized = quality / 3.0;
                assert_eq!(
                    controller.normalizedParamToPlain(info.id, normalized),
                    quality
                );
                assert_eq!(
                    controller.setParamNormalized(info.id, normalized),
                    kResultOk
                );
                found_quality = true;
            }
        }
        assert!(found_quality);
        let mut inputs = [3_u64, 3_u64];
        let mut outputs = [3_u64];
        assert_eq!(
            processor.setBusArrangements(inputs.as_mut_ptr(), 2, outputs.as_mut_ptr(), 1),
            kResultOk
        );
        let mut setup = ProcessSetup {
            processMode: 0,
            symbolicSampleSize: 0,
            maxSamplesPerBlock: block_size as i32,
            sampleRate: rate,
        };
        assert_eq!(processor.setupProcessing(&mut setup), kResultOk);
        assert_eq!(component.setActive(1), kResultOk);
        assert_eq!(processor.setProcessing(1), kResultOk);
        let latency = processor.getLatencySamples();
        assert_eq!(latency, if quality == 3.0 { 512 } else { 2048 });
        let total = frames + 8192;
        let mut state = 0x9182_abcd_u32;
        let signal: Vec<[f32; 2]> = (0..total)
            .map(|i| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let noise = state as i32 as f32 / i32::MAX as f32 * 0.025;
                let t = i as f32 / rate as f32;
                let sine = |f: f32| (std::f32::consts::TAU * f * t).sin();
                let saw = |f: f32| 2.0 * (f * t).fract() - 1.0;
                [
                    0.12 * saw(110.0) + 0.19 * sine(1056.0) + noise,
                    0.1 * saw(146.8) + 0.16 * sine(3217.0) - noise,
                ]
            })
            .collect();
        let mut left = vec![0.0; block_size];
        let mut right = vec![0.0; block_size];
        let mut key_l = vec![0.0; block_size];
        let mut key_r = vec![0.0; block_size];
        let mut out_l = vec![0.0; block_size];
        let mut out_r = vec![0.0; block_size];
        let mut main_ptrs = [left.as_mut_ptr(), right.as_mut_ptr()];
        let mut key_ptrs = [key_l.as_mut_ptr(), key_r.as_mut_ptr()];
        let mut out_ptrs = [out_l.as_mut_ptr(), out_r.as_mut_ptr()];
        let bus = |p| AudioBusBuffers {
            numChannels: 2,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: p,
            },
        };
        let mut audio_inputs = [bus(main_ptrs.as_mut_ptr()), bus(key_ptrs.as_mut_ptr())];
        let mut audio_outputs = [bus(out_ptrs.as_mut_ptr())];
        let mut output = Vec::with_capacity(frames);
        let mut block_ns = Vec::with_capacity(frames.div_ceil(block_size));
        let mut offset = 0;
        while offset < total {
            // End warmup at an exact block boundary before starting timing.
            let remaining = if offset < 8192 {
                8192 - offset
            } else {
                total - offset
            };
            let count = remaining.min(block_size);
            for i in 0..count {
                left[i] = signal[offset + i][0];
                right[i] = signal[offset + i][1];
            }
            let mut data = ProcessData {
                processMode: 0,
                symbolicSampleSize: 0,
                numSamples: count as i32,
                numInputs: 2,
                numOutputs: 1,
                inputs: audio_inputs.as_mut_ptr(),
                outputs: audio_outputs.as_mut_ptr(),
                inputParameterChanges: ptr::null_mut(),
                outputParameterChanges: ptr::null_mut(),
                inputEvents: ptr::null_mut(),
                outputEvents: ptr::null_mut(),
                processContext: ptr::null_mut(),
            };
            let start = Instant::now();
            let status = processor.process(black_box(&mut data));
            let elapsed = start.elapsed().as_nanos() as u64;
            assert_eq!(status, kResultOk);
            if offset >= 8192 {
                block_ns.push(elapsed);
                for i in 0..count {
                    output.push([out_l[i], out_r[i]]);
                }
            }
            offset += count;
        }
        assert!(output.iter().flatten().all(|x| x.is_finite()));
        black_box(&output);
        let elapsed_ns: u64 = block_ns.iter().sum();
        block_ns.sort_unstable();
        let percentile = |p: f64| block_ns[((block_ns.len() - 1) as f64 * p).round() as usize];
        if let Some(file) = capture {
            for x in output.iter().flatten() {
                file.write_all(&x.to_le_bytes()).unwrap();
            }
        }
        assert_eq!(processor.setProcessing(0), kResultOk);
        assert_eq!(component.setActive(0), kResultOk);
        assert_eq!(component.terminate(), kResultOk);
        json!({"case":name,"frames":frames,"sample_rate":rate,"block_size":block_size,
            "latency_samples":latency,"elapsed_ns":elapsed_ns,"ns_per_sample":elapsed_ns as f64/frames as f64,
            "realtime_percent":elapsed_ns as f64/(frames as f64/rate*1e9)*100.0,
            "block_p50_ns":percentile(0.5),"block_p95_ns":percentile(0.95),"block_p99_ns":percentile(0.99)})
    }
}
