//! Media Foundation, loaded at run time. Windows "N" editions have no
//! `mfplat.dll` until the Media Feature Pack is installed; a static import
//! would stop `vale.exe` from starting there at all. So the few flat
//! functions are looked up with `GetProcAddress` the first time a recording
//! starts, and a missing DLL is a clear [`Error::NoMediaFoundation`]. The
//! COM interfaces need no import.

use std::ffi::c_void;
use std::sync::OnceLock;

use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFDXGIDeviceManager, IMFMediaBuffer, IMFMediaType, IMFSample, IMFSinkWriter,
    IMFSourceReader, MF_VERSION, MFSTARTUP_LITE,
};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{GUID, HRESULT, Interface, PCSTR, PCWSTR, s, w};

use super::Error;

type Startup = unsafe extern "system" fn(u32, u32) -> HRESULT;
type Shutdown = unsafe extern "system" fn() -> HRESULT;
type CreateAttributes = unsafe extern "system" fn(*mut *mut c_void, u32) -> HRESULT;
type CreateOut = unsafe extern "system" fn(*mut *mut c_void) -> HRESULT;
type CreateMemoryBuffer = unsafe extern "system" fn(u32, *mut *mut c_void) -> HRESULT;
type CreateDxgiDeviceManager = unsafe extern "system" fn(*mut u32, *mut *mut c_void) -> HRESULT;
type CreateAllocatorEx = unsafe extern "system" fn(*const GUID, *mut *mut c_void) -> HRESULT;
type CreateSinkWriter =
    unsafe extern "system" fn(PCWSTR, *mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT;
type CreateSourceReader =
    unsafe extern "system" fn(PCWSTR, *mut c_void, *mut *mut c_void) -> HRESULT;

/// The flat Media Foundation functions the recorder uses.
pub struct Mf {
    startup: Startup,
    shutdown: Shutdown,
    create_attributes: CreateAttributes,
    create_media_type: CreateOut,
    create_sample: CreateOut,
    create_memory_buffer: CreateMemoryBuffer,
    create_dxgi_device_manager: CreateDxgiDeviceManager,
    create_video_sample_allocator_ex: CreateAllocatorEx,
    create_sink_writer: CreateSinkWriter,
    create_source_reader: CreateSourceReader,
}

/// Loads Media Foundation once per process.
pub fn get() -> Result<&'static Mf, Error> {
    static MF: OnceLock<Option<Mf>> = OnceLock::new();
    MF.get_or_init(|| unsafe { load() })
        .as_ref()
        .ok_or(Error::NoMediaFoundation)
}

unsafe fn load() -> Option<Mf> {
    unsafe {
        let plat = LoadLibraryExW(w!("mfplat.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let rw = LoadLibraryExW(w!("mfreadwrite.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let f = |module, name: PCSTR| GetProcAddress(module, name);
        // SAFETY: each signature matches the export's documented prototype.
        Some(Mf {
            startup: std::mem::transmute::<_, Startup>(f(plat, s!("MFStartup"))?),
            shutdown: std::mem::transmute::<_, Shutdown>(f(plat, s!("MFShutdown"))?),
            create_attributes: std::mem::transmute::<_, CreateAttributes>(f(
                plat,
                s!("MFCreateAttributes"),
            )?),
            create_media_type: std::mem::transmute::<_, CreateOut>(f(
                plat,
                s!("MFCreateMediaType"),
            )?),
            create_sample: std::mem::transmute::<_, CreateOut>(f(plat, s!("MFCreateSample"))?),
            create_memory_buffer: std::mem::transmute::<_, CreateMemoryBuffer>(f(
                plat,
                s!("MFCreateMemoryBuffer"),
            )?),
            create_dxgi_device_manager: std::mem::transmute::<_, CreateDxgiDeviceManager>(f(
                plat,
                s!("MFCreateDXGIDeviceManager"),
            )?),
            create_video_sample_allocator_ex: std::mem::transmute::<_, CreateAllocatorEx>(f(
                plat,
                s!("MFCreateVideoSampleAllocatorEx"),
            )?),
            create_sink_writer: std::mem::transmute::<_, CreateSinkWriter>(f(
                rw,
                s!("MFCreateSinkWriterFromURL"),
            )?),
            create_source_reader: std::mem::transmute::<_, CreateSourceReader>(f(
                rw,
                s!("MFCreateSourceReaderFromURL"),
            )?),
        })
    }
}

/// Turns an out pointer filled by a factory into the interface.
unsafe fn take<T: Interface>(hr: HRESULT, raw: *mut c_void) -> windows::core::Result<T> {
    hr.ok()?;
    // SAFETY: on success the factory handed over one reference to a `T`.
    Ok(unsafe { T::from_raw(raw) })
}

/// Media Foundation started; shut down again on drop (the calls are counted).
pub struct Session(&'static Mf);

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.0.shutdown)();
        }
    }
}

impl Mf {
    pub fn start(&'static self) -> Result<Session, Error> {
        unsafe { (self.startup)(MF_VERSION, MFSTARTUP_LITE) }
            .ok()
            .map_err(|_| Error::NoMediaFoundation)?;
        Ok(Session(self))
    }

    pub fn attributes(&self, size: u32) -> windows::core::Result<IMFAttributes> {
        let mut raw = std::ptr::null_mut();
        unsafe { take((self.create_attributes)(&mut raw, size), raw) }
    }

    pub fn media_type(&self) -> windows::core::Result<IMFMediaType> {
        let mut raw = std::ptr::null_mut();
        unsafe { take((self.create_media_type)(&mut raw), raw) }
    }

    pub fn sample(&self) -> windows::core::Result<IMFSample> {
        let mut raw = std::ptr::null_mut();
        unsafe { take((self.create_sample)(&mut raw), raw) }
    }

    pub fn memory_buffer(&self, len: u32) -> windows::core::Result<IMFMediaBuffer> {
        let mut raw = std::ptr::null_mut();
        unsafe { take((self.create_memory_buffer)(len, &mut raw), raw) }
    }

    /// A DXGI device manager and its reset token.
    pub fn dxgi_device_manager(&self) -> windows::core::Result<(IMFDXGIDeviceManager, u32)> {
        let mut token = 0u32;
        let mut raw = std::ptr::null_mut();
        let manager =
            unsafe { take((self.create_dxgi_device_manager)(&mut token, &mut raw), raw)? };
        Ok((manager, token))
    }

    pub fn video_sample_allocator<T: Interface>(&self) -> windows::core::Result<T> {
        let mut raw = std::ptr::null_mut();
        unsafe {
            take(
                (self.create_video_sample_allocator_ex)(&T::IID, &mut raw),
                raw,
            )
        }
    }

    pub fn sink_writer(
        &self,
        url: PCWSTR,
        attributes: &IMFAttributes,
    ) -> windows::core::Result<IMFSinkWriter> {
        let mut raw = std::ptr::null_mut();
        unsafe {
            take(
                (self.create_sink_writer)(url, std::ptr::null_mut(), attributes.as_raw(), &mut raw),
                raw,
            )
        }
    }

    pub fn source_reader(
        &self,
        url: PCWSTR,
        attributes: Option<&IMFAttributes>,
    ) -> windows::core::Result<IMFSourceReader> {
        let mut raw = std::ptr::null_mut();
        let attrs = attributes.map_or(std::ptr::null_mut(), |a| a.as_raw());
        unsafe { take((self.create_source_reader)(url, attrs, &mut raw), raw) }
    }
}
