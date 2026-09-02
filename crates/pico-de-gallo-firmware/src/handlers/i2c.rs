//! I2C endpoint handlers.

use defmt::{debug, warn};
use embassy_embedded_hal::SetConfig;
use embassy_rp::i2c;
use embassy_time::{Duration, with_timeout};
use embedded_hal_async::i2c::{I2c as _, Operation};
use heapless::Vec as HeaplessVec;
use pico_de_gallo_internal::{
    I2cBatchError, I2cBatchOp, I2cBatchRequest, I2cBatchResponse, I2cError, I2cFrequency, I2cGetConfigurationResponse,
    I2cReadRequest, I2cReadResponse, I2cScanRequest, I2cScanResponse, I2cSetConfigurationRequest,
    I2cSetConfigurationResponse, I2cWriteReadRequest, I2cWriteReadResponse, I2cWriteRequest, I2cWriteResponse,
    MAX_BATCH_OPS, MAX_TRANSFER_SIZE,
};
use postcard_rpc::header::VarHeader;

use crate::context::{Context, map_i2c_error};

/// Handler for `i2c/read` — reads bytes from an I2C slave.
pub(crate) async fn i2c_read_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: I2cReadRequest,
) -> I2cReadResponse<'a> {
    let count = usize::from(req.count);
    if count > MAX_TRANSFER_SIZE {
        warn!("i2c read: requested count {} exceeds buffer", count);
        return Err(I2cError::BufferTooLong);
    }

    debug!("i2c read: addr={=u8:#x} count={=usize}", req.address, count);
    let buf = &mut context.buf[..count];
    context.i2c.read_async(req.address, buf).await.map_err(map_i2c_error)?;
    Ok(&context.buf[..count])
}

/// Handler for `i2c/write` — writes bytes to an I2C slave.
///
/// An empty payload is refused before `write_async` is reached. The
/// RP2040/RP2350 `DW_apb_i2c` block drives the address phase solely by
/// pushing bytes into `IC_DATA_CMD`, so an address-only `START + ADDR +
/// STOP` is physically unreachable (rp-rs/rp-hal#678,
/// embassy-rs/embassy#4474). embassy-rp 0.10.0 guards this in
/// `write_blocking_internal` but *not* in `write_async_internal`: with an
/// empty iterator it queues no command, starts no transaction, and then
/// still awaits a `STOP_DET`/`TX_ABRT` interrupt that can never fire. That
/// await never completes, and because postcard-rpc dispatches handlers
/// serially it wedges every endpoint on the device until USB
/// re-enumeration. This guard is the primary defence: it returns a clean
/// `ZeroLengthWrite` in about a millisecond. The `watchdog_supervisor_task`
/// is only a backstop — without this guard it would notice the dispatch slot
/// blowing its budget and reset the device after roughly 10 s.
/// Issue #101.
pub(crate) async fn i2c_write_handler<'a>(
    context: &mut Context,
    _header: VarHeader,
    req: I2cWriteRequest<'a>,
) -> I2cWriteResponse {
    #[cfg(not(feature = "wedge-test"))]
    if req.contents.is_empty() {
        warn!("i2c write: empty payload refused (addr={=u8:#x})", req.address);
        return Err(I2cError::ZeroLengthWrite);
    }

    debug!("i2c write: addr={=u8:#x} len={=usize}", req.address, req.contents.len());
    context
        .i2c
        .write_async(req.address, req.contents.iter().copied())
        .await
        .map_err(map_i2c_error)
}

/// Handler for `i2c/write-read` — writes then reads in a single I2C transaction.
pub(crate) async fn i2c_write_read_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: I2cWriteReadRequest<'a>,
) -> I2cWriteReadResponse<'a> {
    let count = usize::from(req.count);
    if count > MAX_TRANSFER_SIZE {
        warn!("i2c write_read: requested count {} exceeds buffer", count);
        return Err(I2cError::BufferTooLong);
    }

    debug!(
        "i2c write_read: addr={=u8:#x} write_len={=usize} read_count={=usize}",
        req.address,
        req.contents.len(),
        count
    );
    let buf = &mut context.buf[..count];
    context
        .i2c
        .write_read_async(req.address, req.contents.iter().copied(), buf)
        .await
        .map_err(map_i2c_error)?;
    Ok(&context.buf[..count])
}

/// First standard (non-reserved) 7-bit I2C address.
const I2C_ADDR_FIRST: u8 = 0x08;
/// Last standard (non-reserved) 7-bit I2C address.
const I2C_ADDR_LAST: u8 = 0x77;

/// Handler for `i2c/scan` — probes I2C addresses and returns those that ACK.
pub(crate) async fn i2c_scan_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: I2cScanRequest,
) -> I2cScanResponse<'a> {
    let (start, end) = if req.include_reserved {
        (0x00u8, 0x7Fu8)
    } else {
        (I2C_ADDR_FIRST, I2C_ADDR_LAST)
    };

    debug!("i2c scan: range={=u8:#x}..={=u8:#x}", start, end);

    let mut found = 0usize;

    for addr in start..=end {
        // Probe by attempting a 1-byte read. ACK means a device is present.
        // Bound each probe at 50ms so a single stuck address can't burn the
        // whole scan budget. The watchdog feeder task runs independently and
        // keeps the dog fed even if the scan takes several seconds total.
        let mut probe_buf = [0u8];
        match with_timeout(Duration::from_millis(50), context.i2c.read_async(addr, &mut probe_buf)).await {
            Ok(Ok(_)) => {
                if found >= MAX_TRANSFER_SIZE {
                    break;
                }
                context.buf[found] = addr;
                found += 1;
            }
            Ok(Err(_)) => {} // NACK or other I²C error — no device
            Err(_) => {
                warn!("i2c_scan: address {=u8:#x} timed out", addr);
            }
        }
    }

    debug!("i2c scan: found {=usize} device(s)", found);
    Ok(&context.buf[..found])
}

/// Handler for `i2c/batch` — executes multiple I2C operations as one transaction.
///
/// Decodes postcard-serialized ops and issues a single
/// `embedded_hal_async::i2c::I2c::transaction`, so the batch matches the
/// `embedded-hal` contract: adjacent same-type operations concatenate with no
/// intervening STOP, a direction change emits a repeated START, and only the
/// last operation carries a STOP.
///
/// Read data is accumulated in `context.buf` in operation order. Validation
/// failures name the offending operation; a bus failure applies to the
/// transaction as a whole and reports `failed_op = 0`.
pub(crate) async fn i2c_batch_handler<'a>(
    context: &'a mut Context,
    _header: VarHeader,
    req: I2cBatchRequest<'a>,
) -> I2cBatchResponse<'a> {
    let ops = req.ops;
    let count = req.count as usize;

    // Pre-validate op count
    if count > MAX_BATCH_OPS {
        return Err(I2cBatchError {
            failed_op: 0,
            kind: I2cError::BufferTooLong,
        });
    }

    // Pre-validate: walk the ops to compute total read length
    let mut total_read = 0usize;
    let mut remaining = ops;
    let mut validated = 0usize;
    while !remaining.is_empty() {
        let (op, rest) = postcard::take_from_bytes::<I2cBatchOp>(remaining).map_err(|_| I2cBatchError {
            failed_op: validated as u16,
            kind: I2cError::Other,
        })?;
        match op {
            I2cBatchOp::Read { len } => total_read += len as usize,
            I2cBatchOp::Write { data } => {
                // Refused during pre-validation, not in the execution loop
                // below, so a batch containing one is rejected atomically:
                // catching it mid-execution would already have driven the
                // earlier operations onto the bus. See the rationale on
                // `i2c_write_handler` — an empty payload wedges the
                // dispatcher device-wide. Issue #101.
                if data.is_empty() {
                    warn!("i2c batch: empty write payload at op {=usize}", validated);
                    return Err(I2cBatchError {
                        failed_op: validated as u16,
                        kind: I2cError::ZeroLengthWrite,
                    });
                }
            }
        }
        remaining = rest;
        validated += 1;
    }
    if validated != count {
        return Err(I2cBatchError {
            failed_op: 0,
            kind: I2cError::Other,
        });
    }
    if total_read > MAX_TRANSFER_SIZE {
        return Err(I2cBatchError {
            failed_op: 0,
            kind: I2cError::BufferTooLong,
        });
    }

    debug!(
        "i2c batch: addr={=u8:#x} ops={=usize} total_read={=usize}",
        req.address, count, total_read
    );

    // Split the context so the bus and the scratch buffer are borrowed disjointly.
    let Context { i2c, buf, .. } = context;

    // Materialise the operations, carving disjoint read slices out of buf, then
    // run the whole list as ONE transaction. Adjacent same-type operations
    // concatenate with no intervening STOP, a direction change emits a repeated
    // START, and only the final operation is followed by a STOP.
    let mut ops_vec: HeaplessVec<Operation<'_>, MAX_BATCH_OPS> = HeaplessVec::new();
    {
        let mut free = &mut buf[..total_read];
        let mut remaining = ops;
        while !remaining.is_empty() {
            // Validation above already proved every op decodes.
            let (op, rest) = postcard::take_from_bytes::<I2cBatchOp>(remaining).unwrap();
            remaining = rest;
            let pushed = match op {
                I2cBatchOp::Read { len } => {
                    let (head, tail) = free.split_at_mut(len as usize);
                    free = tail;
                    ops_vec.push(Operation::Read(head))
                }
                I2cBatchOp::Write { data } => ops_vec.push(Operation::Write(data)),
            };
            // `count <= MAX_BATCH_OPS` was checked above, so this cannot overflow.
            if pushed.is_err() {
                return Err(I2cBatchError {
                    failed_op: 0,
                    kind: I2cError::BufferTooLong,
                });
            }
        }

        // A bus failure belongs to the transaction as a whole and cannot be
        // attributed to one operation, so it reports `failed_op = 0`.
        // Validation failures above keep their exact index.
        i2c.transaction(req.address, &mut ops_vec)
            .await
            .map_err(|e| I2cBatchError {
                failed_op: 0,
                kind: map_i2c_error(e),
            })?;
    }
    drop(ops_vec);

    Ok(&buf[..total_read])
}

/// Handler for `i2c/set-config` — reconfigures I2C bus parameters.
pub(crate) async fn i2c_set_config_handler(
    context: &mut Context,
    _header: VarHeader,
    req: I2cSetConfigurationRequest,
) -> I2cSetConfigurationResponse {
    let frequency = match req.frequency {
        I2cFrequency::Standard => 100_000,
        I2cFrequency::Fast => 400_000,
        I2cFrequency::FastPlus => 1_000_000,
    };

    let mut i2c_config = i2c::Config::default();
    i2c_config.frequency = frequency;

    debug!("i2c_set_config: freq={=u32}", frequency);
    context
        .i2c
        .set_config(&i2c_config)
        .map(|_| {
            context.i2c_frequency = req.frequency;
        })
        .map_err(|_| I2cError::Other)
}

/// Handler for `i2c/get-config` — returns the current I2C bus configuration.
pub(crate) fn i2c_get_config_handler(
    context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> I2cGetConfigurationResponse {
    context.i2c_frequency
}
