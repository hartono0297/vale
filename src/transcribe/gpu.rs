//! Whether this PC has a real graphics card, which decides the default
//! model (turbo with a GPU, small without). DXGI lists the adapters; the
//! software ones (WARP, Microsoft Basic Render) do not count.

use std::sync::OnceLock;

use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
};

/// Microsoft Basic Render Driver.
const MICROSOFT_VENDOR: u32 = 0x1414;
const BASIC_RENDER_DEVICE: u32 = 0x8C;

/// Whether an adapter with these description fields is a hardware GPU.
pub fn is_hardware(flags: u32, vendor: u32, device: u32) -> bool {
    flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0
        && !(vendor == MICROSOFT_VENDOR && device == BASIC_RENDER_DEVICE)
}

/// Whether DXGI lists at least one hardware adapter. Asked once per run.
pub fn has_hardware_gpu() -> bool {
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(detect)
}

fn detect() -> bool {
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return false;
        };
        let mut index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            if let Ok(desc) = adapter.GetDesc1()
                && is_hardware(desc.Flags, desc.VendorId, desc.DeviceId)
            {
                return true;
            }
            index += 1;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn software_adapters_do_not_count() {
        assert!(!is_hardware(2, 0x10DE, 0x2504));
        assert!(!is_hardware(0, 0x1414, 0x8C));
        assert!(is_hardware(0, 0x10DE, 0x2504));
        assert!(is_hardware(0, 0x8086, 0x46A6));
    }
}
