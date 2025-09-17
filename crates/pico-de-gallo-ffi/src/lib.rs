use pico_de_gallo_lib as lib;

pub struct PicoDeGallo(lib::PicoDeGallo);

/// Spi phase.
#[repr(C)]
pub enum Phase {
    CaptureOnFirstTransition = 0,
    CaptureOnSecondTransition = 1,
}

impl From<lib::SpiPhase> for Phase {
    fn from(value: lib::SpiPhase) -> Self {
        match value {
            lib::SpiPhase::CaptureOnFirstTransition => Phase::CaptureOnFirstTransition,
            lib::SpiPhase::CaptureOnSecondTransition => Phase::CaptureOnSecondTransition,
        }
    }
}

impl Into<lib::SpiPhase> for Phase {
    fn into(self) -> lib::SpiPhase {
        match self {
            Phase::CaptureOnFirstTransition => lib::SpiPhase::CaptureOnFirstTransition,
            Phase::CaptureOnSecondTransition => lib::SpiPhase::CaptureOnSecondTransition,
        }
    }
}

/// Spi polarity.
#[repr(C)]
pub enum Polarity {
    IdleLow = 0,
    IdleHigh = 1,
}

impl From<lib::SpiPolarity> for Polarity {
    fn from(value: lib::SpiPolarity) -> Self {
        match value {
            lib::SpiPolarity::IdleLow => Polarity::IdleLow,
            lib::SpiPolarity::IdleHigh => Polarity::IdleHigh,
        }
    }
}

impl Into<lib::SpiPolarity> for Polarity {
    fn into(self) -> lib::SpiPolarity {
        match self {
            Polarity::IdleLow => lib::SpiPolarity::IdleLow,
            Polarity::IdleHigh => lib::SpiPolarity::IdleHigh,
        }
    }
}

// ----------------------------- PUBLIC API -----------------------------

#[repr(C)]
pub enum Status {
    /// Operation successful
    Ok = 0,
    /// I2c Read failed
    I2cReadFailed = -1,
    /// I2c Write failed
    I2cWriteFailed = -2,
    /// Firmware produced an invalid response
    InvalidResponse = -3,
    /// Library was not initialized
    Uninitialized = -4,
    /// Caller passed an invalid argument
    InvalidArgument = -5,
}

#[unsafe(no_mangle)]
/// gallo_init - Initialize the library context.
///
/// Returns an opaque representation of the underlying PicoDeGallo
/// device.
pub extern "C" fn gallo_init(
    i2c_frequency: u32,
    spi_frequency: u32,
    spi_phase: Phase,
    spi_polarity: Polarity,
) -> *const PicoDeGallo {
    let config = lib::Config {
        i2c_frequency,
        spi_frequency,
        spi_phase: spi_phase.into(),
        spi_polarity: spi_polarity.into(),
    };

    let gallo = Box::new(PicoDeGallo(lib::PicoDeGallo::new(config).unwrap()));

    Box::into_raw(gallo) as *const PicoDeGallo
}

#[unsafe(no_mangle)]
/// gallo_i2c_read - Read `len` bytes from the device at `address` into `buf`.
///
/// Returns `Status::Ok` in case of success or various error codes.
pub unsafe extern "C" fn gallo_i2c_read(
    gallo: *mut PicoDeGallo,
    address: u8,
    buf: *mut u8,
    len: usize,
) -> Status {
    if gallo.is_null() {
        return Status::Uninitialized;
    }

    if buf.is_null() {
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    // Safety: caller must ensure buf is valid for len bytes.
    let buf = unsafe { std::slice::from_raw_parts_mut(buf, len) };

    let mut io = gallo.0.usb.borrow_mut();
    let result = io.i2c_blocking_read(address, buf);

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::I2cReadFailed,
    }
}

#[unsafe(no_mangle)]
/// gallo_i2c_write - Write `len` bytes from `buf` to the device at `address`.
///
/// Returns `Status::Ok` in case of success or various error codes.
pub unsafe extern "C" fn gallo_i2c_write(
    gallo: *mut PicoDeGallo,
    address: u8,
    buf: *const u8,
    len: usize,
) -> Status {
    if gallo.is_null() {
        return Status::Uninitialized;
    }

    if buf.is_null() {
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    // Safety: caller must ensure buf is valid for len bytes.
    let buf = unsafe { std::slice::from_raw_parts(buf, len) };

    let mut io = gallo.0.usb.borrow_mut();
    let result = io.i2c_blocking_write(address, buf);

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::I2cWriteFailed,
    }
}

#[unsafe(no_mangle)]
/// gallo_i2c_write_read - Perform a write followed by a read.
///
/// Returns `Status::Ok` in case of success or various error codes.
pub unsafe extern "C" fn gallo_i2c_write_read(
    gallo: *mut PicoDeGallo,
    address: u8,
    txbuf: *const u8,
    txlen: usize,
    rxbuf: *mut u8,
    rxlen: usize,
) -> Status {
    if gallo.is_null() {
        return Status::Uninitialized;
    }

    if txbuf.is_null() || rxbuf.is_null() {
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    // Safety: caller must ensure txbuf is valid for txlen bytes.
    let txbuf = unsafe { std::slice::from_raw_parts(txbuf, txlen) };

    // Safety: caller must ensure rxbuf is valid for rxlen bytes.
    let rxbuf = unsafe { std::slice::from_raw_parts_mut(rxbuf, rxlen) };

    let mut io = gallo.0.usb.borrow_mut();

    let result = io.i2c_blocking_write(address, txbuf);
    if result.is_err() {
        return Status::I2cWriteFailed;
    }

    let result = io.i2c_blocking_read(address, rxbuf);
    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::I2cReadFailed,
    }
}

#[unsafe(no_mangle)]
/// gallo_free - Releases and destroys the library context created by `gallo_init`.
pub unsafe extern "C" fn gallo_free(gallo: *const PicoDeGallo) {
    if !gallo.is_null() {
        // Safety: caller must ensure that `gallo` is a valid opaque
        // pointer to `PicoDeGallo` returned by `gallo_init()`.
        drop(unsafe { Box::from_raw(gallo as *mut PicoDeGallo) });
    }
}
