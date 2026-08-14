//! Version, ping, and device info handlers.

use defmt::info;
use pico_de_gallo_internal::{
    Capabilities, DeviceInfo, SCHEMA_VERSION_MAJOR, SCHEMA_VERSION_MINOR, SCHEMA_VERSION_PATCH, VersionInfo,
};
use postcard_rpc::header::VarHeader;

use crate::context::{Context, NUM_GPIOS};
use crate::{HW_VERSION, VERSION_MAJOR, VERSION_MINOR, VERSION_PATCH};

/// Handler for the `ping` endpoint — echoes back the received `u32`.
pub(crate) fn ping_handler(_context: &mut Context, _header: VarHeader, rqst: u32) -> u32 {
    info!("ping: {=u32:#x}", rqst);
    rqst
}

/// Handler for `version` — returns the firmware version.
pub(crate) async fn version_handler(_context: &mut Context, _header: VarHeader, _req: ()) -> VersionInfo {
    VersionInfo {
        major: VERSION_MAJOR,
        minor: VERSION_MINOR,
        patch: VERSION_PATCH,
    }
}

/// Handler for `device/info` — returns firmware version, schema version,
/// hardware version, and peripheral capabilities.
pub(crate) fn device_info_handler(_context: &mut Context, _header: VarHeader, _req: ()) -> DeviceInfo {
    #[cfg(feature = "hw-rev1")]
    let capabilities = Capabilities::I2C | Capabilities::SPI | Capabilities::GPIO | Capabilities::PWM;

    #[cfg(feature = "hw-rev2")]
    let capabilities = Capabilities::I2C
        | Capabilities::SPI
        | Capabilities::UART
        | Capabilities::GPIO
        | Capabilities::PWM
        | Capabilities::ADC
        | Capabilities::ONEWIRE;

    DeviceInfo {
        fw_major: VERSION_MAJOR,
        fw_minor: VERSION_MINOR,
        fw_patch: VERSION_PATCH,
        schema_major: SCHEMA_VERSION_MAJOR,
        schema_minor: SCHEMA_VERSION_MINOR,
        schema_patch: SCHEMA_VERSION_PATCH,
        hw_version: HW_VERSION,
        capabilities,
        num_gpios: NUM_GPIOS as u8,
    }
}
