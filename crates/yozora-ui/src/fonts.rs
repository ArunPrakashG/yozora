//! Startup cost of the system font database.
//!
//! GPUI's text system parses every installed font face when it starts. With a few thousand
//! faces that takes over a second when the font files aren't in the page cache, which is what a
//! lock screen started after hours of idle sees. GPUI can keep the parsed list on disk
//! (`GPUI_FONT_DB_CACHE`) and restore it, with the fonts' headers prefetched in parallel, in a
//! few milliseconds. The cache is rebuilt by itself when a font directory changes, and every
//! Yozora app shares one file.

const CACHE: &str = "GPUI_FONT_DB_CACHE";

/// Turns on GPUI's parsed-font cache for this process, unless the environment already says
/// otherwise (`GPUI_FONT_DB_CACHE=0` turns it off, a path moves it).
///
/// Call it first thing in `main`, before any thread starts: it changes the environment.
pub fn cache_parsed() {
    if std::env::var_os(CACHE).is_some() {
        return;
    }
    // SAFETY: the caller runs this before any other thread exists, so nothing reads the
    // environment concurrently.
    unsafe { std::env::set_var(CACHE, "1") };
}
