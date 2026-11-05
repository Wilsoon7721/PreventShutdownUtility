use std::ffi::c_void;
use std::sync::OnceLock;

use windows_sys::core::{GUID, HRESULT};
use windows_sys::Win32::Media::Audio::{eConsole, eRender, MMDeviceEnumerator};
use windows_sys::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};

/// Anything quieter than this is treated as inaudible
const MIN_AUDIBLE_VOLUME: f32 = 0.10;

const IID_IMMDEVICE_ENUMERATOR: GUID = GUID::from_u128(0xa95664d2_9614_4f35_a746_de8db63617e6);
const IID_IAUDIO_ENDPOINT_VOLUME: GUID = GUID::from_u128(0x5cdf2c82_841e_4546_9722_0cf74078229a);

#[repr(C)]
struct EnumeratorVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    enum_audio_endpoints: usize,
    get_default_audio_endpoint:
        unsafe extern "system" fn(*mut c_void, i32, i32, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct DeviceVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    activate: unsafe extern "system" fn(
        *mut c_void,
        *const GUID,
        u32,
        *const c_void,
        *mut *mut c_void,
    ) -> HRESULT,
}

#[repr(C)]
struct EndpointVolumeVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    register_control_change_notify: usize,
    unregister_control_change_notify: usize,
    get_channel_count: usize,
    set_master_volume_level: usize,
    set_master_volume_level_scalar: usize,
    get_master_volume_level: usize,
    get_master_volume_level_scalar: unsafe extern "system" fn(*mut c_void, *mut f32) -> HRESULT,
    set_channel_volume_level: usize,
    set_channel_volume_level_scalar: usize,
    get_channel_volume_level: usize,
    get_channel_volume_level_scalar: usize,
    set_mute: usize,
    get_mute: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
}

struct Com<T>(
    *mut c_void,
    unsafe extern "system" fn(*mut c_void) -> u32,
    std::marker::PhantomData<T>,
);

impl<T> Drop for Com<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { (self.1)(self.0) };
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Audibility {
    Audible {
        volume: f32,
    },
    Muted {
        volume: f32,
    },
    TooQuiet {
        volume: f32,
    },
    NoDevice {
        hresult: HRESULT,
    },
    Unavailable {
        hresult: HRESULT,
        stage: &'static str,
    },
}

impl Audibility {
    pub fn can_be_heard(&self) -> bool {
        matches!(self, Audibility::Audible { .. })
    }

    pub fn describe(&self) -> String {
        match self {
            Audibility::Audible { volume } => {
                format!("audible (volume {}%)", (volume * 100.0).round())
            }
            Audibility::Muted { volume } => {
                format!("NOT audible: muted (volume {}%)", (volume * 100.0).round())
            }
            Audibility::TooQuiet { volume } => {
                format!("NOT audible: volume only {}%", (volume * 100.0).round())
            }
            Audibility::NoDevice { hresult } => format!(
                "NOT audible: no playback device (hr=0x{hresult:08X}) - no speakers, or the Windows Audio service is stopped"
            ),
            Audibility::Unavailable { hresult, stage } => {
                format!("NOT audible: audio unavailable at {stage} (hr=0x{hresult:08X})")
            }
        }
    }
}

fn com_ready() -> bool {
    static INIT: OnceLock<bool> = OnceLock::new();
    *INIT.get_or_init(|| {
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        hr >= 0 || hr == -2147417850 /* RPC_E_CHANGED_MODE */
    })
}

/// Asks the default playback endpoint whether a sound would be heard
pub fn probe() -> Audibility {
    if !com_ready() {
        return Audibility::Unavailable {
            hresult: -1,
            stage: "CoInitializeEx",
        };
    }
    unsafe {
        let mut raw: *mut c_void = std::ptr::null_mut();
        let hr = CoCreateInstance(
            &MMDeviceEnumerator,
            std::ptr::null_mut(),
            CLSCTX_ALL,
            &IID_IMMDEVICE_ENUMERATOR,
            &mut raw,
        );
        if hr < 0 || raw.is_null() {
            return Audibility::Unavailable {
                hresult: hr,
                stage: "CoCreateInstance(MMDeviceEnumerator)",
            };
        }
        let evt = (*raw.cast::<*const EnumeratorVtbl>()).read();
        let _enumerator = Com::<EnumeratorVtbl>(raw, evt.release, std::marker::PhantomData);

        let mut device: *mut c_void = std::ptr::null_mut();
        let hr = (evt.get_default_audio_endpoint)(raw, eRender, eConsole, &mut device);
        if hr < 0 || device.is_null() {
            return Audibility::NoDevice { hresult: hr };
        }
        let dvt = (*device.cast::<*const DeviceVtbl>()).read();
        let _device = Com::<DeviceVtbl>(device, dvt.release, std::marker::PhantomData);

        let mut vol: *mut c_void = std::ptr::null_mut();
        let hr = (dvt.activate)(
            device,
            &IID_IAUDIO_ENDPOINT_VOLUME,
            CLSCTX_ALL,
            std::ptr::null(),
            &mut vol,
        );
        if hr < 0 || vol.is_null() {
            return Audibility::Unavailable {
                hresult: hr,
                stage: "IMMDevice::Activate(IAudioEndpointVolume)",
            };
        }
        let vvt = (*vol.cast::<*const EndpointVolumeVtbl>()).read();
        let _vol = Com::<EndpointVolumeVtbl>(vol, vvt.release, std::marker::PhantomData);

        let mut level: f32 = 0.0;
        let hr = (vvt.get_master_volume_level_scalar)(vol, &mut level);
        if hr < 0 {
            return Audibility::Unavailable {
                hresult: hr,
                stage: "GetMasterVolumeLevelScalar",
            };
        }
        let mut muted: i32 = 0;
        let hr = (vvt.get_mute)(vol, &mut muted);
        if hr < 0 {
            return Audibility::Unavailable {
                hresult: hr,
                stage: "GetMute",
            };
        }

        if muted != 0 {
            Audibility::Muted { volume: level }
        } else if level < MIN_AUDIBLE_VOLUME {
            Audibility::TooQuiet { volume: level }
        } else {
            Audibility::Audible { volume: level }
        }
    }
}
