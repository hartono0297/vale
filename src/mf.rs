//! Media Foundation, loaded at run time from System32 for the recorder's
//! writer, the transcript decoder and the player.
//!
//! Never import `mfplat.dll` or `mfreadwrite.dll` statically: Windows N
//! editions without the Media Feature Pack lack them, and a static import
//! would stop `vale.exe` from starting there. Callers map a missing loader
//! (`MfSession::start() == None`) to their own error.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::OnceLock;

use windows::Win32::Foundation::{E_POINTER, HMODULE};
use windows::Win32::Media::MediaFoundation::{MF_VERSION, MFSTARTUP_LITE};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{HRESULT, Interface, PCWSTR, s, w};

/// What `GetProcAddress` returns (`FARPROC`).
type Proc = unsafe extern "system" fn() -> isize;
type StartupFn = unsafe extern "system" fn(u32, u32) -> HRESULT;
type ShutdownFn = unsafe extern "system" fn() -> HRESULT;
type CreateAttributesFn = unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT;
type CreateObjectFn = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type CreateMemoryBufferFn = unsafe extern "system" fn(u32, *mut *mut c_void) -> HRESULT;
type CreateSinkWriterFn =
    unsafe extern "system" fn(PCWSTR, *mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT;
type CreateSourceReaderFn =
    unsafe extern "system" fn(PCWSTR, *mut c_void, *mut *mut c_void) -> HRESULT;

/// The Media Foundation entry points, resolved at run time. COM methods go
/// through the `windows` crate as usual.
pub struct Mf {
    pub startup: StartupFn,
    pub shutdown: ShutdownFn,
    pub create_attributes: CreateAttributesFn,
    pub create_media_type: CreateObjectFn,
    pub create_sample: CreateObjectFn,
    pub create_memory_buffer: CreateMemoryBufferFn,
    pub create_sink_writer: CreateSinkWriterFn,
    pub create_source_reader: CreateSourceReaderFn,
}

/// Loads the two DLLs once; `None` when either or an export is missing.
pub fn mf() -> Option<&'static Mf> {
    static MF: OnceLock<Option<Mf>> = OnceLock::new();
    MF.get_or_init(|| unsafe {
        let plat: HMODULE =
            LoadLibraryExW(w!("mfplat.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let rw: HMODULE =
            LoadLibraryExW(w!("mfreadwrite.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        // SAFETY: each target type is the documented signature of that export.
        Some(Mf {
            startup: std::mem::transmute::<Proc, StartupFn>(GetProcAddress(plat, s!("MFStartup"))?),
            shutdown: std::mem::transmute::<Proc, ShutdownFn>(GetProcAddress(
                plat,
                s!("MFShutdown"),
            )?),
            create_attributes: std::mem::transmute::<Proc, CreateAttributesFn>(GetProcAddress(
                plat,
                s!("MFCreateAttributes"),
            )?),
            create_media_type: std::mem::transmute::<Proc, CreateObjectFn>(GetProcAddress(
                plat,
                s!("MFCreateMediaType"),
            )?),
            create_sample: std::mem::transmute::<Proc, CreateObjectFn>(GetProcAddress(
                plat,
                s!("MFCreateSample"),
            )?),
            create_memory_buffer: std::mem::transmute::<Proc, CreateMemoryBufferFn>(
                GetProcAddress(plat, s!("MFCreateMemoryBuffer"))?,
            ),
            create_sink_writer: std::mem::transmute::<Proc, CreateSinkWriterFn>(GetProcAddress(
                rw,
                s!("MFCreateSinkWriterFromURL"),
            )?),
            create_source_reader: std::mem::transmute::<Proc, CreateSourceReaderFn>(
                GetProcAddress(rw, s!("MFCreateSourceReaderFromURL"))?,
            ),
        })
    })
    .as_ref()
}

/// Calls a creator that returns an interface through an out pointer.
pub fn created<T: Interface>(
    call: impl FnOnce(*mut *mut c_void) -> HRESULT,
) -> windows::core::Result<T> {
    let mut raw = null_mut();
    call(&mut raw).ok()?;
    if raw.is_null() {
        return Err(windows::core::Error::from_hresult(E_POINTER));
    }
    // SAFETY: on success the creator stored an owned reference of type T.
    Ok(unsafe { T::from_raw(raw) })
}

/// Per-thread COM (MTA where possible) and one `MFStartup`, undone on drop
/// in reverse order.
pub struct MfSession {
    mf: &'static Mf,
    com: bool,
}

impl MfSession {
    /// `None` when Media Foundation is missing or `MFStartup` fails.
    pub fn start() -> Option<Self> {
        let mf = mf()?;
        // S_FALSE (already initialised) still needs its CoUninitialize;
        // RPC_E_CHANGED_MODE (an STA thread) works as is and must not get one.
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
        if unsafe { (mf.startup)(MF_VERSION, MFSTARTUP_LITE) }.is_err() {
            if com {
                unsafe { CoUninitialize() };
            }
            return None;
        }
        Some(MfSession { mf, com })
    }

    /// The entry points this session started Media Foundation with.
    pub fn api(&self) -> &'static Mf {
        self.mf
    }
}

impl Drop for MfSession {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.mf.shutdown)();
            if self.com {
                CoUninitialize();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_foundation_loads_once() {
        assert!(mf().is_some());
        assert!(std::ptr::eq(mf().unwrap(), mf().unwrap()));
    }

    #[test]
    fn a_session_starts_and_stops() {
        assert!(MfSession::start().is_some());
        assert!(MfSession::start().is_some());
    }
}
