use futures::executor::block_on;
use pico_de_gallo_lib as lib;
use std::ffi::CStr;
use std::os::raw::c_char;

pub struct PicoDeGallo(lib::PicoDeGallo);

// ----------------------------- Status Codes -----------------------------

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
    /// Ping failed
    PingFailed = -6,
    /// Spi Read failed
    SpiReadFailed = -7,
    /// Spi Write failed
    SpiWriteFailed = -8,
    /// Spi Flush failed
    SpiFlushFailed = -9,
    /// Gpio get failed
    GpioGetFailed = -10,
    /// Gpio put failed
    GpioPutFailed = -11,
    /// Gpio wait failed
    GpioWaitFailed = -12,
    /// Set config failed
    SetConfigFailed = -13,
    /// Version failed
    VersionFailed = -14,
}

// ----------------------------- Library Lifetime -----------------------------

/// gallo_init - Initialize the library context.
///
/// Returns an opaque representation of the underlying PicoDeGallo
/// device.
#[unsafe(no_mangle)]
pub extern "C" fn gallo_init() -> *const PicoDeGallo {
    let gallo = Box::new(PicoDeGallo(lib::PicoDeGallo::new()));

    Box::into_raw(gallo) as *const PicoDeGallo
}

/// gallo_init_with_serial_number - Initialize the library context for
/// a device with the given serial number.
///
/// Returns an opaque representation of the underlying PicoDeGallo
/// device.
///
/// # Safety
///
/// `c_serial_number` must point to a valid c-string containing a
/// valid Pico de Gallo serial number with a NULL-terminator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_init_with_serial_number(
    c_serial_number: *const c_char,
) -> *const PicoDeGallo {
    if c_serial_number.is_null() {
        eprintln!("NULL serial number received");
        return std::ptr::null();
    }

    // Safety: Pointer is not null due to the check above. Caller must
    // make sure to pass a null-terminated string.
    let serial_number = unsafe { CStr::from_ptr(c_serial_number).to_str() };

    if serial_number.is_err() {
        eprintln!("Invalid UTF-8 string");
        return std::ptr::null();
    }

    let gallo = Box::new(PicoDeGallo(lib::PicoDeGallo::new_with_serial_number(
        serial_number.unwrap(),
    )));

    Box::into_raw(gallo) as *const PicoDeGallo
}

/// gallo_free - Releases and destroys the library context created by `gallo_init`.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_free(gallo: *const PicoDeGallo) {
    if !gallo.is_null() {
        // Safety: caller must ensure that `gallo` is a valid opaque
        // pointer to `PicoDeGallo` returned by `gallo_init()`.
        drop(unsafe { Box::from_raw(gallo as *mut PicoDeGallo) });
    }
}

// ----------------------------- Ping endpoint -----------------------------

/// gallo_ping - Ping the firmware and wait for a response
///
/// Returns the same `u32` passed as the first argument.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_ping(gallo: *mut PicoDeGallo, id: &mut u32) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    let result = block_on(gallo.0.ping(*id));
    match result {
        Ok(back) => {
            *id = back;
            Status::Ok
        }
        Err(_) => Status::PingFailed,
    }
}

// ----------------------------- I2c endpoints -----------------------------

/// gallo_i2c_read - Read `len` bytes from the device at `address` into `buf`.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()` and `buf` must be valid
/// for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_i2c_read(
    gallo: *mut PicoDeGallo,
    address: u8,
    buf: *mut u8,
    len: usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() {
        eprintln!("Unexpected NULL buffer");
        return Status::InvalidArgument;
    }

    if len > u16::MAX.into() {
        eprintln!("Buffer is too large");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    // Safety: caller must ensure buf is valid for len bytes.
    let buf = unsafe { std::slice::from_raw_parts_mut(buf, len) };

    let result = block_on(gallo.0.i2c_read(address, len as u16));

    match result {
        Ok(data) => {
            buf.copy_from_slice(&data);
            Status::Ok
        }
        Err(_) => Status::I2cReadFailed,
    }
}

/// gallo_i2c_write - Write `len` bytes from `buf` to the device at `address`.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()` and `buf` must be valid
/// for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_i2c_write(
    gallo: *mut PicoDeGallo,
    address: u8,
    buf: *const u8,
    len: usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() {
        eprintln!("Unexpected NULL buffer");
        return Status::InvalidArgument;
    }

    if len > u16::MAX.into() {
        eprintln!("Buffer is too large");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    // Safety: caller must ensure buf is valid for len bytes.
    let buf = unsafe { std::slice::from_raw_parts(buf, len) };

    let result = block_on(gallo.0.i2c_write(address, buf));

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::I2cWriteFailed,
    }
}

/// gallo_i2c_write_read - Perform a write followed by a read.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`, `txbuf` must be valid
/// for `txlen` bytes, and `rxbuf` must be valid for `rxlen` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_i2c_write_read(
    gallo: *mut PicoDeGallo,
    address: u8,
    txbuf: *const u8,
    txlen: usize,
    rxbuf: *mut u8,
    rxlen: usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if txbuf.is_null() || rxbuf.is_null() {
        eprintln!("Unexpected NULL buffer");
        return Status::InvalidArgument;
    }

    if txlen > u16::MAX.into() || rxlen > u16::MAX.into() {
        eprintln!("Buffer is too large");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    // Safety: caller must ensure txbuf is valid for txlen bytes.
    let txbuf = unsafe { std::slice::from_raw_parts(txbuf, txlen) };

    // Safety: caller must ensure rxbuf is valid for rxlen bytes.
    let rxbuf = unsafe { std::slice::from_raw_parts_mut(rxbuf, rxlen) };

    let result = block_on(gallo.0.i2c_write(address, txbuf));
    if result.is_err() {
        return Status::I2cWriteFailed;
    }

    let result = block_on(gallo.0.i2c_read(address, rxlen as u16));
    match result {
        Ok(data) => {
            rxbuf.copy_from_slice(&data);
            Status::Ok
        }
        Err(_) => Status::I2cReadFailed,
    }
}

// ----------------------------- Spi endpoints -----------------------------

/// gallo_spi_read - Read `len` bytes.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()` and `buf` must be valid
/// for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_spi_read(
    gallo: *mut PicoDeGallo,
    buf: *mut u8,
    len: usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() {
        eprintln!("Unexpected NULL buffer");
        return Status::InvalidArgument;
    }

    if len > u16::MAX.into() {
        eprintln!("Buffer is too large");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    // Safety: caller must ensure buf is valid for len bytes.
    let buf = unsafe { std::slice::from_raw_parts_mut(buf, len) };

    let result = block_on(gallo.0.spi_read(len as u16));

    match result {
        Ok(data) => {
            buf.copy_from_slice(&data);
            Status::Ok
        }
        Err(_) => Status::SpiReadFailed,
    }
}

/// gallo_spi_write - Write `len` bytes from `buf`.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()` and `buf` must be valid
/// for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_spi_write(
    gallo: *mut PicoDeGallo,
    buf: *const u8,
    len: usize,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    if buf.is_null() {
        eprintln!("Unexpected NULL buffer");
        return Status::InvalidArgument;
    }

    if len > u16::MAX.into() {
        eprintln!("Buffer is too large");
        return Status::InvalidArgument;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    // Safety: caller must ensure buf is valid for len bytes.
    let buf = unsafe { std::slice::from_raw_parts(buf, len) };

    let result = block_on(gallo.0.spi_write(buf));

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::SpiWriteFailed,
    }
}

/// gallo_spi_flush - Flush the SPI interface.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_spi_flush(gallo: *mut PicoDeGallo) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    let result = block_on(gallo.0.spi_flush());

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::SpiFlushFailed,
    }
}

// ----------------------------- Gpio endpoints -----------------------------

/// gallo_gpio_get - Get the state of a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_get(
    gallo: *mut PicoDeGallo,
    pin: u8,
    state: &mut bool,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    let result = block_on(gallo.0.gpio_get(pin));

    match result {
        Ok(s) => {
            *state = s == lib::GpioState::High;
            Status::Ok
        }
        Err(_) => Status::GpioGetFailed,
    }
}

/// gallo_gpio_put - Set the state of a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_put(gallo: *mut PicoDeGallo, pin: u8, state: bool) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    let s = if state {
        lib::GpioState::High
    } else {
        lib::GpioState::Low
    };
    let result = block_on(gallo.0.gpio_put(pin, s));

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::GpioPutFailed,
    }
}

/// gallo_gpio_wait_for_high - Waits for a high level on a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_high(gallo: *mut PicoDeGallo, pin: u8) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    let result = block_on(gallo.0.gpio_wait_for_high(pin));

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::GpioWaitFailed,
    }
}

/// gallo_gpio_wait_for_low - Waits for a low level on a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_low(gallo: *mut PicoDeGallo, pin: u8) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    let result = block_on(gallo.0.gpio_wait_for_low(pin));

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::GpioWaitFailed,
    }
}

/// gallo_gpio_wait_for_rising_edge - Waits for a rising edge on a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_rising_edge(
    gallo: *mut PicoDeGallo,
    pin: u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    let result = block_on(gallo.0.gpio_wait_for_rising_edge(pin));

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::GpioWaitFailed,
    }
}

/// gallo_gpio_wait_for_falling_edge - Waits for a falling edge on a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_falling_edge(
    gallo: *mut PicoDeGallo,
    pin: u8,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    let result = block_on(gallo.0.gpio_wait_for_falling_edge(pin));

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::GpioWaitFailed,
    }
}

/// gallo_gpio_wait_for_any_edge - Waits for a any edge on a given GPIO pin.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_gpio_wait_for_any_edge(gallo: *mut PicoDeGallo, pin: u8) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    let result = block_on(gallo.0.gpio_wait_for_any_edge(pin));

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::GpioWaitFailed,
    }
}

// ----------------------------- Set config endpoint -----------------------------

/// gallo_set_config - Sets the configuration parameters for the
/// underlying I2c and Spi buses.
///
/// `spi_phase`: false means "Capture on first transition" or CPHA=0,
/// true means "Capture on second transition" or CPHA=1.
///
/// `spi_polarity`: false means "Idle low" or CPOL=0, true means "Idle
/// high" or CPOL=1.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gallo_set_config(
    gallo: *mut PicoDeGallo,
    i2c_frequency: u32,
    spi_frequency: u32,
    spi_phase: bool,
    spi_polarity: bool,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    let phase = if spi_phase {
        lib::SpiPhase::CaptureOnSecondTransition
    } else {
        lib::SpiPhase::CaptureOnFirstTransition
    };

    let polarity = if spi_polarity {
        lib::SpiPolarity::IdleHigh
    } else {
        lib::SpiPolarity::IdleLow
    };

    let result = block_on(
        gallo
            .0
            .set_config(i2c_frequency, spi_frequency, phase, polarity),
    );

    match result {
        Ok(()) => Status::Ok,
        Err(_) => Status::SetConfigFailed,
    }
}

// ----------------------------- Version endpoint -----------------------------

#[unsafe(no_mangle)]
/// gallo_version - Gets the firmware version.
///
/// Returns `Status::Ok` in case of success or various error codes.
///
/// # Safety
///
/// Caller must ensure that `gallo` is a valid, opaque pointer to
/// `PicoDeGallo` returned by `gallo_init()`.
pub unsafe extern "C" fn gallo_version(
    gallo: *mut PicoDeGallo,
    major: &mut u16,
    minor: &mut u16,
    patch: &mut u32,
) -> Status {
    if gallo.is_null() {
        eprintln!("Unexpected NULL context");
        return Status::Uninitialized;
    }

    // Safety: caller must ensure that `gallo` is a valid opaque
    // pointer to `PicoDeGallo` returned by `gallo_init()`.
    let gallo = unsafe { Box::from_raw(gallo) };

    let result = block_on(gallo.0.version());

    match result {
        Ok(lib::VersionInfo {
            major: a,
            minor: b,
            patch: c,
        }) => {
            *major = a;
            *minor = b;
            *patch = c;

            Status::Ok
        }
        Err(_) => Status::VersionFailed,
    }
}
