use nusb::DeviceInfo;
use pico_de_gallo_internal::{
    GpioGet, GpioGetFail, GpioGetRequest, GpioPut, GpioPutFail, GpioPutRequest, GpioWaitFail, GpioWaitForAny,
    GpioWaitForFalling, GpioWaitForHigh, GpioWaitForLow, GpioWaitForRising, GpioWaitRequest, I2cRead, I2cReadFail,
    I2cReadRequest, I2cWrite, I2cWriteFail, I2cWriteRequest, MICROSOFT_VID, PICO_DE_GALLO_PID, SetConfiguration,
    SetConfigurationFail, SetConfigurationRequest, SpiFlush, SpiFlushFail, SpiRead, SpiReadFail, SpiReadRequest,
    SpiWrite, SpiWriteFail, SpiWriteRequest, Version,
};

pub use pico_de_gallo_internal::{GpioState, SpiPhase, SpiPolarity, VersionInfo};

use postcard_rpc::{
    header::VarSeqKind,
    host_client::{HostClient, HostErr},
    standard_icd::{ERROR_PATH, PingEndpoint, WireError},
};
use std::convert::Infallible;

#[derive(Debug)]
pub enum PicoDeGalloError<E> {
    Comms(HostErr<WireError>),
    Endpoint(E),
}

impl<E> From<HostErr<WireError>> for PicoDeGalloError<E> {
    fn from(value: HostErr<WireError>) -> Self {
        Self::Comms(value)
    }
}

trait FlattenErr {
    type Good;
    type Bad;
    fn flatten(self) -> Result<Self::Good, PicoDeGalloError<Self::Bad>>;
}

impl<T, E> FlattenErr for Result<T, E> {
    type Good = T;
    type Bad = E;
    fn flatten(self) -> Result<Self::Good, PicoDeGalloError<Self::Bad>> {
        self.map_err(PicoDeGalloError::Endpoint)
    }
}

#[derive(Clone)]
pub struct PicoDeGallo {
    pub client: HostClient<WireError>,
}

impl Default for PicoDeGallo {
    fn default() -> Self {
        Self::new()
    }
}

impl PicoDeGallo {
    /// Create a new instance for the Pico de Gallo device.
    ///
    /// NOTICE:
    ///
    /// This constructor will return the first matching device in case
    /// there are more than one connected.
    ///
    /// If you want more control, please use `new_with_serial_number`
    /// instead.
    pub fn new() -> Self {
        Self::new_inner(|dev| dev.vendor_id() == MICROSOFT_VID && dev.product_id() == PICO_DE_GALLO_PID)
    }

    /// Create a new instance for the Pico de Gallo device with the
    /// given serial number.
    pub fn new_with_serial_number(serial_number: &str) -> Self {
        Self::new_inner(|dev| {
            dev.vendor_id() == MICROSOFT_VID
                && dev.product_id() == PICO_DE_GALLO_PID
                && dev.serial_number() == Some(serial_number)
        })
    }

    fn new_inner<F: FnMut(&DeviceInfo) -> bool>(func: F) -> Self {
        let client = HostClient::new_raw_nusb(func, ERROR_PATH, 8, VarSeqKind::Seq2);
        Self { client }
    }

    /// Wait until the client has closed the connection.
    pub async fn wait_closed(&self) {
        self.client.wait_closed().await;
    }

    /// Ping endpoint.
    ///
    /// Only used for testing purposes. Send a `u32` and get the same
    /// `u32` as a response.
    pub async fn ping(&self, id: u32) -> Result<u32, PicoDeGalloError<Infallible>> {
        Ok(self.client.send_resp::<PingEndpoint>(&id).await?)
    }

    /// Read `count` bytes from the I2C device at `address`.
    ///
    /// An arbitrary limit of `u16::MAX` is imposed currently, that
    /// may change in the future.
    pub async fn i2c_read(&self, address: u8, count: u16) -> Result<Vec<u8>, PicoDeGalloError<I2cReadFail>> {
        self.client
            .send_resp::<I2cRead>(&I2cReadRequest { address, count })
            .await?
            .flatten()
    }

    /// Write `contents` to the I2C device at `address`.
    pub async fn i2c_write(&self, address: u8, contents: &[u8]) -> Result<(), PicoDeGalloError<I2cWriteFail>> {
        self.client
            .send_resp::<I2cWrite>(&I2cWriteRequest { address, contents })
            .await?
            .flatten()
    }

    /// Read `count` bytes from the SPI bus.
    ///
    /// An arbitrary limit of `u16::MAX` is imposed currently, that
    /// may change in the future.
    pub async fn spi_read(&self, count: u16) -> Result<Vec<u8>, PicoDeGalloError<SpiReadFail>> {
        self.client
            .send_resp::<SpiRead>(&SpiReadRequest { count })
            .await?
            .flatten()
    }

    /// Write `contents` to the SPI bus.
    pub async fn spi_write(&self, contents: &[u8]) -> Result<(), PicoDeGalloError<SpiWriteFail>> {
        self.client
            .send_resp::<SpiWrite>(&SpiWriteRequest { contents })
            .await?
            .flatten()
    }

    /// Flush the SPI interface.
    pub async fn spi_flush(&self) -> Result<(), PicoDeGalloError<SpiFlushFail>> {
        self.client.send_resp::<SpiFlush>(&()).await?.flatten()
    }

    /// Get the current state of GPIO numbered by `pin`.
    ///
    /// Pico de Gallo offers 8 total GPIOs, numbered 0 through 7.
    pub async fn gpio_get(&self, pin: u8) -> Result<GpioState, PicoDeGalloError<GpioGetFail>> {
        self.client
            .send_resp::<GpioGet>(&GpioGetRequest { pin })
            .await?
            .flatten()
    }

    /// Set the GPIO numbered by `pin` to state `state`.
    ///
    /// Pico de Gallo offers 8 total GPIOs, numbered 0 through 7.
    pub async fn gpio_put(&self, pin: u8, state: GpioState) -> Result<(), PicoDeGalloError<GpioPutFail>> {
        self.client
            .send_resp::<GpioPut>(&GpioPutRequest { pin, state })
            .await?
            .flatten()
    }

    /// Wait for GPIO numbered by `pin` to reach `High` state.
    ///
    /// Pico de Gallo offers 8 total GPIOs, numbered 0 through 7.
    pub async fn gpio_wait_for_high(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioWaitFail>> {
        self.client
            .send_resp::<GpioWaitForHigh>(&GpioWaitRequest { pin })
            .await?
            .flatten()
    }

    /// Wait for GPIO numbered by `pin` to reach `Low` state.
    ///
    /// Pico de Gallo offers 8 total GPIOs, numbered 0 through 7.
    pub async fn gpio_wait_for_low(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioWaitFail>> {
        self.client
            .send_resp::<GpioWaitForLow>(&GpioWaitRequest { pin })
            .await?
            .flatten()
    }

    /// Wait for a rising edge on the GPIO numbered by `pin`.
    ///
    /// Pico de Gallo offers 8 total GPIOs, numbered 0 through 7.
    pub async fn gpio_wait_for_rising_edge(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioWaitFail>> {
        self.client
            .send_resp::<GpioWaitForRising>(&GpioWaitRequest { pin })
            .await?
            .flatten()
    }

    /// Wait for a falling edge on the GPIO numbered by `pin`.
    ///
    /// Pico de Gallo offers 8 total GPIOs, numbered 0 through 7.
    pub async fn gpio_wait_for_falling_edge(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioWaitFail>> {
        self.client
            .send_resp::<GpioWaitForFalling>(&GpioWaitRequest { pin })
            .await?
            .flatten()
    }

    /// Wait for either a rising edge or a falling edge on the GPIO
    /// numbered by `pin`.
    ///
    /// Pico de Gallo offers 8 total GPIOs, numbered 0 through 7.
    pub async fn gpio_wait_for_any_edge(&self, pin: u8) -> Result<(), PicoDeGalloError<GpioWaitFail>> {
        self.client
            .send_resp::<GpioWaitForAny>(&GpioWaitRequest { pin })
            .await?
            .flatten()
    }

    /// Set configuration parameters for I2C and SPI interfaces.
    pub async fn set_config(
        &self,
        i2c_frequency: u32,
        spi_frequency: u32,
        spi_phase: SpiPhase,
        spi_polarity: SpiPolarity,
    ) -> Result<(), PicoDeGalloError<SetConfigurationFail>> {
        self.client
            .send_resp::<SetConfiguration>(&SetConfigurationRequest {
                i2c_frequency,
                spi_frequency,
                spi_phase,
                spi_polarity,
            })
            .await?
            .flatten()
    }

    /// Get the firmware version from the Pico de Gallo device.
    pub async fn version(&self) -> Result<VersionInfo, PicoDeGalloError<Infallible>> {
        Ok(self.client.send_resp::<Version>(&()).await?)
    }
}
