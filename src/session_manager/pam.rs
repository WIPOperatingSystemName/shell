//! Linux-PAM ABI, confined to a fresh privileged worker for each login attempt.
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;

#[repr(C)]
struct Message { style: c_int, text: *const c_char }
#[repr(C)]
struct Response { text: *mut c_char, code: c_int }
#[repr(C)]
struct Conversation {
    callback: unsafe extern "C" fn(c_int, *const *const Message, *mut *mut Response, *mut c_void) -> c_int,
    data: *mut c_void,
}

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(service: *const c_char, user: *const c_char, conversation: *const Conversation, handle: *mut *mut c_void) -> c_int;
    fn pam_end(handle: *mut c_void, status: c_int) -> c_int;
    fn pam_authenticate(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_chauthtok(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_acct_mgmt(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_setcred(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_open_session(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_close_session(handle: *mut c_void, flags: c_int) -> c_int;
    fn pam_set_item(handle: *mut c_void, item: c_int, value: *const c_void) -> c_int;
    fn pam_get_item(handle: *mut c_void, item: c_int, value: *mut *const c_void) -> c_int;
    fn pam_putenv(handle: *mut c_void, value: *const c_char) -> c_int;
    fn pam_getenv(handle: *mut c_void, name: *const c_char) -> *const c_char;
    fn calloc(count: usize, size: usize) -> *mut c_void;
    fn free(value: *mut c_void);
}

// Passwords never enter arguments, the environment, reports or the journal.
pub(super) struct Secret(pub Vec<u8>);
impl Secret {
    fn clear(&mut self) {
        for byte in &mut self.0 { unsafe { ptr::write_volatile(byte, 0) }; }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
        self.0.clear();
    }
}
impl Drop for Secret {
    fn drop(&mut self) { self.clear(); }
}

unsafe extern "C" fn converse(count: c_int, messages: *const *const Message, result: *mut *mut Response, data: *mut c_void) -> c_int {
    // Linux-PAM owns successful responses and frees them with libc free().
    unsafe {
        if !(1..=32).contains(&count) || messages.is_null() || result.is_null() || data.is_null() { return 19; }
        *result = ptr::null_mut();
        let secret = &*(data as *const Secret);
        let responses = calloc(count as usize, size_of::<Response>()) as *mut Response;
        if responses.is_null() { return 5; }
        for index in 0..count as usize {
            let message = *messages.add(index);
            let valid = !message.is_null() && match (*message).style {
                1 => {
                    let text = calloc(secret.0.len() + 1, 1) as *mut c_char;
                    if !text.is_null() {
                        ptr::copy_nonoverlapping(secret.0.as_ptr(), text.cast(), secret.0.len());
                        (*responses.add(index)).text = text;
                    }
                    !text.is_null()
                }
                3 | 4 => true,
                // The username is supplied explicitly. Interactive challenges beyond
                // the local password policy require a richer conversation protocol.
                _ => false,
            };
            if !valid {
                for previous in 0..=index { free((*responses.add(previous)).text.cast()); }
                free(responses.cast());
                return 19;
            }
        }
        *result = responses;
        0
    }
}

pub(super) struct Session {
    handle: *mut c_void,
    credentials: bool,
    opened: bool,
    // PAM may invoke the conversation until pam_end; retain stable storage.
    _secret: Box<Secret>,
}
impl Session {
    pub fn set_password(user: &str, secret: Secret) -> Result<(), String> {
        let user = CString::new(user).map_err(|_| "Invalid user")?;
        let mut secret = Box::new(secret);
        let conversation = Conversation { callback: converse, data: (&mut *secret as *mut Secret).cast() };
        let mut handle = ptr::null_mut();
        unsafe {
            check(pam_start(c"telorgon-password".as_ptr(), user.as_ptr(), &conversation, &mut handle), "start password change")?;
            let mut session = Self { handle, credentials: false, opened: false, _secret: secret };
            let result = check(pam_chauthtok(handle, 0), "change password");
            session._secret.clear();
            result
        }
    }
    pub fn open(user: &str, secret: Secret) -> Result<Self, String> {
        let user = CString::new(user).map_err(|_| "Invalid user")?;
        let mut secret = Box::new(secret);
        let conversation = Conversation { callback: converse, data: (&mut *secret as *mut Secret).cast() };
        let mut handle = ptr::null_mut();
        unsafe {
            check(pam_start(c"telorgon-session".as_ptr(), user.as_ptr(), &conversation, &mut handle), "start")?;
            let mut session = Self { handle, credentials: false, opened: false, _secret: secret };
            check(pam_set_item(handle, 3, c"/dev/tty1".as_ptr().cast()), "tty")?;
            check(pam_authenticate(handle, 1), "authenticate")?; // Reject empty passwords.
            session._secret.clear();
            check(pam_acct_mgmt(handle, 0), "account")?;
            let mut authenticated = ptr::null();
            check(pam_get_item(handle, 2, &mut authenticated), "user")?;
            if authenticated.is_null() || CStr::from_ptr(authenticated.cast()) != user.as_c_str() {
                return Err("PAM changed the requested local user".into());
            }
            check(pam_setcred(handle, 2), "establish credentials")?;
            session.credentials = true;
            for entry in [c"XDG_SESSION_TYPE=wayland", c"XDG_SESSION_CLASS=user", c"XDG_SESSION_DESKTOP=telorgon", c"XDG_SEAT=seat0", c"XDG_VTNR=1"] {
                check(pam_putenv(handle, entry.as_ptr()), "environment")?;
            }
            check(pam_open_session(handle, 0), "open session")?;
            session.opened = true;
            Ok(session)
        }
    }
    pub fn environment(&self, name: &CStr) -> Result<String, String> {
        let value = unsafe { pam_getenv(self.handle, name.as_ptr()) };
        if value.is_null() { return Err(format!("PAM did not supply {}", name.to_string_lossy())); }
        unsafe { CStr::from_ptr(value) }.to_str().map(str::to_owned).map_err(|_| "Invalid PAM environment".into())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            if self.opened { pam_close_session(self.handle, 0); }
            if self.credentials { pam_setcred(self.handle, 4); }
            pam_end(self.handle, 0);
        }
    }
}
fn check(status: c_int, operation: &str) -> Result<(), String> {
    if status == 0 { Ok(()) } else { Err(format!("PAM {operation} failed ({status})")) }
}
