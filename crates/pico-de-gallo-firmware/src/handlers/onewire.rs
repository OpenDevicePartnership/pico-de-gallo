//! 1-Wire endpoint handlers.

#[cfg(feature = "hw-rev2")]
use defmt::{debug, warn};
#[cfg(feature = "hw-rev2")]
use embassy_rp::pio_programs::onewire::PioOneWireSearch;
#[cfg(feature = "hw-rev2")]
use embassy_time::Duration;
#[cfg(feature = "hw-rev2")]
use pico_de_gallo_internal::MAX_TRANSFER_SIZE;
use pico_de_gallo_internal::{
    OneWireError, OneWireReadRequest, OneWireReadResponse, OneWireResetResponse, OneWireSearchResponse,
    OneWireWritePullupRequest, OneWireWritePullupResponse, OneWireWriteRequest, OneWireWriteResponse,
};
use postcard_rpc::header::VarHeader;

use crate::context::Context;

/// Handler for `onewire/reset` — performs a bus reset and returns presence detection.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn onewire_reset_handler(context: &mut Context, _header: VarHeader, _req: ()) -> OneWireResetResponse {
    debug!("onewire reset");
    let present = context.onewire.reset().await;
    Ok(present)
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn onewire_reset_handler(
    _context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> OneWireResetResponse {
    Err(OneWireError::Unsupported)
}

/// Handler for `onewire/read` — reads bytes from the 1-Wire bus.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn onewire_read_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: OneWireReadRequest,
) -> OneWireReadResponse<'a> {
    let len = usize::from(req.len);
    if len > MAX_TRANSFER_SIZE {
        warn!("onewire read: requested len {} exceeds buffer", len);
        return Err(OneWireError::BufferTooLong);
    }

    debug!("onewire read: len={=usize}", len);
    let buf = &mut context.buf[..len];
    context.onewire.read_bytes(buf).await;
    Ok(&context.buf[..len])
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn onewire_read_handler<'a>(
    _context: &'a mut Context,
    _header: VarHeader,
    _req: OneWireReadRequest,
) -> OneWireReadResponse<'a> {
    Err(OneWireError::Unsupported)
}

/// Handler for `onewire/write` — writes bytes to the 1-Wire bus.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn onewire_write_handler<'a>(
    context: &mut Context,
    _header: VarHeader,
    req: OneWireWriteRequest<'a>,
) -> OneWireWriteResponse {
    if req.data.len() > MAX_TRANSFER_SIZE {
        warn!("onewire write: data len {} exceeds buffer", req.data.len());
        return Err(OneWireError::BufferTooLong);
    }

    debug!("onewire write: len={=usize}", req.data.len());
    context.onewire.write_bytes(req.data).await;
    Ok(())
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn onewire_write_handler<'a>(
    _context: &mut Context,
    _header: VarHeader,
    _req: OneWireWriteRequest<'a>,
) -> OneWireWriteResponse {
    Err(OneWireError::Unsupported)
}

/// Handler for `onewire/write-pullup` — writes bytes then applies strong pullup.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn onewire_write_pullup_handler<'a>(
    context: &mut Context,
    _header: VarHeader,
    req: OneWireWritePullupRequest<'a>,
) -> OneWireWritePullupResponse {
    if req.data.len() > MAX_TRANSFER_SIZE {
        warn!("onewire write-pullup: data len {} exceeds buffer", req.data.len());
        return Err(OneWireError::BufferTooLong);
    }

    let duration = Duration::from_millis(u64::from(req.pullup_duration_ms));
    debug!(
        "onewire write-pullup: len={=usize} pullup_ms={=u16}",
        req.data.len(),
        req.pullup_duration_ms
    );
    // pullup_duration_ms is u16, so up to 65.5 s — well above the default
    // dispatch budget.
    let _budget = crate::progress::declare(duration + crate::progress::DEFAULT_DISPATCH_BUDGET);
    context.onewire.write_bytes_pullup(req.data, duration).await;
    Ok(())
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn onewire_write_pullup_handler<'a>(
    _context: &mut Context,
    _header: VarHeader,
    _req: OneWireWritePullupRequest<'a>,
) -> OneWireWritePullupResponse {
    Err(OneWireError::Unsupported)
}

/// Handler for `onewire/search` — starts a new ROM search from scratch.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn onewire_search_handler(
    context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> OneWireSearchResponse {
    debug!("onewire search: starting new search");
    context.onewire_search = PioOneWireSearch::new();
    let result = context.onewire_search.next(&mut context.onewire).await;
    Ok(result)
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn onewire_search_handler(
    _context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> OneWireSearchResponse {
    Err(OneWireError::Unsupported)
}

/// Handler for `onewire/search-next` — continues the current ROM search.
#[cfg(feature = "hw-rev2")]
pub(crate) async fn onewire_search_next_handler(
    context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> OneWireSearchResponse {
    debug!("onewire search-next");
    if context.onewire_search.is_finished() {
        return Ok(None);
    }
    let result = context.onewire_search.next(&mut context.onewire).await;
    Ok(result)
}

#[cfg(not(feature = "hw-rev2"))]
pub(crate) async fn onewire_search_next_handler(
    _context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> OneWireSearchResponse {
    Err(OneWireError::Unsupported)
}
