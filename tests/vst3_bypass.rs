//! Exercise bypass readback and processing through the exported VST3 interfaces.

use lullaby_dsp::{Engine, Quality, Settings};
use nice_plug::wrapper::vst3::vst3::{
    Class, ComPtr, ComWrapper, Interface,
    Steinberg::{IBStream_::IStreamSeekMode_ as SeekMode, Vst::*, *},
};
use std::{
    cell::RefCell,
    ffi::c_void,
    io::{BufRead, Cursor, Seek, SeekFrom, Write},
    ptr, slice,
};

const SAMPLE_RATE: f64 = 48_000.0;
const BLOCK_SIZE: usize = 256;
const SAMPLE_32: i32 = SymbolicSampleSizes_::kSample32 as i32;
const REALTIME: i32 = ProcessModes_::kRealtime as i32;
const BYPASS_FLAG: i32 = ParameterInfo_::ParameterFlags_::kIsBypass;

#[test]
fn vst3_bypass_controller_reads_back_immediately_while_processing() {
    let (component, processor, controller, bypass) = active_plugin(0.0);
    // The component and controller originate from the same initialized factory instance.
    unsafe {
        assert_eq!(controller.getParamNormalized(bypass), 0.0);
        assert_eq!(controller.setParamNormalized(bypass, 1.0), kResultOk);
        let enabled = controller.getParamNormalized(bypass);
        flush_bypass(&processor, bypass, 1.0);
        assert_eq!(controller.setParamNormalized(bypass, 0.0), kResultOk);
        let disabled = controller.getParamNormalized(bypass);
        flush_bypass(&processor, bypass, 0.0);
        assert_eq!(processor.setProcessing(0), kResultOk);
        assert_eq!(component.setActive(0), kResultOk);
        assert_eq!(component.terminate(), kResultOk);
        assert_eq!(
            (enabled, disabled),
            (1.0, 0.0),
            "controller updates must read back before the matching processor flush"
        );
    }
}

#[test]
fn vst3_bypass_controller_keeps_newer_edit_when_an_older_event_arrives() {
    let (component, processor, controller, bypass) = active_plugin(0.0);
    // The component and controller originate from the same initialized factory instance.
    unsafe {
        assert_eq!(controller.setParamNormalized(bypass, 1.0), kResultOk);
        assert_eq!(controller.setParamNormalized(bypass, 0.0), kResultOk);
        flush_bypass(&processor, bypass, 1.0);
        assert_eq!(controller.getParamNormalized(bypass), 0.0);

        flush_bypass(&processor, bypass, 0.0);
        assert_eq!(controller.getParamNormalized(bypass), 0.0);
        // Once the matching event is delivered, processor-only automation is visible again.
        flush_bypass(&processor, bypass, 1.0);
        assert_eq!(controller.getParamNormalized(bypass), 1.0);

        assert_eq!(processor.setProcessing(0), kResultOk);
        assert_eq!(component.setActive(0), kResultOk);
        assert_eq!(component.terminate(), kResultOk);
    }
}

#[test]
fn vst3_bypass_controller_clears_pending_edits_on_stop_and_state_load() {
    let (component, processor, controller, bypass) = active_plugin(0.0);
    let saved_state = ComWrapper::new(StateStream::default());
    let stream = saved_state.as_com_ref::<IBStream>().unwrap();
    // The stream and component remain live for all state and parameter calls.
    unsafe {
        assert_eq!(component.getState(stream.as_ptr()), kResultOk);
        assert_eq!(controller.setParamNormalized(bypass, 1.0), kResultOk);
        assert_eq!(processor.setProcessing(1), kResultOk);
        assert_eq!(controller.getParamNormalized(bypass), 1.0);
        assert_eq!(
            controller.setParamNormalized(bypass, f64::NAN),
            kInvalidArgument
        );
        assert_eq!(
            controller.setParamNormalized(u32::MAX, 0.0),
            kInvalidArgument
        );
        assert_eq!(controller.getParamNormalized(bypass), 1.0);

        assert_eq!(processor.setProcessing(0), kResultOk);
        assert_eq!(controller.getParamNormalized(bypass), 0.0);
        assert_eq!(processor.setProcessing(1), kResultOk);
        assert_eq!(controller.getParamNormalized(bypass), 0.0);

        assert_eq!(controller.setParamNormalized(bypass, 1.0), kResultOk);
        assert_eq!(
            stream.seek(0, SeekMode::kIBSeekSet as i32, ptr::null_mut()),
            kResultOk
        );
        assert_eq!(component.setState(stream.as_ptr()), kResultOk);
        assert_eq!(controller.getParamNormalized(bypass), 0.0);
        assert_eq!(processor.setProcessing(0), kResultOk);
        assert_eq!(component.setActive(0), kResultOk);
        assert_eq!(component.terminate(), kResultOk);
    }
}

#[test]
fn vst3_bypass_audio_waits_for_event_offset_and_state_round_trips() {
    let output_db = -12.0;
    let (component, processor, controller, bypass) = active_plugin(output_db);
    let mut settings = Settings {
        output_db,
        ..Settings::default()
    };
    let mut reference = Engine::new(SAMPLE_RATE as f32, Quality::Normal, false);
    reference.set_settings(settings);
    let sample = 0.25;
    let mut input_left = [sample; BLOCK_SIZE];
    let mut input_right = [sample; BLOCK_SIZE];
    let mut side_left = [0.0; BLOCK_SIZE];
    let mut side_right = [0.0; BLOCK_SIZE];
    let mut output_left = [0.0; BLOCK_SIZE];
    let mut output_right = [0.0; BLOCK_SIZE];
    let mut main = [input_left.as_mut_ptr(), input_right.as_mut_ptr()];
    let mut side = [side_left.as_mut_ptr(), side_right.as_mut_ptr()];
    let mut output = [output_left.as_mut_ptr(), output_right.as_mut_ptr()];
    let mut inputs = [stereo_bus(&mut main), stereo_bus(&mut side)];
    let mut outputs = [stereo_bus(&mut output)];
    let mut block = ProcessData {
        numSamples: BLOCK_SIZE as i32,
        numInputs: 2,
        numOutputs: 1,
        inputs: inputs.as_mut_ptr(),
        outputs: outputs.as_mut_ptr(),
        ..empty_process_data()
    };
    let initial = ComWrapper::new(StateStream::default());
    let pending = ComWrapper::new(StateStream::default());
    let enabled = ComWrapper::new(StateStream::default());
    let initial_stream = initial.as_com_ref::<IBStream>().unwrap();
    let pending_stream = pending.as_com_ref::<IBStream>().unwrap();
    let enabled_stream = enabled.as_com_ref::<IBStream>().unwrap();

    // Audio and parameter queue storage remain valid through each process call.
    unsafe {
        assert_eq!(component.getState(initial_stream.as_ptr()), kResultOk);
        assert_eq!(controller.setParamNormalized(bypass, 1.0), kResultOk);
        assert_eq!(component.getState(pending_stream.as_ptr()), kResultOk);
        assert_eq!(
            initial.0.borrow().get_ref(),
            pending.0.borrow().get_ref(),
            "a controller edit must not change the processor's persisted value"
        );

        // Let the DSP fill its delay and filter state while the host still has
        // not delivered the matching audio-side event.
        let warmup_blocks = 24;
        for _ in 0..warmup_blocks {
            assert_eq!(processor.process(&mut block), kResultOk);
            for actual in output_left {
                let expected = reference.process_sample([sample; 2], [0.0; 2])[0];
                assert!((actual - expected).abs() < 2e-6);
            }
        }

        let event_offset = 96;
        let changes = one_change(bypass, event_offset as i32, 1.0);
        block.inputParameterChanges = changes.as_ptr();
        assert_eq!(processor.process(&mut block), kResultOk);
        let unbypassed = output_left[event_offset - 1];
        for (index, actual) in output_left.into_iter().enumerate() {
            if index == event_offset {
                settings.bypass = true;
                reference.set_settings(settings);
            }
            let expected = reference.process_sample([sample; 2], [0.0; 2])[0];
            assert!(
                (actual - expected).abs() < 2e-6,
                "bypass did not follow its sample offset at sample {index}"
            );
        }
        assert!(
            (output_left[BLOCK_SIZE - 1] - unbypassed).abs() > 1e-3,
            "fixture must exercise the bypass transition"
        );

        assert_eq!(processor.setProcessing(0), kResultOk);
        assert_eq!(component.getState(enabled_stream.as_ptr()), kResultOk);
        assert_ne!(initial.0.borrow().get_ref(), enabled.0.borrow().get_ref());
        assert_eq!(processor.setProcessing(1), kResultOk);
        flush_bypass(&processor, bypass, 0.0);
        assert_eq!(controller.getParamNormalized(bypass), 0.0);
        assert_eq!(processor.setProcessing(0), kResultOk);

        assert_eq!(
            enabled_stream.seek(0, SeekMode::kIBSeekSet as i32, ptr::null_mut()),
            kResultOk
        );
        assert_eq!(component.setState(enabled_stream.as_ptr()), kResultOk);
        assert_eq!(controller.getParamNormalized(bypass), 1.0);
        assert_eq!(component.setActive(0), kResultOk);
        assert_eq!(component.terminate(), kResultOk);
    }
}

fn active_plugin(
    output_db: f32,
) -> (
    ComPtr<IComponent>,
    ComPtr<IAudioProcessor>,
    ComPtr<IEditController>,
    ParamID,
) {
    // The exported factory transfers ownership of each non-null COM pointer.
    unsafe {
        let factory =
            ComPtr::<IPluginFactory>::from_raw(lullaby::GetPluginFactory().cast()).unwrap();
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
        let processor = component.cast::<IAudioProcessor>().unwrap();
        let controller = component.cast::<IEditController>().unwrap();
        let mut bypass = None;
        let mut output = None;
        let output_title: Vec<u16> = "Output".encode_utf16().chain([0]).collect();
        for index in 0..controller.getParameterCount() {
            let mut info: ParameterInfo = std::mem::zeroed();
            assert_eq!(controller.getParameterInfo(index, &mut info), kResultOk);
            if info.flags & BYPASS_FLAG != 0 {
                bypass = Some(info.id);
            } else if info.title.starts_with(&output_title) {
                output = Some(info.id);
            }
        }
        let bypass = bypass.expect("the VST3 effect exposes a bypass parameter");
        let output = output.expect("the test can set the output gain");
        let gain = controller.plainParamToNormalized(output, f64::from(output_db));
        assert_eq!(controller.setParamNormalized(output, gain), kResultOk);
        assert_eq!(
            processor.setupProcessing(&mut ProcessSetup {
                processMode: REALTIME,
                symbolicSampleSize: SAMPLE_32,
                maxSamplesPerBlock: BLOCK_SIZE as i32,
                sampleRate: SAMPLE_RATE,
            }),
            kResultOk
        );
        assert_eq!(component.setActive(1), kResultOk);
        assert_eq!(processor.setProcessing(1), kResultOk);
        (component, processor, controller, bypass)
    }
}

fn stereo_bus(channels: &mut [*mut f32; 2]) -> AudioBusBuffers {
    AudioBusBuffers {
        numChannels: 2,
        silenceFlags: 0,
        __field0: AudioBusBuffers__type0 {
            channelBuffers32: channels.as_mut_ptr(),
        },
    }
}

fn empty_process_data() -> ProcessData {
    ProcessData {
        processMode: REALTIME,
        symbolicSampleSize: SAMPLE_32,
        numSamples: 0,
        numInputs: 0,
        numOutputs: 0,
        inputs: ptr::null_mut(),
        outputs: ptr::null_mut(),
        inputParameterChanges: ptr::null_mut(),
        outputParameterChanges: ptr::null_mut(),
        inputEvents: ptr::null_mut(),
        outputEvents: ptr::null_mut(),
        processContext: ptr::null_mut(),
    }
}

fn flush_bypass(processor: &ComPtr<IAudioProcessor>, id: ParamID, value: ParamValue) {
    let changes = one_change(id, 0, value);
    let mut block = ProcessData {
        inputParameterChanges: changes.as_ptr(),
        ..empty_process_data()
    };
    // The queue remains valid during the parameter-only process call.
    assert_eq!(unsafe { processor.process(&mut block) }, kResultOk);
}

fn one_change(id: ParamID, offset: i32, value: ParamValue) -> ComPtr<IParameterChanges> {
    let point = ComWrapper::new(OnePoint { id, offset, value })
        .to_com_ptr::<IParamValueQueue>()
        .unwrap();
    ComWrapper::new(OneQueue(point))
        .to_com_ptr::<IParameterChanges>()
        .unwrap()
}

struct OnePoint {
    id: ParamID,
    offset: i32,
    value: ParamValue,
}

impl Class for OnePoint {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for OnePoint {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id
    }

    unsafe fn getPointCount(&self) -> i32 {
        1
    }

    unsafe fn getPoint(&self, index: i32, offset: *mut i32, value: *mut ParamValue) -> tresult {
        if index != 0 || offset.is_null() || value.is_null() {
            return kInvalidArgument;
        }
        unsafe {
            offset.write(self.offset);
            value.write(self.value);
        }
        kResultOk
    }

    unsafe fn addPoint(&self, _: i32, _: ParamValue, _: *mut i32) -> tresult {
        kNotImplemented
    }
}

struct OneQueue(ComPtr<IParamValueQueue>);

impl Class for OneQueue {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for OneQueue {
    unsafe fn getParameterCount(&self) -> i32 {
        1
    }

    unsafe fn getParameterData(&self, index: i32) -> *mut IParamValueQueue {
        if index == 0 {
            self.0.as_ptr()
        } else {
            ptr::null_mut()
        }
    }

    unsafe fn addParameterData(&self, _: *const ParamID, _: *mut i32) -> *mut IParamValueQueue {
        ptr::null_mut()
    }
}

#[derive(Default)]
struct StateStream(RefCell<Cursor<Vec<u8>>>);

impl Class for StateStream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for StateStream {
    unsafe fn read(&self, buffer: *mut c_void, bytes: i32, read: *mut i32) -> tresult {
        if buffer.is_null() || bytes < 0 {
            return kInvalidArgument;
        }
        let mut stream = self.0.borrow_mut();
        let remaining = stream.fill_buf().unwrap();
        let count = remaining.len().min(bytes as usize);
        // The VST3 component may pass uninitialized storage as the destination.
        unsafe { ptr::copy_nonoverlapping(remaining.as_ptr(), buffer.cast(), count) };
        stream.consume(count);
        if !read.is_null() {
            unsafe { read.write(count as i32) };
        }
        kResultOk
    }

    unsafe fn write(&self, buffer: *mut c_void, bytes: i32, written: *mut i32) -> tresult {
        if buffer.is_null() || bytes < 0 {
            return kInvalidArgument;
        }
        let buffer = unsafe { slice::from_raw_parts(buffer.cast(), bytes as usize) };
        self.0.borrow_mut().write_all(buffer).unwrap();
        if !written.is_null() {
            unsafe { written.write(bytes) };
        }
        kResultOk
    }

    unsafe fn seek(&self, pos: i64, mode: i32, result: *mut i64) -> tresult {
        let from = match mode as u32 {
            SeekMode::kIBSeekSet if pos >= 0 => SeekFrom::Start(pos as u64),
            SeekMode::kIBSeekCur => SeekFrom::Current(pos),
            SeekMode::kIBSeekEnd => SeekFrom::End(pos),
            _ => return kInvalidArgument,
        };
        let Ok(pos) = self.0.borrow_mut().seek(from) else {
            return kInvalidArgument;
        };
        if !result.is_null() {
            unsafe { result.write(pos as i64) };
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut i64) -> tresult {
        if pos.is_null() {
            return kInvalidArgument;
        }
        unsafe { pos.write(self.0.borrow().position() as i64) };
        kResultOk
    }
}
