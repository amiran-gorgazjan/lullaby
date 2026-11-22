//! Check audio bus negotiation through the exported VST3 factory.

use nice_plug::wrapper::vst3::vst3::{
    ComPtr, Interface,
    Steinberg::{
        IPluginBaseTrait, IPluginFactory, IPluginFactoryTrait, PClassInfo, Vst::*,
        kInvalidArgument, kResultFalse as RESULT_FALSE, kResultOk as RESULT_OK, tresult,
    },
};
use std::{ffi::c_void, ptr};

const MONO: SpeakerArrangement = SpeakerArr::kMono;
const STEREO: SpeakerArrangement = SpeakerArr::kStereo;
const INPUT: BusDirection = BusDirections_::kInput as BusDirection;
const OUTPUT: BusDirection = BusDirections_::kOutput as BusDirection;
const AUDIO: MediaType = MediaTypes_::kAudio as MediaType;

#[test]
fn vst3_round_trips_supported_sidechain_arrangements() {
    with_processor(|processor| {
        assert_eq!(arrangements(processor), ([STEREO, STEREO], [STEREO]));

        for layout in [MONO, STEREO] {
            assert_eq!(
                set_arrangements(processor, [layout, layout], [layout]),
                RESULT_OK
            );
            assert_eq!(arrangements(processor), ([layout, layout], [layout]));
        }
    });
}

#[test]
fn vst3_does_not_substitute_an_auxiliary_bus_when_accepting_mixed_layouts() {
    with_processor(|processor| {
        // Steinberg's SideChain Arrangement test switches the main I/O to mono
        // and leaves the auxiliary input at its default stereo arrangement.
        check_acceptance(processor, [MONO, STEREO], [MONO]);
        check_acceptance(processor, [STEREO, MONO], [STEREO]);
    });
}

#[test]
fn vst3_does_not_substitute_the_speaker_mask_of_an_accepted_sidechain() {
    with_processor(|processor| {
        // The two masks have the same channel count and different speaker positions.
        check_acceptance(processor, [STEREO, SpeakerArr::kStereoSurround], [STEREO]);
    });
}

#[test]
fn vst3_reports_bus_counts_and_masks_when_sidechain_is_removed_and_restored() {
    with_processor(|processor| {
        let component = processor.cast::<IComponent>().unwrap();
        for layout in [MONO, STEREO] {
            let mut inputs = [layout];
            let mut outputs = [layout];
            // The arrays contain the single main bus in each direction.
            unsafe {
                assert_eq!(
                    processor.setBusArrangements(inputs.as_mut_ptr(), 1, outputs.as_mut_ptr(), 1),
                    RESULT_OK
                );
                assert_eq!(component.getBusCount(AUDIO, INPUT), 1);
                assert_eq!(component.getBusCount(AUDIO, OUTPUT), 1);
            }
            assert_eq!(read_arrangement(processor, INPUT, 0), layout);
            assert_eq!(read_arrangement(processor, OUTPUT, 0), layout);

            assert_eq!(
                set_arrangements(processor, [layout, layout], [layout]),
                RESULT_OK
            );
            assert_eq!(unsafe { component.getBusCount(AUDIO, INPUT) }, 2);
            assert_eq!(arrangements(processor), ([layout, layout], [layout]));
        }
    });
}

#[test]
fn vst3_rejects_invalid_bus_queries_without_changing_the_layout() {
    with_processor(|processor| {
        let component = processor.cast::<IComponent>().unwrap();
        let before = arrangements(processor);
        let mut inputs = [MONO; 3];
        let mut outputs = [MONO];
        let mut mask = 0;
        // Non-null pointers have enough storage for each advertised array or output.
        unsafe {
            assert_eq!(
                processor.setBusArrangements(inputs.as_mut_ptr(), -1, outputs.as_mut_ptr(), 1),
                kInvalidArgument
            );
            assert_eq!(
                processor.setBusArrangements(ptr::null_mut(), 2, outputs.as_mut_ptr(), 1),
                kInvalidArgument
            );
            assert_eq!(
                processor.setBusArrangements(ptr::null_mut(), 0, ptr::null_mut(), 0),
                RESULT_FALSE
            );
            assert_eq!(
                processor.setBusArrangements(inputs.as_mut_ptr(), 3, outputs.as_mut_ptr(), 1),
                RESULT_FALSE
            );
            for index in [i32::MIN, -1, 2] {
                let mut info = std::mem::zeroed();
                assert_eq!(
                    processor.getBusArrangement(INPUT, index, &mut mask),
                    kInvalidArgument
                );
                assert_eq!(
                    component.getBusInfo(AUDIO, INPUT, index, &mut info),
                    kInvalidArgument
                );
            }
        }
        assert_eq!(arrangements(processor), before);
    });
}

fn with_processor(check: impl FnOnce(&ComPtr<IAudioProcessor>)) {
    // All interface pointers come from the exported factory and are held by
    // COM references for the duration of the check.
    unsafe {
        let factory =
            ComPtr::<IPluginFactory>::from_raw(lullaby::GetPluginFactory().cast()).unwrap();
        let mut class: PClassInfo = std::mem::zeroed();
        assert_eq!(factory.getClassInfo(0, &mut class), RESULT_OK);
        let mut raw: *mut c_void = ptr::null_mut();
        assert_eq!(
            factory.createInstance(
                class.cid.as_ptr(),
                IComponent::IID.as_ptr().cast(),
                &mut raw
            ),
            RESULT_OK
        );
        let component = ComPtr::<IComponent>::from_raw(raw.cast()).unwrap();
        assert_eq!(component.initialize(ptr::null_mut()), RESULT_OK);
        let processor = component.cast::<IAudioProcessor>().unwrap();
        check(&processor);
        assert_eq!(component.terminate(), RESULT_OK);
    }
}

fn set_arrangements(
    processor: &ComPtr<IAudioProcessor>,
    mut inputs: [SpeakerArrangement; 2],
    mut outputs: [SpeakerArrangement; 1],
) -> tresult {
    // Both buffers are writable and match the counts given to the processor.
    unsafe {
        processor.setBusArrangements(
            inputs.as_mut_ptr(),
            inputs.len() as i32,
            outputs.as_mut_ptr(),
            outputs.len() as i32,
        )
    }
}

fn arrangements(
    processor: &ComPtr<IAudioProcessor>,
) -> ([SpeakerArrangement; 2], [SpeakerArrangement; 1]) {
    let read = |direction, index| read_arrangement(processor, direction, index);
    ([read(INPUT, 0), read(INPUT, 1)], [read(OUTPUT, 0)])
}

fn read_arrangement(
    processor: &ComPtr<IAudioProcessor>,
    direction: BusDirection,
    index: i32,
) -> SpeakerArrangement {
    let mut mask = 0;
    // The output pointer remains writable for the duration of the call.
    assert_eq!(
        unsafe { processor.getBusArrangement(direction, index, &mut mask) },
        RESULT_OK
    );
    mask
}

fn check_acceptance(
    processor: &ComPtr<IAudioProcessor>,
    inputs: [SpeakerArrangement; 2],
    outputs: [SpeakerArrangement; 1],
) {
    let before = arrangements(processor);
    match set_arrangements(processor, inputs, outputs) {
        RESULT_OK => assert_eq!(
            arrangements(processor),
            (inputs, outputs),
            "accepted request must round-trip every bus arrangement"
        ),
        RESULT_FALSE => assert_eq!(
            arrangements(processor),
            before,
            "rejected request changed the current bus arrangements"
        ),
        result => panic!("unexpected arrangement negotiation result: {result}"),
    }
}
