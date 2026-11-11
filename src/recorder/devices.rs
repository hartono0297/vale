//! The audio endpoints a recording can use: the active microphones and
//! outputs by id and friendly name, for the device picker and the capture
//! thread. Every call needs COM on the calling thread and reads the devices
//! as they are right now; nothing is cached and nothing watches.

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eConsole,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ};

use super::capture::Source;

/// One audio endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    /// The endpoint id Windows gives (stable across reboots).
    pub id: String,
    /// The name Windows shows, such as "Microphone (Realtek Audio)".
    pub name: String,
}

/// The active endpoints of `source`'s kind: capture devices for the mic,
/// render devices for the PC audio. Empty when Windows cannot say.
pub fn list(source: Source) -> Vec<Endpoint> {
    enumerator().map_or_else(Vec::new, |e| list_in(&e, source))
}

/// The endpoint Windows uses for `source` now (the default for the console
/// role), `None` when there is none.
pub fn default_endpoint(source: Source) -> Option<Endpoint> {
    let enumerator = enumerator()?;
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(source.flow(), eConsole) }.ok()?;
    endpoint_of(&device)
}

/// The friendly name of the endpoint `id`, even when it is not active (an
/// unplugged device keeps its name in Windows). `None` for an unknown id.
pub fn name_of(id: &str) -> Option<String> {
    let enumerator = enumerator()?;
    let device = unsafe { enumerator.GetDevice(&windows::core::HSTRING::from(id)) }.ok()?;
    friendly_name(&device)
}

fn enumerator() -> Option<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.ok()
}

/// [`list`] with an enumerator the caller already has.
pub(super) fn list_in(enumerator: &IMMDeviceEnumerator, source: Source) -> Vec<Endpoint> {
    unsafe {
        let Ok(devices) = enumerator.EnumAudioEndpoints(source.flow(), DEVICE_STATE_ACTIVE) else {
            return Vec::new();
        };
        let count = devices.GetCount().unwrap_or(0);
        (0..count)
            .filter_map(|i| devices.Item(i).ok())
            .filter_map(|d| endpoint_of(&d))
            .collect()
    }
}

/// The ids of the active endpoints of `source`'s kind, without their names
/// (cheaper than [`list_in`]).
pub(super) fn ids_in(enumerator: &IMMDeviceEnumerator, source: Source) -> Vec<String> {
    unsafe {
        let Ok(devices) = enumerator.EnumAudioEndpoints(source.flow(), DEVICE_STATE_ACTIVE) else {
            return Vec::new();
        };
        let count = devices.GetCount().unwrap_or(0);
        (0..count)
            .filter_map(|i| devices.Item(i).ok())
            .filter_map(|d| device_id(&d))
            .collect()
    }
}

/// The id and the name of `device`.
pub(super) fn endpoint_of(device: &IMMDevice) -> Option<Endpoint> {
    let id = device_id(device)?;
    let name = friendly_name(device).unwrap_or_else(|| id.clone());
    Some(Endpoint { id, name })
}

/// The id of `device`.
pub(super) fn device_id(device: &IMMDevice) -> Option<String> {
    unsafe {
        let raw = device.GetId().ok()?;
        let id = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0.cast_const().cast()));
        id
    }
}

/// `PKEY_Device_FriendlyName` of `device`.
pub(super) fn friendly_name(device: &IMMDevice) -> Option<String> {
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let raw = PropVariantToStringAlloc(&value).ok()?;
        let name = raw.to_string().ok();
        CoTaskMemFree(Some(raw.0.cast_const().cast()));
        name.filter(|n| !n.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

    fn com() {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    }

    #[test]
    fn the_default_endpoint_is_one_of_the_active_ones_and_has_a_name() {
        com();
        for source in [Source::Mic, Source::System] {
            let all = list(source);
            let mut ids: Vec<&str> = all.iter().map(|e| e.id.as_str()).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), all.len(), "{source}: ids are unique");
            assert!(all.iter().all(|e| !e.name.is_empty()));
            if let Some(default) = default_endpoint(source) {
                assert!(
                    all.contains(&default),
                    "{source}: {default:?} not in {all:?}"
                );
                assert_eq!(name_of(&default.id), Some(default.name));
            }
        }
    }

    #[test]
    fn an_unknown_id_has_no_name() {
        com();
        assert_eq!(name_of("{0.0.0.00000000}.{not-a-device}"), None);
    }
}
