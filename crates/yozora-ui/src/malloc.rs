//! Allocator tuning for long-running apps.

/// Keeps large allocations out of the heap.
///
/// glibc serves allocations of 128 KiB and up with their own mappings, but each time one is
/// freed it raises that threshold to the freed size, up to 32 MiB. After a few frames' worth of
/// big temporary buffers, they're all served from the heap, and the heap keeps the freed space.
/// Fixing the threshold returns that memory to the system: about 4 MB for the bar, with no
/// change in frame timing.
pub fn tune() {
    #[cfg(target_env = "gnu")]
    {
        unsafe extern "C" {
            fn mallopt(param: std::ffi::c_int, value: std::ffi::c_int) -> std::ffi::c_int;
        }
        const M_MMAP_THRESHOLD: std::ffi::c_int = -3;
        // SAFETY: mallopt only changes allocator parameters.
        unsafe { mallopt(M_MMAP_THRESHOLD, 128 * 1024) };
    }
}
