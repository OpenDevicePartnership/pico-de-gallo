use pico_de_gallo_lib::{GpioState, PicoDeGallo};
use std::sync::Arc;
use tokio::runtime::{Handle, Runtime};
use tokio::sync::Mutex;
use tokio::task::block_in_place;

pub use pico_de_gallo_lib::{SpiPhase, SpiPolarity};

pub struct Hal {
    gallo: Arc<Mutex<PicoDeGallo>>,
    _runtime: Option<Runtime>,
    handle: Handle,
    in_async: bool,
}

impl Default for Hal {
    fn default() -> Self {
        Self::new()
    }
}

impl Hal {
    /// Instantiate the library context.
    pub fn new() -> Self {
        Self::new_inner(None)
    }

    /// Instantiate the library context for the device with the given
    /// `serial_number`.
    pub fn new_with_serial_number(serial_number: &str) -> Self {
        Self::new_inner(Some(serial_number))
    }

    fn new_inner(serial_number: Option<&str>) -> Self {
        let (runtime, handle, in_async) = match Handle::try_current() {
            Ok(handle) => (None, handle, true),
            Err(_) => {
                let runtime = Runtime::new().unwrap();
                let handle = runtime.handle().clone();
                (Some(runtime), handle, false)
            }
        };

        let gallo = if in_async {
            if let Some(serial_number) = serial_number {
                PicoDeGallo::new_with_serial_number(serial_number)
            } else {
                PicoDeGallo::new()
            }
        } else {
            handle.block_on(async {
                if let Some(serial_number) = serial_number {
                    PicoDeGallo::new_with_serial_number(serial_number)
                } else {
                    PicoDeGallo::new()
                }
            })
        };

        Self {
            gallo: Arc::new(Mutex::new(gallo)),
            _runtime: runtime,
            handle,
            in_async,
        }
    }

    /// Set interface configuration parameters
    pub fn set_config(
        &mut self,
        i2c_frequency: u32,
        spi_frequency: u32,
        spi_phase: SpiPhase,
        spi_polarity: SpiPolarity,
    ) {
        let handle = self.handle.clone();
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.set_config(i2c_frequency, spi_frequency, spi_phase, spi_polarity))
            .unwrap();
    }

    /// Gpio
    pub fn gpio(&self, pin: u8) -> Gpio {
        let gallo = Arc::clone(&self.gallo);
        let handle = self.handle.clone();
        Gpio {
            pin,
            gallo,
            handle,
            in_async: self.in_async,
        }
    }

    /// I2c
    pub fn i2c(&self) -> I2c {
        let gallo = Arc::clone(&self.gallo);
        let handle = self.handle.clone();
        I2c {
            gallo,
            handle,
            in_async: self.in_async,
        }
    }

    /// Spi
    pub fn spi(&self) -> Spi {
        let gallo = Arc::clone(&self.gallo);
        let handle = self.handle.clone();
        Spi {
            gallo,
            handle,
            in_async: self.in_async,
        }
    }

    /// Delay
    pub fn delay(&self) -> Delay {
        Delay
    }
}

// ----------------------------- Error -----------------------------

/// Pico de gallo errors
#[derive(Debug)]
pub enum Error {
    /// Other errors
    Other,
}

// ----------------------------- Gpio -----------------------------

pub struct Gpio {
    pin: u8,
    gallo: Arc<Mutex<PicoDeGallo>>,
    handle: Handle,
    in_async: bool,
}

impl Gpio {
    fn set_low_inner(&mut self) -> std::result::Result<(), Error> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.gpio_put(self.pin, GpioState::Low))
            .map_err(|_| Error::Other)
    }

    fn set_high_inner(&mut self) -> std::result::Result<(), Error> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.gpio_put(self.pin, GpioState::High))
            .map_err(|_| Error::Other)
    }

    fn is_low_inner(&mut self) -> std::result::Result<bool, Error> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.gpio_get(self.pin))
            .map_err(|_| Error::Other)
            .map(|s| s == GpioState::Low)
    }

    fn is_high_inner(&mut self) -> std::result::Result<bool, Error> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.gpio_get(self.pin))
            .map_err(|_| Error::Other)
            .map(|s| s == GpioState::High)
    }
}

impl embedded_hal::digital::Error for Error {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}

impl embedded_hal::digital::ErrorType for Gpio {
    type Error = Error;
}

impl embedded_hal::digital::OutputPin for Gpio {
    fn set_low(&mut self) -> std::result::Result<(), Self::Error> {
        if self.in_async {
            block_in_place(|| self.set_low_inner())
        } else {
            self.set_low_inner()
        }
    }

    fn set_high(&mut self) -> std::result::Result<(), Self::Error> {
        if self.in_async {
            block_in_place(|| self.set_high_inner())
        } else {
            self.set_high_inner()
        }
    }
}

impl embedded_hal::digital::InputPin for Gpio {
    fn is_low(&mut self) -> std::result::Result<bool, Self::Error> {
        if self.in_async {
            block_in_place(|| self.is_low_inner())
        } else {
            self.is_low_inner()
        }
    }

    fn is_high(&mut self) -> std::result::Result<bool, Self::Error> {
        if self.in_async {
            block_in_place(|| self.is_high_inner())
        } else {
            self.is_high_inner()
        }
    }
}

impl embedded_hal_async::digital::Wait for Gpio {
    async fn wait_for_high(&mut self) -> Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_high(self.pin)
            .await
            .map_err(|_| Self::Error::Other)
    }

    async fn wait_for_low(&mut self) -> Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_low(self.pin)
            .await
            .map_err(|_| Self::Error::Other)
    }

    async fn wait_for_rising_edge(&mut self) -> Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_rising_edge(self.pin)
            .await
            .map_err(|_| Self::Error::Other)
    }

    async fn wait_for_falling_edge(&mut self) -> Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_falling_edge(self.pin)
            .await
            .map_err(|_| Self::Error::Other)
    }

    async fn wait_for_any_edge(&mut self) -> Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo
            .gpio_wait_for_any_edge(self.pin)
            .await
            .map_err(|_| Self::Error::Other)
    }
}

// ----------------------------- I2c -----------------------------

pub struct I2c {
    gallo: Arc<Mutex<PicoDeGallo>>,
    handle: Handle,
    in_async: bool,
}

impl I2c {
    fn transaction_inner(
        &mut self,
        address: embedded_hal::i2c::SevenBitAddress,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> std::result::Result<(), Error> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());

        for op in operations {
            match op {
                embedded_hal::i2c::Operation::Read(read) => {
                    let contents = handle
                        .block_on(gallo.i2c_read(address, read.len() as u16))
                        .map_err(|_| Error::Other)?;
                    read.copy_from_slice(&contents);
                }
                embedded_hal::i2c::Operation::Write(write) => handle
                    .block_on(gallo.i2c_write(address, write))
                    .map_err(|_| Error::Other)?,
            }
        }

        Ok(())
    }
}

impl embedded_hal::i2c::Error for Error {
    fn kind(&self) -> embedded_hal::i2c::ErrorKind {
        embedded_hal::i2c::ErrorKind::Other
    }
}

impl embedded_hal::i2c::ErrorType for I2c {
    type Error = Error;
}

impl embedded_hal::i2c::I2c<embedded_hal::i2c::SevenBitAddress> for I2c {
    fn transaction(
        &mut self,
        address: embedded_hal::i2c::SevenBitAddress,
        operations: &mut [embedded_hal::i2c::Operation<'_>],
    ) -> std::result::Result<(), Self::Error> {
        if self.in_async {
            block_in_place(|| self.transaction_inner(address, operations))
        } else {
            self.transaction_inner(address, operations)
        }
    }
}

impl embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress> for I2c {
    async fn transaction(
        &mut self,
        address: embedded_hal_async::i2c::SevenBitAddress,
        operations: &mut [embedded_hal_async::i2c::Operation<'_>],
    ) -> std::result::Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;

        for op in operations {
            match op {
                embedded_hal_async::i2c::Operation::Read(read) => {
                    let contents = gallo
                        .i2c_read(address, read.len() as u16)
                        .await
                        .map_err(|_| Self::Error::Other)?;
                    read.copy_from_slice(&contents);
                }
                embedded_hal_async::i2c::Operation::Write(write) => gallo
                    .i2c_write(address, write)
                    .await
                    .map_err(|_| Self::Error::Other)?,
            }
        }

        Ok(())
    }
}

// ----------------------------- Spi -----------------------------

pub struct Spi {
    gallo: Arc<Mutex<PicoDeGallo>>,
    handle: Handle,
    in_async: bool,
}

impl Spi {
    fn read_inner(&mut self, words: &mut [u8]) -> std::result::Result<(), Error> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        let contents = handle
            .block_on(gallo.spi_read(words.len() as u16))
            .map_err(|_| Error::Other)?;
        words.copy_from_slice(&contents);
        Ok(())
    }

    fn write_inner(&mut self, words: &[u8]) -> std::result::Result<(), Error> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle
            .block_on(gallo.spi_write(words))
            .map_err(|_| Error::Other)
    }

    fn flush_inner(&mut self) -> std::result::Result<(), Error> {
        let handle = &self.handle;
        let gallo = handle.block_on(self.gallo.lock());
        handle.block_on(gallo.spi_flush()).map_err(|_| Error::Other)
    }
}

impl embedded_hal::spi::Error for Error {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}

impl embedded_hal::spi::ErrorType for Spi {
    type Error = Error;
}

impl embedded_hal::spi::SpiBus for Spi {
    fn read(&mut self, words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        if self.in_async {
            block_in_place(|| self.read_inner(words))
        } else {
            self.read_inner(words)
        }
    }

    fn write(&mut self, words: &[u8]) -> std::result::Result<(), Self::Error> {
        if self.in_async {
            block_in_place(|| self.write_inner(words))
        } else {
            self.write_inner(words)
        }
    }

    fn transfer(&mut self, read: &mut [u8], write: &[u8]) -> std::result::Result<(), Self::Error> {
        if self.in_async {
            block_in_place(|| {
                self.write_inner(write)?;
                self.read_inner(read)
            })
        } else {
            self.write_inner(write)?;
            self.read_inner(read)
        }
    }

    fn transfer_in_place(&mut self, words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        if self.in_async {
            block_in_place(|| {
                self.write_inner(words)?;
                self.read_inner(words)
            })
        } else {
            self.write_inner(words)?;
            self.read_inner(words)
        }
    }

    fn flush(&mut self) -> std::result::Result<(), Self::Error> {
        if self.in_async {
            block_in_place(|| self.flush_inner())
        } else {
            self.flush_inner()
        }
    }
}

impl embedded_hal_async::spi::SpiBus for Spi {
    async fn read(&mut self, words: &mut [u8]) -> std::result::Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        let contents = gallo
            .spi_read(words.len() as u16)
            .await
            .map_err(|_| Self::Error::Other)?;
        words.copy_from_slice(&contents);
        Ok(())
    }

    async fn write(&mut self, words: &[u8]) -> std::result::Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo.spi_write(words).await.map_err(|_| Self::Error::Other)
    }

    async fn transfer(
        &mut self,
        read: &mut [u8],
        write: &[u8],
    ) -> std::result::Result<(), Self::Error> {
        self.write(write).await?;
        self.read(read).await
    }

    async fn transfer_in_place(
        &mut self,
        words: &mut [u8],
    ) -> std::result::Result<(), Self::Error> {
        self.write(words).await?;
        self.read(words).await
    }

    async fn flush(&mut self) -> std::result::Result<(), Self::Error> {
        let gallo = self.gallo.lock().await;
        gallo.spi_flush().await.map_err(|_| Self::Error::Other)
    }
}

// ----------------------------- Delay -----------------------------

pub struct Delay;

impl embedded_hal::delay::DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        std::thread::sleep(std::time::Duration::from_nanos(ns.into()))
    }
}

impl embedded_hal_async::delay::DelayNs for Delay {
    async fn delay_ns(&mut self, ns: u32) {
        tokio::time::sleep(tokio::time::Duration::from_nanos(ns.into())).await
    }
}
