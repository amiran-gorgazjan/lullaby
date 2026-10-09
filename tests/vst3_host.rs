//! Exercise the exported VST3 factory, controller conversion and audio wrapper.
//! This catches a parameter-scale bug that testing the DSP directly would miss.
use lullaby_dsp::{Engine, Settings};
use nice_plug::wrapper::vst3::vst3::{
    ComPtr, Interface,
    Steinberg::{Vst::*, *},
};
use std::{ffi::c_void, ptr};

#[path = "../crates/lullaby-dsp/tests/support/mod.rs"]
#[allow(dead_code)]
mod support;

#[test]
fn vst3_depth_values_and_processed_audio_match_the_dsp() {
    // All COM objects originate from this plugin's factory. Buffers outlive
    // every process call and are sized for the maximum advertised block.
    unsafe {
        let factory =
            ComPtr::<IPluginFactory>::from_raw(lullaby::GetPluginFactory().cast()).unwrap();
        assert_eq!(factory.countClasses(), 1);
        let mut class: PClassInfo = std::mem::zeroed();
        assert_eq!(factory.getClassInfo(0, &mut class), kResultOk);
        let mut raw: *mut c_void = ptr::null_mut();
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
        let mut depth_id = None;
        for i in 0..controller.getParameterCount() {
            let mut info: ParameterInfo = std::mem::zeroed();
            assert_eq!(controller.getParameterInfo(i, &mut info), kResultOk);
            let len = info
                .title
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(info.title.len());
            if String::from_utf16_lossy(&info.title[..len]) == "Depth" {
                depth_id = Some(info.id);
                assert!((info.defaultNormalizedValue - 0.25).abs() < 1e-6);
            }
        }
        let depth_id = depth_id.expect("exported Depth parameter");
        assert!((controller.getParamNormalized(depth_id) - 0.25).abs() < 1e-6);
        let mut input_arrangements = [3_u64, 3_u64]; // VST3 stereo L|R
        let mut output_arrangements = [3_u64];
        assert_eq!(
            processor.setBusArrangements(
                input_arrangements.as_mut_ptr(),
                2,
                output_arrangements.as_mut_ptr(),
                1
            ),
            kResultOk
        );
        let input = support::harmonic_fixture(48000.0, true);
        for (normalized, plain) in [(0.25, 6.0), (0.0, 0.0), (0.5, 12.0)] {
            assert_eq!(
                controller.setParamNormalized(depth_id, normalized),
                kResultOk
            );
            assert!((controller.normalizedParamToPlain(depth_id, normalized) - plain).abs() < 1e-6);
            let mut setup = ProcessSetup {
                processMode: 0,
                symbolicSampleSize: 0,
                maxSamplesPerBlock: 256,
                sampleRate: 48000.0,
            };
            assert_eq!(processor.setupProcessing(&mut setup), kResultOk);
            assert_eq!(component.setActive(1), kResultOk);
            assert_eq!(processor.setProcessing(1), kResultOk);
            assert_eq!(processor.getLatencySamples(), 2048);
            let mut reference = Engine::new(48000.0, lullaby_dsp::Quality::Normal, false);
            reference.set_settings(Settings {
                depth: plain as f32,
                ..Settings::default()
            });
            let mut left = [0.0_f32; 256];
            let mut right = [0.0_f32; 256];
            let mut key_l = [0.0_f32; 256];
            let mut key_r = [0.0_f32; 256];
            let mut out_l = [0.0_f32; 256];
            let mut out_r = [0.0_f32; 256];
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
            let mut inputs = [bus(main_ptrs.as_mut_ptr()), bus(key_ptrs.as_mut_ptr())];
            let mut outputs = [bus(out_ptrs.as_mut_ptr())];
            let mut offset = 0;
            let mut block = 0;
            while offset < input.len() + 2048 {
                let count = [31, 256, 127][block % 3].min(input.len() + 2048 - offset);
                for i in 0..count {
                    left[i] = input.get(offset + i).copied().unwrap_or(0.0);
                    right[i] = left[i];
                }
                let mut data = ProcessData {
                    processMode: 0,
                    symbolicSampleSize: 0,
                    numSamples: count as i32,
                    numInputs: 2,
                    numOutputs: 1,
                    inputs: inputs.as_mut_ptr(),
                    outputs: outputs.as_mut_ptr(),
                    inputParameterChanges: ptr::null_mut(),
                    outputParameterChanges: ptr::null_mut(),
                    inputEvents: ptr::null_mut(),
                    outputEvents: ptr::null_mut(),
                    processContext: ptr::null_mut(),
                };
                assert_eq!(processor.process(&mut data), kResultOk);
                for i in 0..count {
                    let sample = input.get(offset + i).copied().unwrap_or(0.0);
                    let expected = reference.process_sample([sample; 2], [0.0; 2]);
                    assert!(
                        (out_l[i] - expected[0]).abs() < 2e-6,
                        "VST3 differs from DSP at Depth {plain}, sample {}",
                        offset + i
                    );
                    assert!((out_r[i] - expected[1]).abs() < 2e-6);
                }
                offset += count;
                block += 1;
            }
            assert_eq!(processor.setProcessing(0), kResultOk);
            assert_eq!(component.setActive(0), kResultOk);
        }
        assert_eq!(component.terminate(), kResultOk);
    }
}
