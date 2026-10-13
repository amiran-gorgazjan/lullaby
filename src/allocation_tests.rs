//! Test-only accounting of live Rust heap bytes.
//! Unlike RSS, this distinguishes retained allocations from allocator caches.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicIsize, Ordering};

struct CountingAllocator;
static PROCESS_LIVE: AtomicIsize = AtomicIsize::new(0);
thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
}
fn account(bytes: isize) {
    PROCESS_LIVE.fetch_add(bytes, Ordering::Relaxed);
    let _ = LIVE.try_with(|live| live.set(live.get() + bytes));
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            account(layout.size() as isize);
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            account(layout.size() as isize);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        account(-(layout.size() as isize));
        unsafe { System.dealloc(ptr, layout) };
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(ptr, layout, new_size) };
        if !next.is_null() {
            account(new_size as isize - layout.size() as isize);
        }
        next
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

// GUI tests are single-threaded and may run beside unrelated tests. VST3
// lifecycle tests serialize and count the whole process: wrapper workers can
// allocate on one thread and free on another, which defeats thread-only totals.
pub(crate) fn live_bytes(all_threads: bool) -> isize {
    if all_threads {
        PROCESS_LIVE.load(Ordering::Relaxed)
    } else {
        LIVE.with(Cell::get)
    }
}
