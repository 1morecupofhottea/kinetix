//! Optional allocation accounting (NFR-1.8: "allocations per request where
//! measurable").
//!
//! When the `alloc-stats` cargo feature is enabled, `main` installs
//! [`CountingAllocator`] as the global allocator and these counters track every
//! allocation. When the feature is off the counters stay zero and nothing is
//! measured, so the default build pays no cost and the metric honestly reports
//! "not measured" (0).

use std::sync::atomic::{AtomicU64, Ordering};

pub static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
pub static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

/// Total allocation calls since start (0 unless built with `alloc-stats`).
pub fn allocations() -> u64 {
    ALLOCATIONS.load(Ordering::Relaxed)
}

/// Total bytes allocated since start (0 unless built with `alloc-stats`).
pub fn alloc_bytes() -> u64 {
    ALLOC_BYTES.load(Ordering::Relaxed)
}

/// A `#[global_allocator]` that counts allocations and bytes, delegating to the
/// system allocator. Installed by `main` only when `alloc-stats` is enabled.
#[cfg(feature = "alloc-stats")]
pub struct CountingAllocator;

#[cfg(feature = "alloc-stats")]
unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        std::alloc::System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        std::alloc::System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        std::alloc::System.realloc(ptr, layout, new_size)
    }
}
