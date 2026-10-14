//! Password checks through PAM, the same way swaylock and hyprlock do it: `pam_authenticate`,
//! then `pam_setcred` to refresh credentials such as Kerberos tickets.
//!
//! Blocking, and slow on a wrong password (`pam_unix` waits about two seconds), so run it on
//! a worker thread.

use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::{Path, PathBuf},
    ptr,
};

use zeroize::Zeroizing;

const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_CONV_ERR: c_int = 19;
const PAM_MAXTRIES: c_int = 11;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_PROMPT_ECHO_ON: c_int = 2;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
const PAM_REFRESH_CRED: c_int = 0x0010;

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type ConvFn = unsafe extern "C" fn(c_int, *mut *const PamMessage, *mut *mut PamResponse, *mut c_void) -> c_int;

#[repr(C)]
struct PamConv {
    conv: ConvFn,
    appdata_ptr: *mut c_void,
}

#[repr(C)]
struct PamHandle {
    _private: [u8; 0],
}

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(service: *const c_char, user: *const c_char, conv: *const PamConv, pamh: *mut *mut PamHandle)
    -> c_int;
    fn pam_start_confdir(
        service: *const c_char,
        user: *const c_char,
        conv: *const PamConv,
        confdir: *const c_char,
        pamh: *mut *mut PamHandle,
    ) -> c_int;
    fn pam_authenticate(pamh: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_setcred(pamh: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_end(pamh: *mut PamHandle, status: c_int) -> c_int;
    fn pam_strerror(pamh: *mut PamHandle, errnum: c_int) -> *const c_char;
}

/// Why a check failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// What PAM's modules said, such as pam_faillock's "The account is locked…". Empty when
    /// they said nothing, which usually means a wrong password.
    pub messages: Vec<String>,
    /// PAM refused more tries for now, for example because faillock locked the account.
    pub max_tries: bool,
    /// PAM's own description of the error, for the log.
    pub error: String,
}

/// Where PAM's configuration comes from.
#[derive(Debug, Clone)]
pub struct Service {
    pub name: String,
    /// A directory to read `name` from instead of `/etc/pam.d`. For tests.
    pub confdir: Option<PathBuf>,
}

impl Service {
    /// `yozora-lock` when its PAM file is installed, `login` otherwise. A missing service
    /// falls through to PAM's `other`, which denies everything, so it has to exist.
    pub fn system() -> Self {
        let installed = ["/etc/pam.d/yozora-lock", "/usr/lib/pam.d/yozora-lock"].iter().any(|p| Path::new(p).exists());
        let name = if installed { "yozora-lock" } else { "login" };
        Self { name: name.into(), confdir: None }
    }
}

struct Conversation<'a> {
    password: &'a CStr,
    messages: Vec<String>,
}

/// Answers PAM's prompts: the password for every hidden prompt, and nothing for others.
unsafe extern "C" fn converse(
    num_msg: c_int,
    msg: *mut *const PamMessage,
    resp: *mut *mut PamResponse,
    appdata: *mut c_void,
) -> c_int {
    if num_msg <= 0 || msg.is_null() || resp.is_null() || appdata.is_null() {
        return PAM_CONV_ERR;
    }
    let conv = unsafe { &mut *(appdata as *mut Conversation) };
    let n = num_msg as usize;
    // PAM frees the array and each answer with free(), so they come from calloc and strdup.
    let replies = unsafe { libc::calloc(n, std::mem::size_of::<PamResponse>()) } as *mut PamResponse;
    if replies.is_null() {
        return PAM_BUF_ERR;
    }
    for i in 0..n {
        // Linux-PAM passes an array of pointers to messages.
        let m = unsafe { &**msg.add(i) };
        let text = if m.msg.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(m.msg) }.to_string_lossy().into_owned()
        };
        match m.msg_style {
            PAM_PROMPT_ECHO_OFF => {
                let copy = unsafe { libc::strdup(conv.password.as_ptr()) };
                if copy.is_null() {
                    free_replies(replies, i);
                    return PAM_BUF_ERR;
                }
                unsafe { (*replies.add(i)).resp = copy };
            }
            // A visible prompt asks for something other than the password, such as a username
            // or an OTP. The lock screen has no field for it, so it answers empty.
            PAM_PROMPT_ECHO_ON => {
                let copy = unsafe { libc::strdup(c"".as_ptr()) };
                if copy.is_null() {
                    free_replies(replies, i);
                    return PAM_BUF_ERR;
                }
                unsafe { (*replies.add(i)).resp = copy };
            }
            PAM_ERROR_MSG | PAM_TEXT_INFO => {
                let text = text.trim();
                if !text.is_empty() {
                    conv.messages.push(text.to_string());
                }
            }
            _ => {
                free_replies(replies, i);
                return PAM_CONV_ERR;
            }
        }
    }
    unsafe { *resp = replies };
    PAM_SUCCESS
}

/// Frees the first `filled` answers and the array, wiping passwords first.
fn free_replies(replies: *mut PamResponse, filled: usize) {
    for i in 0..filled {
        let r = unsafe { (*replies.add(i)).resp };
        if !r.is_null() {
            unsafe {
                let len = libc::strlen(r);
                ptr::write_bytes(r, 0, len);
                libc::free(r as *mut c_void);
            }
        }
    }
    unsafe { libc::free(replies as *mut c_void) };
}

/// Checks `password` for `user`. Blocking.
pub fn authenticate(service: &Service, user: &str, password: &Zeroizing<String>) -> Result<(), Failure> {
    let fail = |error: String| Failure { messages: Vec::new(), max_tries: false, error };
    let name = CString::new(service.name.as_str()).map_err(|_| fail("bad service name".into()))?;
    let user = CString::new(user).map_err(|_| fail("bad user name".into()))?;
    // A NUL can't be typed, but check anyway: CString would refuse it.
    let password = Zeroizing::new(
        CString::new(password.as_bytes()).map_err(|_| fail("bad password".into()))?.into_bytes_with_nul(),
    );
    let password = CStr::from_bytes_with_nul(&password).map_err(|_| fail("bad password".into()))?;

    let mut conv_state = Conversation { password, messages: Vec::new() };
    let conv = PamConv { conv: converse, appdata_ptr: &mut conv_state as *mut Conversation as *mut c_void };
    let mut handle: *mut PamHandle = ptr::null_mut();
    let started = match &service.confdir {
        Some(dir) => {
            let dir = CString::new(dir.as_os_str().as_encoded_bytes()).map_err(|_| fail("bad PAM directory".into()))?;
            unsafe { pam_start_confdir(name.as_ptr(), user.as_ptr(), &conv, dir.as_ptr(), &mut handle) }
        }
        None => unsafe { pam_start(name.as_ptr(), user.as_ptr(), &conv, &mut handle) },
    };
    if started != PAM_SUCCESS || handle.is_null() {
        return Err(fail(format!("pam_start failed ({started})")));
    }

    let status = unsafe { pam_authenticate(handle, 0) };
    if status == PAM_SUCCESS {
        // Failing to refresh credentials doesn't keep anyone out; log it and carry on.
        let cred = unsafe { pam_setcred(handle, PAM_REFRESH_CRED) };
        if cred != PAM_SUCCESS {
            tracing::warn!("pam_setcred: {}", strerror(handle, cred));
        }
    }
    let error = strerror(handle, status);
    unsafe { pam_end(handle, status) };
    if status == PAM_SUCCESS {
        Ok(())
    } else {
        Err(Failure { messages: conv_state.messages, max_tries: status == PAM_MAXTRIES, error })
    }
}

fn strerror(handle: *mut PamHandle, status: c_int) -> String {
    let s = unsafe { pam_strerror(handle, status) };
    if s.is_null() {
        format!("PAM error {status}")
    } else {
        unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PAM directory with a service that accepts everything and one that denies everything.
    fn confdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yozora-lock-pam-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("allow"), "auth required pam_permit.so\naccount required pam_permit.so\n").unwrap();
        std::fs::write(dir.join("deny"), "auth required pam_deny.so\n").unwrap();
        dir
    }

    fn service(name: &str) -> Service {
        Service { name: name.into(), confdir: Some(confdir()) }
    }

    #[test]
    fn permit_and_deny() {
        let pw = Zeroizing::new("hunter2".to_string());
        assert_eq!(authenticate(&service("allow"), "nobody", &pw), Ok(()));
        let err = authenticate(&service("deny"), "nobody", &pw).unwrap_err();
        assert!(!err.max_tries);
        assert!(!err.error.is_empty());
    }

    #[test]
    fn missing_service_denies() {
        // Falls through to `other` in our directory, which doesn't exist: denied, not allowed.
        let pw = Zeroizing::new("x".to_string());
        assert!(authenticate(&service("no-such-service"), "nobody", &pw).is_err());
    }
}
