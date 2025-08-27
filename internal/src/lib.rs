#![no_std]

use serde::{Deserialize, Serialize};

/// Status values
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Status {
    Success,
    Fail,
}

/// Request representation.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Request<'a> {
    #[serde(borrow)]
    I2c(I2cRequest<'a>),
    #[serde(borrow)]
    Spi(SpiRequest<'a>),
    Gpio(GpioRequest),
}

/// Response representation.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Response<'a> {
    InvalidRequest,
    #[serde(borrow)]
    I2c(I2cResponse<'a>),
    #[serde(borrow)]
    Spi(SpiResponse<'a>),
    Gpio(GpioResponse),
}

/// I2c Request.
///
/// I2c requests consist of an opcode, the i2c device address, the
/// size of the transfer and an optional data block used for write
/// requests.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct I2cRequest<'a> {
    pub opcode: I2cOpcode,
    pub address: u16,
    pub size: u16,
    #[serde(borrow)]
    pub data: Option<&'a [u8]>,
}

/// I2c Response.
///
/// I2c responses consist of a [`Status`], an optional device address,
/// an optional size, and an optional data.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct I2cResponse<'a> {
    pub status: Status,
    pub address: Option<u16>,
    pub size: Option<u16>,
    #[serde(borrow)]
    pub data: Option<&'a [u8]>,
}

/// I2c opcodes.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum I2cOpcode {
    Read = 0,
    Write = 1,
}

/// Spi Request.
///
/// Spi requests consist of an opcode, an optional read size and an
/// optional data block used for write requests.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SpiRequest<'a> {
    pub opcode: SpiOpcode,
    pub size: Option<u16>,
    #[serde(borrow)]
    pub data: Option<&'a [u8]>,
}

/// Spi Response.
///
/// Spi responses consist of a [`Status`], an optional size, and an
/// optional data.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SpiResponse<'a> {
    pub status: Status,
    pub size: Option<u16>,
    #[serde(borrow)]
    pub data: Option<&'a [u8]>,
}

/// Spi opcodes.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SpiOpcode {
    Transfer = 0,
    Flush = 1,
}

/// Gpio Request.
///
/// Gpio requests consist of an opcode, the target [`Pin`] and an
/// optional [`State`] to set the pin to.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct GpioRequest {
    pub opcode: GpioOpcode,
    pub pin: Pin,
    pub state: Option<GpioState>,
}

/// Gpio Response.
///
/// Gpio responses consist of a [`Status`], a target [`Pin`] copied
/// from the [`GpioRequest`] and an optional [`GpioState`].
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct GpioResponse {
    pub status: Status,
    pub pin: Pin,
    pub state: Option<GpioState>,
}

/// Gpio opcodes.
#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum GpioOpcode {
    GetState = 0,
    SetState = 1,
}

/// Pin representation.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Pin {
    pub index: u8,
}

/// Gpio state.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum GpioState {
    Low,
    High,
}
