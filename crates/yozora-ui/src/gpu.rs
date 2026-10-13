//! Keeps the OpenGL stack out of the process when Vulkan will do.
//!
//! GPUI's renderer asks wgpu for both Vulkan and GL. Asking for GL loads EGL through glvnd, and
//! glvnd loads every EGL vendor it finds: on an NVIDIA machine with Mesa installed that's the
//! NVIDIA EGL libraries plus libLLVM and Gallium, about 40 MB of RSS (10 MB of it private) that
//! is never used, because Vulkan wins. Pointing glvnd at an empty vendor list skips all of it.
//!
//! If Vulkan then turns out not to work, the app restarts itself with GL allowed
//! ([`restart_with_gl`]). Setting `YOZORA_GL=1` skips the whole thing.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

const VENDORS: &str = "__EGL_VENDOR_LIBRARY_FILENAMES";
const ALLOW_GL: &str = "YOZORA_GL";

static GL_HIDDEN: AtomicBool = AtomicBool::new(false);

/// Hides the EGL vendors from this process if a Vulkan driver is installed.
///
/// Call it first thing in `main`, before any thread starts: it changes the environment.
pub fn prefer_vulkan() {
    let set = |name| std::env::var_os(name).is_some();
    if set(ALLOW_GL) || set(VENDORS) || set("__EGL_VENDOR_LIBRARY_DIRS") || !vulkan_installed() {
        return;
    }
    // SAFETY: the caller runs this before any other thread exists, so nothing reads the
    // environment concurrently.
    unsafe { std::env::set_var(VENDORS, "/dev/null") };
    GL_HIDDEN.store(true, Ordering::Relaxed);
}

/// Whether [`prefer_vulkan`] hid GL from this process.
pub fn gl_hidden() -> bool {
    GL_HIDDEN.load(Ordering::Relaxed)
}

/// Undoes [`prefer_vulkan`] for a child process, so the commands and apps Yozora starts see the
/// user's own environment.
pub fn child(command: &mut Command) -> &mut Command {
    if gl_hidden() {
        command.env_remove(VENDORS);
    }
    command
}

/// Replaces this process with a fresh copy of itself that may use GL. Only returns if that
/// fails. Call it when the first window can't open with GL hidden.
pub fn restart_with_gl() -> std::io::Error {
    use std::os::unix::process::CommandExt;
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return e,
    };
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    Command::new(exe).args(args).env_remove(VENDORS).env(ALLOW_GL, "1").exec()
}

/// Whether the Vulkan loader would find a driver, by the same search it uses.
fn vulkan_installed() -> bool {
    if std::env::var_os("VK_ICD_FILENAMES").is_some() || std::env::var_os("VK_DRIVER_FILES").is_some() {
        return true;
    }
    let mut dirs: Vec<PathBuf> = vec!["/etc/vulkan/icd.d".into()];
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/share")));
    dirs.extend(data_home.map(|d| d.join("vulkan/icd.d")));
    let data_dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(data_dirs.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join("vulkan/icd.d")));
    dirs.iter().any(|dir| {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries.flatten().any(|e| e.path().extension().is_some_and(|x| x == "json"))
        })
    })
}
