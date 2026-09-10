//! UART endpoint handlers.

#[cfg(feature = "hw-rev2")]
use defmt::{debug, warn};
#[cfg(feature = "hw-rev2")]
use embassy_time::{Duration, with_timeout};
#[cfg(feature = "hw-rev2")]
use embedded_io_async::{Read as AsyncRead, Write as AsyncWrite};
#[cfg(feature = "hw-rev2")]
use pico_de_gallo_internal::{MAX_TRANSFER_SIZE, UartConfigurationInfo, UartDataBits, UartParity, UartStopBits};
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

/// Applies word length, parity and stop bits to `UART0`'s `UARTLCR_H`.
///
/// embassy-rp 0.10 exposes no runtime API for these: `BufferedUart`'s entire
/// public surface is `new`, `new_with_rtscts`, `blocking_{read,write,flush}`,
/// `busy`, `send_break`, `set_baudrate`, `split` and `split_ref`, and
/// `uart::Config` is consumed only by the constructors. So this writes the
/// register directly through the public `embassy_rp::pac`.
///
/// The sequence below is a deliberate replication of embassy's own private
/// `Uart::lcr_modify` (embassy-rp-0.10.0/src/uart/mod.rs:998-1046). Keep them
/// in step: if an embassy bump changes that function, review this one.
///
/// The delay is not optional, and embassy's comment explains why: the PL011's
/// `BUSY` flag is OR'd with TX-FIFO-not-full, so with FIFOs enabled there is
/// no way to poll for end-of-character, and FIFOs cannot be disabled
/// mid-character without losing integrity. Fifteen baud periods comfortably
/// exceeds start + data + parity + stop.
///
/// Call this *after* `set_baudrate`, so the divisor the delay is computed
/// from is the one that will be in force.
#[cfg(feature = "hw-rev2")]
fn apply_framing(data_bits: UartDataBits, parity: UartParity, stop_bits: UartStopBits) {
    use embassy_rp::pac;

    // `UartDataBits`' discriminants ARE the PL011 WLEN encoding, so this is a
    // cast rather than a match. The host-side
    // `uart_data_bits_discriminants_are_the_wlen_encoding` pins the values but
    // cannot see this crate's reliance on them, and the firmware hosts no
    // tests, so restate the dependency here: a reorder of the wire enum must
    // fail the FIRMWARE BUILD rather than silently mis-frame a board.
    const {
        assert!(UartDataBits::Five as u8 == 0b00);
        assert!(UartDataBits::Six as u8 == 0b01);
        assert!(UartDataBits::Seven as u8 == 0b10);
        assert!(UartDataBits::Eight as u8 == 0b11);
    }
    let wlen = data_bits as u8;
    let stp2 = matches!(stop_bits, UartStopBits::Two);
    // No `_` arm, deliberately. Appending a variant to `UartParity` must fail
    // this build rather than silently fall into a default framing - the same
    // reasoning as the `-Werror=switch` rule on the Zephyr Status mapping
    // (AGENTS.md 15.1).
    let (pen, eps, sps) = match parity {
        UartParity::None => (false, false, false),
        UartParity::Odd => (true, false, false),
        UartParity::Even => (true, true, false),
        // Stick parity: SPS set, EPS selects the constant. EPS=0 sends 1.
        UartParity::Mark => (true, false, true),
        UartParity::Space => (true, true, true),
    };

    let r = pac::UART0;
    let cr = r.uartcr().read();

    if cr.uarten() {
        r.uartcr().modify(|w| {
            w.set_uarten(false);
            w.set_txe(false);
            w.set_rxe(false);
        });

        // Maximise precision: a 16.6 fixed-point ratio scaled to 32 bits.
        // 3662 is ~(15 * 244.14), where 244.14 is 16e6 / 2^16.
        let mut brdiv_ratio = 64 * r.uartibrd().read().0 + r.uartfbrd().read().0;
        brdiv_ratio <<= 10;
        let scaled_freq = embassy_rp::clocks::clk_peri_freq() / 3662;
        let wait_time_us = brdiv_ratio / scaled_freq;
        // Busy-wait rather than Timer::after, matching embassy. The UART is
        // disabled across this window and postcard-rpc dispatches serially,
        // so yielding would buy nothing and widen the window.
        embassy_time::block_for(Duration::from_micros(u64::from(wait_time_us)));
    }

    r.uartlcr_h().modify(|w| {
        w.set_wlen(wlen);
        w.set_stp2(stp2);
        w.set_pen(pen);
        w.set_eps(eps);
        w.set_sps(sps);
    });

    r.uartcr().write_value(cr);
}

/// Handler for `uart/set-config` - changes baud rate and framing.
///
/// Applies baud first and framing immediately afterwards; both complete
/// before this handler returns. They are *not* applied atomically:
/// `set_baudrate` restores an enabled `UARTCR` before returning
/// (embassy-rp-0.10.0/src/uart/mod.rs:1070-1075), so there is a brief window
/// carrying the new divisor and the old framing.
///
/// Runtime reconfiguration does not drain the buffered UART either. A byte
/// already queued, or arriving from the peer during the window, may cross the
/// configuration boundary. Callers must quiesce both directions first.
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
    apply_framing(req.data_bits, req.parity, req.stop_bits);

    context.uart_config = UartConfigurationInfo {
        baud_rate: req.baud_rate,
        data_bits: req.data_bits,
        parity: req.parity,
        stop_bits: req.stop_bits,
    };
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

/// Handler for `uart/get-config` - returns the current UART configuration.
#[cfg(feature = "hw-rev2")]
pub(crate) fn uart_get_config_handler(
    context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> UartGetConfigurationResponse {
    Ok(context.uart_config.clone())
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) fn uart_get_config_handler(
    _context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> UartGetConfigurationResponse {
    Err(UartError::Unsupported)
}
