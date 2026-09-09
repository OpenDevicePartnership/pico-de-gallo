//! Byte-payload encoding and argument validation for MCP tools.
//!
//! Mirrors the `gallo` CLI conventions: bytes are accepted as hex strings
//! (`"0x48,0x00"`, `"4800"`) or comma-separated `0x`/`0b`/decimal tokens, and
//! returned as a [`Bytes`] struct carrying both a hex string and the decoded
//! array.

use pico_de_gallo_lib::{MAX_REQUEST_FRAME, MAX_RESPONSE_PAYLOAD, MAX_TRANSFER_SIZE};
use serde::Serialize;

/// A byte payload rendered both as a hex string and a decoded array.
///
/// Returned by every tool that reads bytes so an agent can reason about
/// register values in hex while still having the raw integers.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema, PartialEq, Eq)]
pub struct Bytes {
    /// Comma-separated `0xNN` tokens, e.g. `"0x48,0x00"`. Empty string for no bytes.
    pub hex: String,
    /// Decoded byte values.
    pub bytes: Vec<u8>,
}

impl Bytes {
    /// Build a [`Bytes`] from a raw slice.
    pub fn from_slice(data: &[u8]) -> Self {
        let hex = data
            .iter()
            .map(|b| format!("0x{b:02X}"))
            .collect::<Vec<_>>()
            .join(",");
        Self {
            hex,
            bytes: data.to_vec(),
        }
    }
}

/// Parse a single byte token: `0x`-hex, `0b`-binary, or decimal.
pub fn parse_byte(s: &str) -> Result<u8, String> {
    let s = s.trim();
    let parsed = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16)
    } else if let Some(bin) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        u8::from_str_radix(bin, 2)
    } else {
        s.parse::<u8>()
    };
    parsed.map_err(|e| format!("invalid byte '{s}': {e}"))
}

/// Parse a byte payload.
///
/// Accepts either a comma-separated list of `parse_byte` tokens
/// (`"0x0A,20,0xFF"`) or a bare even-length hex string (`"cc44"` / `"0xCC44"`).
/// An empty string yields an empty vector.
pub fn parse_bytes(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    if s.contains(',') {
        return s.split(',').map(|tok| parse_byte(tok.trim())).collect();
    }
    // Single token with a radix prefix -> one byte, if it is a valid single byte.
    if (s.starts_with("0x") || s.starts_with("0X") || s.starts_with("0b") || s.starts_with("0B"))
        && let Ok(b) = parse_byte(s)
    {
        return Ok(vec![b]);
    }
    // Bare hex string: strip an optional 0x/0X, require even length.
    let hex = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    if !hex.len().is_multiple_of(2) {
        return Err(format!("hex string '{s}' has an odd number of digits"));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| format!("invalid hex at {i}: {e}"))
        })
        .collect()
}

/// Validate a 7-bit I2C address (`0x00..=0x7F`).
pub fn validate_i2c_address(addr: u8) -> Result<u8, String> {
    if addr > 0x7F {
        return Err(format!(
            "I2C address 0x{addr:02X} exceeds 7-bit range (max 0x7F)"
        ));
    }
    Ok(addr)
}

/// Validate an ADC channel index (`0..=3`).
pub fn validate_adc_channel(ch: u8) -> Result<u8, String> {
    if ch > 3 {
        return Err(format!("ADC channel {ch} out of range (0..=3)"));
    }
    Ok(ch)
}

/// Validate the MCP GPIO tools' non-zero timeout in milliseconds.
///
/// The firmware clamps accepted values above its 30-minute ceiling and
/// returns the GPIO endpoint's `Timeout` error on expiry.
pub fn validate_timeout_ms(ms: u32) -> Result<u32, String> {
    if ms == 0 {
        return Err("timeout_ms must be non-zero".to_string());
    }
    Ok(ms)
}

/// Validate that an I2C write payload is non-empty.
///
/// The RP2040/RP2350 I2C block cannot emit an address-only transaction,
/// so a zero-length write can never succeed. Firmware refuses it, but
/// refusing here reports it as an invalid argument — which it is — rather
/// than as a device error, and costs no USB round-trip (issue #136).
///
/// Only the write paths use this. [`parse_bytes`] still maps `""` to an
/// empty vector, because an empty write phase is legal for
/// `i2c_write_read`, whose transfer does not terminate with a STOP.
pub fn validate_i2c_write_payload(data: &[u8]) -> Result<(), String> {
    if data.is_empty() {
        return Err(
            "zero-length I2C write is not supported by this hardware; probe an address \
             with i2c_read or i2c_scan instead"
                .to_string(),
        );
    }
    Ok(())
}

/// Validate that a requested read count fits in one response frame.
///
/// Two independent ceilings bound a transfer, chosen by the *direction* of
/// the bytes. Anything the device must send back is capped at
/// [`MAX_RESPONSE_PAYLOAD`] (1014) — a property of the host USB transport,
/// far below the 4096-byte firmware buffer. `pico-de-gallo-lib` enforces
/// this too, but its refusal reaches an agent through `map_pdg_err` as a
/// *device* error whose text names no size. Refusing here reports it as the
/// invalid argument it is, and names both the offending count and the limit
/// so the agent can re-chunk without guessing (issue #158).
pub fn validate_read_count(count: u16) -> Result<(), String> {
    validate_response_len(count as usize).map_err(|_| {
        format!(
            "read count {count} exceeds the {MAX_RESPONSE_PAYLOAD}-byte response limit; \
             split the read into smaller chunks"
        )
    })
}

/// Validate the total number of bytes a single call asks the device to
/// return.
///
/// The response ceiling bounds the whole reply frame, so it applies to an
/// *aggregate*: a batch of individually legal reads can still overflow it,
/// and a full-duplex `spi_transfer` is bounded by its payload length
/// because those bytes come back. This mirrors `pico-de-gallo-lib`'s
/// `check_i2c_batch_ops` / `check_spi_batch_ops`, which sum the returning
/// operations and report the overflow with `failed_op = 0` because an
/// aggregate is not attributable to any single operation.
///
/// Takes `usize` rather than `u16` because the sum of up to
/// [`pico_de_gallo_lib::MAX_BATCH_OPS`] reads overflows a `u16`, and
/// because a duplex payload arrives as a decoded byte vector.
pub fn validate_response_len(total: usize) -> Result<(), String> {
    if total > MAX_RESPONSE_PAYLOAD {
        return Err(format!(
            "{total} bytes to be returned exceeds the {MAX_RESPONSE_PAYLOAD}-byte \
             response limit; request fewer bytes back"
        ));
    }
    Ok(())
}

/// Validate that an outbound payload fits the firmware transfer buffer.
///
/// The *outbound* ceiling is [`MAX_TRANSFER_SIZE`] (4096) — looser than the
/// response ceiling, because bytes travelling to the device are not carried
/// in a single response frame. As with [`validate_read_count`], refusing
/// locally turns a size mistake into an actionable invalid-argument message
/// instead of an opaque "device error: buffer exceeds firmware limit"
/// (issue #158).
pub fn validate_write_payload(data: &[u8]) -> Result<(), String> {
    let len = data.len();
    if len > MAX_TRANSFER_SIZE {
        return Err(format!(
            "write payload {len} bytes exceeds the {MAX_TRANSFER_SIZE}-byte transfer \
             limit; split the write into smaller chunks"
        ));
    }
    Ok(())
}

/// Validate that a batch's whole request fits one frame.
///
/// The send-direction counterpart of [`validate_response_len`], and the
/// only ceiling that bounds a batch's *outgoing* bytes: no single batch
/// operation is capped at [`MAX_TRANSFER_SIZE`], so the aggregate is all
/// there is (issue #186). `frame_len` comes from
/// `pico_de_gallo_lib::i2c_batch_request_frame_len` or its SPI twin, which
/// account for the postcard-rpc header and every byte of encoding overhead.
///
/// `pico-de-gallo-lib` refuses this too, but its refusal reaches an agent
/// through `map_pdg_err` as a *device* error reading "buffer exceeds
/// firmware limit" — which is wrong twice over here: the device never
/// received the request, and the limit is the transport's, not the
/// firmware buffer's. Refusing locally names the actual overrun and the
/// remedy.
///
/// Reports the overshoot rather than a target payload size, because the
/// mapping from payload bytes to frame bytes depends on how many
/// operations carry them.
pub fn validate_request_frame_len(frame_len: usize) -> Result<(), String> {
    if frame_len > MAX_REQUEST_FRAME {
        return Err(format!(
            "the batch encodes to a {frame_len}-byte request, {} bytes over the \
             {MAX_REQUEST_FRAME}-byte limit for a single request; split it into \
             several batches",
            frame_len - MAX_REQUEST_FRAME
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_byte_accepts_hex_bin_decimal() {
        assert_eq!(parse_byte("0x48").unwrap(), 0x48);
        assert_eq!(parse_byte("0xab").unwrap(), 0xAB);
        assert_eq!(parse_byte("0b10101010").unwrap(), 0xAA);
        assert_eq!(parse_byte("20").unwrap(), 20);
    }

    #[test]
    fn parse_byte_rejects_garbage_and_overflow() {
        assert!(parse_byte("0xGG").is_err());
        assert!(parse_byte("0x100").is_err());
        assert!(parse_byte("").is_err());
    }

    #[test]
    fn parse_bytes_accepts_comma_list_and_bare_hex() {
        assert_eq!(parse_bytes("0x0A,20,0xFF").unwrap(), vec![0x0A, 20, 0xFF]);
        assert_eq!(parse_bytes("cc44").unwrap(), vec![0xCC, 0x44]);
        assert_eq!(parse_bytes("0xCC44").unwrap(), vec![0xCC, 0x44]);
        assert_eq!(parse_bytes("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn parse_bytes_rejects_odd_bare_hex() {
        assert!(parse_bytes("ccc").is_err());
    }

    #[test]
    fn bytes_from_slice_roundtrip() {
        let b = Bytes::from_slice(&[0x48, 0x00]);
        assert_eq!(b.hex, "0x48,0x00");
        assert_eq!(b.bytes, vec![0x48, 0x00]);
    }

    #[test]
    fn bytes_empty_formats_cleanly() {
        let b = Bytes::from_slice(&[]);
        assert_eq!(b.hex, "");
        assert!(b.bytes.is_empty());
    }

    #[test]
    fn validate_i2c_address_bounds() {
        assert_eq!(validate_i2c_address(0x48).unwrap(), 0x48);
        assert!(validate_i2c_address(0x80).is_err());
    }

    #[test]
    fn validate_adc_channel_bounds() {
        for c in 0..=3u8 {
            assert!(validate_adc_channel(c).is_ok());
        }
        assert!(validate_adc_channel(4).is_err());
    }

    #[test]
    fn validate_timeout_rejects_zero() {
        assert!(validate_timeout_ms(0).is_err());
        assert_eq!(validate_timeout_ms(1000).unwrap(), 1000);
    }

    // --- Zero-length I2C write refusal (issue #136) ---

    #[test]
    fn validate_i2c_write_payload_rejects_empty() {
        let err = validate_i2c_write_payload(&[]).expect_err("empty payload must be refused");
        assert!(
            err.contains("zero-length"),
            "message must name the problem, got {err:?}"
        );
    }

    #[test]
    fn validate_i2c_write_payload_accepts_one_byte() {
        assert!(validate_i2c_write_payload(&[0x00]).is_ok());
    }

    #[test]
    fn parse_bytes_still_yields_empty_for_empty_input() {
        // `parse_bytes` deliberately keeps returning an empty vec: an empty
        // payload is legal for `i2c_write_read`, whose write phase does not
        // terminate with a STOP. Only the write paths refuse it, and they
        // do so via `validate_i2c_write_payload` rather than by tightening
        // the shared parser.
        assert_eq!(parse_bytes("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn validate_read_count_accepts_the_limit() {
        assert!(validate_read_count(MAX_RESPONSE_PAYLOAD as u16).is_ok());
        assert!(validate_read_count(0).is_ok());
    }

    #[test]
    fn validate_read_count_refuses_one_past_the_limit() {
        let over = MAX_RESPONSE_PAYLOAD as u16 + 1;
        let e = validate_read_count(over).unwrap_err();
        assert!(e.contains(&over.to_string()), "{e}");
        assert!(e.contains(&MAX_RESPONSE_PAYLOAD.to_string()), "{e}");
    }

    #[test]
    fn validate_response_len_refuses_an_aggregate_over_the_limit() {
        assert!(validate_response_len(MAX_RESPONSE_PAYLOAD).is_ok());
        let over = MAX_RESPONSE_PAYLOAD + 1;
        let e = validate_response_len(over).unwrap_err();
        assert!(e.contains(&over.to_string()), "{e}");
    }

    #[test]
    fn validate_write_payload_accepts_the_limit() {
        assert!(validate_write_payload(&vec![0u8; MAX_TRANSFER_SIZE]).is_ok());
        assert!(validate_write_payload(&[]).is_ok());
    }

    #[test]
    fn validate_write_payload_refuses_one_past_the_limit() {
        let over = MAX_TRANSFER_SIZE + 1;
        let e = validate_write_payload(&vec![0u8; over]).unwrap_err();
        assert!(e.contains(&over.to_string()), "{e}");
        assert!(e.contains(&MAX_TRANSFER_SIZE.to_string()), "{e}");
    }

    #[test]
    fn validate_request_frame_len_accepts_the_limit() {
        assert!(validate_request_frame_len(0).is_ok());
        assert!(validate_request_frame_len(MAX_REQUEST_FRAME).is_ok());
    }

    #[test]
    fn validate_request_frame_len_refuses_one_past_the_limit() {
        let over = MAX_REQUEST_FRAME + 1;
        let e = validate_request_frame_len(over).unwrap_err();
        // The whole reason this exists rather than letting the library's
        // `BufferTooLong` surface: the message has to name the size, the
        // limit, the overshoot and the remedy (issue #186).
        assert!(e.contains(&over.to_string()), "{e}");
        assert!(e.contains(&MAX_REQUEST_FRAME.to_string()), "{e}");
        assert!(e.contains(" 1 bytes over"), "{e}");
        assert!(e.contains("split"), "{e}");
    }

    #[test]
    fn the_two_aggregate_limits_are_distinguishable_in_their_messages() {
        // Both refusals reach the library as an indistinguishable
        // `BufferTooLong`; the point of validating here is that an agent
        // can tell which direction overflowed.
        let resp = validate_response_len(MAX_RESPONSE_PAYLOAD + 1).unwrap_err();
        let req = validate_request_frame_len(MAX_REQUEST_FRAME + 1).unwrap_err();
        assert!(resp.contains("returned"), "{resp}");
        assert!(req.contains("request"), "{req}");
        assert_ne!(resp, req);
    }
}
