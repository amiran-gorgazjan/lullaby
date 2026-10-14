use lullaby_dsp::{Engine, Phase, Quality, Settings};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

struct CountAlloc;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

// The system allocator's contract is preserved. Tracking is thread-local so
// unrelated test runner allocations cannot affect the audio-thread assertion.
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            ALLOCS.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if TRACK.try_with(Cell::get).unwrap_or(false) {
            ALLOCS.with(|c| c.set(c.get() + 1));
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;

#[test]
fn audio_and_automation_never_allocate_or_free() {
    let mut engine = Engine::new(48000.0, Quality::Ultra, false);
    let mut settings = Settings::default();
    ALLOCS.with(|c| c.set(0));
    TRACK.with(|c| c.set(true));
    for i in 0..12000 {
        if i % 100 == 0 {
            settings.depth = (i % 11) as f32;
            settings.detail = ((i / 100) % 11) as f32;
            settings.attack = ((i / 100) % 11) as f32;
            settings.release = 10.0 - settings.attack;
            settings.max_cut_db = if i % 200 == 0 {
                lullaby_dsp::MAX_CUT_OFF
            } else {
                3.0
            };
            settings.detail_tilt = if i % 200 == 0 {
                [0.1, 10.0]
            } else {
                [10.0, 0.1]
            };
            settings.attack_tilt = settings.detail_tilt;
            settings.release_tilt = [settings.detail_tilt[1], settings.detail_tilt[0]];
            settings.phase = if i % 200 == 0 {
                Phase::Minimum
            } else {
                Phase::Linear
            };
            settings.listen_band = if i % 300 == 0 { 0 } else { -1 };
            engine.set_settings(settings);
        }
        let value = (i as f32 * 0.21).sin() * 0.3;
        let output = engine.process_sample([value; 2], [value; 2]);
        assert!(output.iter().all(|x| x.is_finite()));
    }
    engine.reset();
    TRACK.with(|c| c.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
}
