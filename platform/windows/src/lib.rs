//! NullIME TSF text input processor (docs/architecture.md "Windows").

mod candidate_window;
mod composition;
mod dictionary;
mod display_attribute;
mod keymap;
mod registration;
mod server;
mod service;

use std::sync::atomic::{AtomicIsize, Ordering};
use windows::core::{BOOL, HSTRING};
use windows::Win32::Foundation::{HINSTANCE, HMODULE};
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows::Win32::System::SystemServices::DLL_PROCESS_ATTACH;

static MODULE: AtomicIsize = AtomicIsize::new(0);

fn module() -> HMODULE {
    HMODULE(MODULE.load(Ordering::Relaxed) as *mut _)
}

/// Visible in DebugView / a debugger; the TIP runs inside other apps.
fn log(msg: &str) {
    unsafe { OutputDebugStringW(&HSTRING::from(format!("[nullime] {msg}\n"))) };
}

#[unsafe(no_mangle)]
extern "system" fn DllMain(instance: HINSTANCE, reason: u32, _reserved: *mut core::ffi::c_void) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(instance.0 as isize, Ordering::Relaxed);
    }
    true.into()
}
