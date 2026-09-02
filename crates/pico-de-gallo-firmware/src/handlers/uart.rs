//! UART endpoint handlers.

#[cfg(feature = "hw-rev2")]
use defmt::{debug, warn};
#[cfg(feature = "hw-rev2")]
use embassy_time::{Duration, with_timeout};
#[cfg(feature = "hw-rev2")]
use embedded_io_async::{Read as AsyncRead, Write as AsyncWrite};
#[cfg(feature = "hw-rev2")]
use pico_de_gallo_internal::{MAX_TRANSFER_SIZE, UartConfigurationInfo};
use pico_de_gallo_internal::{
    UartError, UartFlushResponse, UartGetConfigurationResponse, UartReadRequest, UartReadResponse,
    UartSetConfigurationRequest, UartSetConfigurationResponse, UartWriteRequest, UartWriteResponse,
};
use postcard_rpc::header::VarHeader;

use crate::context::Context;

/// Handler for `uart/read` — reads bytes from the UART receive buffer.
///
/// Reads up to `count` bytes. `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT); a value of
/// `0` still selects the non-blocking 1 ms poll. Returns whatever bytes are
/// available (1 to count), or an empty slice on timeout.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn uart_read_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: UartReadRequest,
) -> UartReadResponse<'a> {
    let count = (req.count as usize).min(MAX_TRANSFER_SIZE);
    if count == 0 {
        return Ok(&[]);
    }

    let buf = &mut context.buf[..count];

    if req.timeout_ms == 0 {
        // Non-blocking: try to read whatever is buffered. Well inside the
        // default dispatch budget, so no declaration is needed.
        match with_timeout(Duration::from_millis(1), AsyncRead::read(&mut context.uart, buf)).await {
            Ok(Ok(n)) => Ok(&context.buf[..n]),
            Ok(Err(_)) => Err(UartError::Other),
            Err(_) => Ok(&[]),
        }
    } else {
        match crate::progress::bounded(req.timeout_ms, AsyncRead::read(&mut context.uart, buf)).await {
            Ok(Ok(n)) => Ok(&context.buf[..n]),
            Ok(Err(_)) => Err(UartError::Other),
            Err(_) => Ok(&[]),
        }
    }
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn uart_read_handler<'a>(
    _context: &'a mut Context,
    _header: VarHeader,
    _req: UartReadRequest,
) -> UartReadResponse<'a> {
    Err(UartError::Unsupported)
}

/// Fixed supervisor budget for UART transmit paths.
///
/// The 1024-byte TX buffer takes about 27 s to drain at 300 baud, which
/// exceeds the default dispatch budget. Deriving this from the configured
/// baud rate is possible but couples the handler to UART configuration state
/// for no practical gain.
#[cfg(feature = "hw-rev2")]
const UART_TX_BUDGET: Duration = Duration::from_secs(60);

/// Handler for `uart/write` — writes bytes to the UART transmit buffer.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn uart_write_handler(
    context: &mut Context,
    _header: VarHeader,
    req: UartWriteRequest<'_>,
) -> UartWriteResponse {
    if req.contents.len() > MAX_TRANSFER_SIZE {
        return Err(UartError::BufferTooLong);
    }

    let _budget = crate::progress::declare(UART_TX_BUDGET);
    AsyncWrite::write_all(&mut context.uart, req.contents)
        .await
        .map_err(|_| UartError::Other)
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn uart_write_handler(
    _context: &mut Context,
    _header: VarHeader,
    _req: UartWriteRequest<'_>,
) -> UartWriteResponse {
    Err(UartError::Unsupported)
}

/// Handler for `uart/flush` — flushes the UART transmit buffer.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn uart_flush_handler(context: &mut Context, _header: VarHeader, _req: ()) -> UartFlushResponse {
    let _budget = crate::progress::declare(UART_TX_BUDGET);
    AsyncWrite::flush(&mut context.uart).await.map_err(|_| UartError::Other)
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn uart_flush_handler(_context: &mut Context, _header: VarHeader, _req: ()) -> UartFlushResponse {
    Err(UartError::Unsupported)
}

/// Handler for `uart/set-config` — changes the UART baud rate.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn uart_set_config_handler(
    context: &mut Context,
    _header: VarHeader,
    req: UartSetConfigurationRequest,
) -> UartSetConfigurationResponse {
    if req.baud_rate == 0 {
        warn!("uart_set_config: baud_rate must be non-zero");
        return Err(UartError::InvalidBaudRate);
    }

    debug!("uart_set_config: baud_rate={=u32}", req.baud_rate);
    context.uart.set_baudrate(req.baud_rate);
    context.uart_baud_rate = req.baud_rate;
    Ok(())
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn uart_set_config_handler(
    _context: &mut Context,
    _header: VarHeader,
    _req: UartSetConfigurationRequest,
) -> UartSetConfigurationResponse {
    Err(UartError::Unsupported)
}

/// Handler for `uart/get-config` — returns the current UART configuration.
#[cfg(feature = "hw-rev2")]
pub(crate) fn uart_get_config_handler(
    context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> UartGetConfigurationResponse {
    Ok(UartConfigurationInfo {
        baud_rate: context.uart_baud_rate,
    })
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) fn uart_get_config_handler(
    _context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> UartGetConfigurationResponse {
    Err(UartError::Unsupported)
}
