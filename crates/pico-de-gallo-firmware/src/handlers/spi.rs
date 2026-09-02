//! SPI endpoint handlers.

use defmt::{debug, warn};
use embassy_rp::peripherals::SPI0;
use embassy_rp::spi::{self, Phase, Polarity, Spi};
use embassy_time::Duration;
use pico_de_gallo_internal::{
    MAX_BATCH_OPS, MAX_TRANSFER_SIZE, SpiBatchError, SpiBatchOp, SpiBatchRequest, SpiBatchResponse,
    SpiConfigurationInfo, SpiError, SpiFlushResponse, SpiGetConfigurationResponse, SpiPhase, SpiPolarity,
    SpiReadRequest, SpiReadResponse, SpiSetConfigurationRequest, SpiSetConfigurationResponse, SpiTransferRequest,
    SpiTransferResponse, SpiWriteRequest, SpiWriteResponse,
};
use postcard_rpc::header::VarHeader;

use crate::context::{Context, NUM_GPIOS, PinMode};

/// Handler for `spi/read` — reads bytes from the SPI bus.
pub(crate) async fn spi_read_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: SpiReadRequest,
) -> SpiReadResponse<'a> {
    let count = usize::from(req.count);
    if count > MAX_TRANSFER_SIZE {
        warn!("spi read: requested count {} exceeds buffer", count);
        return Err(SpiError::BufferTooLong);
    }

    debug!("spi read: count={=usize}", count);
    let buf = &mut context.buf[..count];
    context.spi.read(buf).await.map_err(|_| SpiError::Other)?;
    Ok(&context.buf[..count])
}

/// Handler for `spi/write` — writes bytes to the SPI bus.
pub(crate) async fn spi_write_handler<'a>(
    context: &mut Context,
    _header: VarHeader,
    req: SpiWriteRequest<'a>,
) -> SpiWriteResponse {
    debug!("spi write: len={=usize}", req.contents.len());
    context.spi.write(req.contents).await.map_err(|_| SpiError::Other)
}

/// Handler for `spi/flush` — flushes the SPI interface.
pub(crate) async fn spi_flush_handler(context: &mut Context, _header: VarHeader, _req: ()) -> SpiFlushResponse {
    debug!("spi flush");
    context.spi.flush().map_err(|_| SpiError::Other)
}

/// Handler for `spi/transfer` — performs a full-duplex SPI transfer via DMA.
pub(crate) async fn spi_transfer_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: SpiTransferRequest<'a>,
) -> SpiTransferResponse<'a> {
    let len = req.contents.len();
    if len > MAX_TRANSFER_SIZE {
        warn!("spi transfer: requested len {} exceeds buffer", len);
        return Err(SpiError::BufferTooLong);
    }

    debug!("spi transfer: len={=usize}", len);
    let buf = &mut context.buf[..len];
    context
        .spi
        .transfer(buf, req.contents)
        .await
        .map_err(|_| SpiError::Other)?;
    Ok(&context.buf[..len])
}

/// Handler for `spi/batch` — executes multiple SPI operations atomically under CS.
///
/// The firmware validates the encoded operations first, then its chip-select
/// pin. Refusals raised before chip-select is driven leave the pin untouched.
/// For an accepted transaction, CS is configured as an output, asserted low,
/// and deasserted high after execution even when an operation fails.
/// Read and Transfer data is accumulated in `context.buf`.
pub(crate) async fn spi_batch_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: SpiBatchRequest<'a>,
) -> SpiBatchResponse<'a> {
    let ops = req.ops;
    let count = req.count as usize;
    let cs_idx = usize::from(req.cs_pin);

    // Pre-validate op count
    if count > MAX_BATCH_OPS {
        return Err(SpiBatchError {
            failed_op: 0,
            kind: SpiError::BufferTooLong,
        });
    }

    // Pre-validate: walk the ops to compute total read length and the
    // accumulated delay, which sets this dispatch's supervisor budget.
    // MAX_BATCH_OPS (64) DelayNs ops of u32::MAX ns each is 274.9 s, well
    // above DEFAULT_DISPATCH_BUDGET.
    let mut total_read = 0usize;
    let mut total_delay_ns = 0u64;
    let mut remaining = ops;
    let mut validated = 0usize;
    while !remaining.is_empty() {
        let (op, rest) = postcard::take_from_bytes::<SpiBatchOp>(remaining).map_err(|_| SpiBatchError {
            failed_op: validated as u16,
            kind: SpiError::Other,
        })?;
        match op {
            SpiBatchOp::Read { len } => total_read += len as usize,
            SpiBatchOp::Transfer { data } => total_read += data.len(),
            SpiBatchOp::DelayNs { ns } => total_delay_ns = total_delay_ns.saturating_add(u64::from(ns)),
            _ => {}
        }
        remaining = rest;
        validated += 1;
    }
    if validated != count {
        return Err(SpiBatchError {
            failed_op: 0,
            kind: SpiError::Other,
        });
    }
    if total_read > MAX_TRANSFER_SIZE {
        return Err(SpiBatchError {
            failed_op: 0,
            kind: SpiError::BufferTooLong,
        });
    }

    // Declared before chip-select is touched, so the guard covers the whole
    // transaction including deassertion.
    let _budget = crate::progress::declare(
        crate::progress::DEFAULT_DISPATCH_BUDGET + embassy_time::Duration::from_millis(total_delay_ns / 1_000_000),
    );

    if cs_idx >= NUM_GPIOS {
        debug!(
            "spi batch refused: cs_pin={=u8} num_gpios={=usize} reason=invalid",
            req.cs_pin, NUM_GPIOS
        );
        return Err(SpiBatchError {
            failed_op: 0,
            kind: SpiError::InvalidCsPin,
        });
    }

    if context.gpios.get(cs_idx).is_none_or(Option::is_none) {
        debug!("spi batch refused: cs_pin={=u8} reason=monitored", req.cs_pin);
        return Err(SpiBatchError {
            failed_op: 0,
            kind: SpiError::CsPinMonitored,
        });
    }

    if context.pin_modes.get(cs_idx) == Some(&PinMode::ExplicitInput) {
        debug!("spi batch refused: cs_pin={=u8} reason=explicit-input", req.cs_pin);
        return Err(SpiBatchError {
            failed_op: 0,
            kind: SpiError::CsPinUnavailable,
        });
    }

    let cs = context
        .gpios
        .get_mut(cs_idx)
        .and_then(Option::as_mut)
        .ok_or(SpiBatchError {
            failed_op: 0,
            kind: SpiError::CsPinMonitored,
        })?;
    cs.set_as_output();
    cs.set_high();

    debug!(
        "spi batch: cs_pin={=u8} ops={=usize} total_read={=usize}",
        req.cs_pin, count, total_read
    );

    // Assert CS (active low)
    let cs = context
        .gpios
        .get_mut(cs_idx)
        .and_then(Option::as_mut)
        .ok_or(SpiBatchError {
            failed_op: 0,
            kind: SpiError::CsPinMonitored,
        })?;
    cs.set_low();

    let result = spi_batch_execute(&mut context.spi, &mut context.buf, ops).await;

    // Deassert CS (always, even on error)
    let cs = context
        .gpios
        .get_mut(cs_idx)
        .and_then(Option::as_mut)
        .ok_or(SpiBatchError {
            failed_op: 0,
            kind: SpiError::CsPinMonitored,
        })?;
    cs.set_high();

    result
}

/// Inner execution loop for SPI batch, separated so CS can be
/// reliably deasserted in the caller regardless of outcome.
async fn spi_batch_execute<'a>(
    spi: &mut Spi<'static, SPI0, spi::Async>,
    buf: &'a mut [u8; MAX_TRANSFER_SIZE],
    ops: &[u8],
) -> Result<&'a [u8], SpiBatchError> {
    let mut remaining = ops;
    let mut read_offset = 0usize;
    let mut op_index = 0u16;

    while !remaining.is_empty() {
        let (op, rest) = postcard::take_from_bytes::<SpiBatchOp>(remaining).unwrap();
        remaining = rest;

        match op {
            SpiBatchOp::Read { len } => {
                let len = len as usize;
                let slice = &mut buf[read_offset..read_offset + len];
                spi.read(slice).await.map_err(|_| SpiBatchError {
                    failed_op: op_index,
                    kind: SpiError::Other,
                })?;
                read_offset += len;
            }
            SpiBatchOp::Write { data } => {
                spi.write(data).await.map_err(|_| SpiBatchError {
                    failed_op: op_index,
                    kind: SpiError::Other,
                })?;
            }
            SpiBatchOp::Transfer { data } => {
                let len = data.len();
                let slice = &mut buf[read_offset..read_offset + len];
                spi.transfer(slice, data).await.map_err(|_| SpiBatchError {
                    failed_op: op_index,
                    kind: SpiError::Other,
                })?;
                read_offset += len;
            }
            SpiBatchOp::DelayNs { ns } => {
                embassy_time::Timer::after(Duration::from_nanos(ns as u64)).await;
            }
        }
        op_index += 1;
    }

    Ok(&buf[..read_offset])
}

/// Handler for `spi/set-config` — reconfigures SPI bus parameters.
///
/// Validates the requested frequency before applying. The RP2350 SPI peripheral
/// requires a non-zero frequency no greater than half the peripheral clock
/// (75 MHz at the default 150 MHz system clock).
pub(crate) async fn spi_set_config_handler(
    context: &mut Context,
    _header: VarHeader,
    req: SpiSetConfigurationRequest,
) -> SpiSetConfigurationResponse {
    // Guard: embassy-rp's calc_prescs panics on freq == 0 or impossibly high values
    if req.spi_frequency == 0 {
        warn!("spi_set_config: frequency must be non-zero");
        return Err(SpiError::Other);
    }

    let mut spi_config = spi::Config::default();
    spi_config.frequency = req.spi_frequency;
    spi_config.phase = match req.spi_phase {
        SpiPhase::CaptureOnFirstTransition => Phase::CaptureOnFirstTransition,
        SpiPhase::CaptureOnSecondTransition => Phase::CaptureOnSecondTransition,
    };
    spi_config.polarity = match req.spi_polarity {
        SpiPolarity::IdleLow => Polarity::IdleLow,
        SpiPolarity::IdleHigh => Polarity::IdleHigh,
    };

    debug!("spi_set_config: freq={=u32}", req.spi_frequency);
    context.spi.set_config(&spi_config);
    context.spi_frequency = req.spi_frequency;
    context.spi_phase = req.spi_phase;
    context.spi_polarity = req.spi_polarity;
    Ok(())
}

/// Handler for `spi/get-config` — returns the current SPI bus configuration.
pub(crate) fn spi_get_config_handler(
    context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> SpiGetConfigurationResponse {
    SpiConfigurationInfo {
        spi_frequency: context.spi_frequency,
        spi_phase: context.spi_phase,
        spi_polarity: context.spi_polarity,
    }
}
