#![cfg_attr(not(feature = "use-std"), no_std)]

use postcard_rpc::{TopicDirection, endpoints, topics};
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

pub const MICROSOFT_VID: u16 = 0x045e;
pub const PICO_DE_GALLO_PID: u16 = 0x067d;

// ---

pub type I2cWriteResponse = Result<(), I2cWriteFail>;

#[cfg(feature = "use-std")]
pub type I2cReadResponse<'a> = Result<Vec<u8>, I2cReadFail>;
#[cfg(not(feature = "use-std"))]
pub type I2cReadResponse<'a> = Result<&'a [u8], I2cReadFail>;

pub type SpiWriteResponse = Result<(), SpiWriteFail>;

#[cfg(feature = "use-std")]
pub type SpiReadResponse<'a> = Result<Vec<u8>, SpiReadFail>;
#[cfg(not(feature = "use-std"))]
pub type SpiReadResponse<'a> = Result<&'a [u8], SpiReadFail>;

pub type SpiFlushResponse = Result<(), SpiFlushFail>;
pub type GpioGetResponse = Result<GpioState, GpioGetFail>;
pub type GpioPutResponse = Result<(), GpioPutFail>;
pub type GpioWaitResponse = Result<(), GpioWaitFail>;
pub type SetConfigurationResponse = Result<(), SetConfigurationFail>;

endpoints! {
    list = ENDPOINT_LIST;
    | EndpointTy         | RequestTy               | ResponseTy               | Path                |
    | ----------         | ---------               | ----------               | ----                |
    | PingEndpoint       | u32                     | u32                      | "ping"              |
    | I2cRead            | I2cReadRequest          | I2cReadResponse<'a>      | "i2c/read"          |
    | I2cWrite           | I2cWriteRequest<'a>     | I2cWriteResponse         | "i2c/write"         |
    | SpiRead            | SpiReadRequest          | SpiReadResponse<'a>      | "spi/read"          |
    | SpiWrite           | SpiWriteRequest<'a>     | SpiWriteResponse         | "spi/write"         |
    | SpiFlush           | ()                      | SpiFlushResponse         | "spi/flush"         |
    | GpioGet            | GpioGetRequest          | GpioGetResponse          | "gpio/get"          |
    | GpioPut            | GpioPutRequest          | GpioPutResponse          | "gpio/put"          |
    | GpioWaitForHigh    | GpioWaitRequest         | GpioWaitResponse         | "gpio/wait-high"    |
    | GpioWaitForLow     | GpioWaitRequest         | GpioWaitResponse         | "gpio/wait-low"     |
    | GpioWaitForRising  | GpioWaitRequest         | GpioWaitResponse         | "gpio/wait-rising"  |
    | GpioWaitForFalling | GpioWaitRequest         | GpioWaitResponse         | "gpio/wait-falling" |
    | GpioWaitForAny     | GpioWaitRequest         | GpioWaitResponse         | "gpio/wait-any"     |
    | SetConfiguration   | SetConfigurationRequest | SetConfigurationResponse | "set-config"        |
    | Version            | ()                      | VersionInfo              | "version"           |
}

topics! {
    list = TOPICS_IN_LIST;
    direction = TopicDirection::ToServer;
    | TopicTy | MessageTy | Path |
    | ------- | --------- | ---- |
}

topics! {
    list = TOPICS_OUT_LIST;
    direction = TopicDirection::ToClient;
    | TopicTy | MessageTy | Path | Cfg |
    | ------- | --------- | ---- | --- |
}

// --- I2C

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct I2cReadRequest {
    pub address: u8,
    pub count: u16,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct I2cReadFail;

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct I2cWriteRequest<'a> {
    pub address: u8,
    pub contents: &'a [u8],
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct I2cWriteFail;

// --- SPI

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SpiReadRequest {
    pub count: u16,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SpiReadFail;

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SpiWriteRequest<'a> {
    pub contents: &'a [u8],
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SpiWriteFail;

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SpiFlushFail;

// --- GPIO

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioGetRequest {
    pub pin: u8,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioGetFail;

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioPutRequest {
    pub pin: u8,
    pub state: GpioState,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioPutFail;

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub enum GpioState {
    Low,
    High,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioWaitRequest {
    pub pin: u8,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct GpioWaitFail;

// --- Set config

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SetConfigurationRequest {
    pub i2c_frequency: u32,
    pub spi_frequency: u32,
    pub spi_phase: SpiPhase,
    pub spi_polarity: SpiPolarity,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub enum SpiPhase {
    CaptureOnFirstTransition = 0,
    CaptureOnSecondTransition = 1,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub enum SpiPolarity {
    IdleLow = 0,
    IdleHigh = 1,
}

#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct SetConfigurationFail;

// --- Version
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
pub struct VersionInfo {
    pub major: u16,
    pub minor: u16,
    pub patch: u32,
}
