//! Byte-payload encoding and argument validation for MCP tools.
//!
//! Mirrors the `gallo` CLI conventions: bytes are accepted as hex strings
//! (`"0x48,0x00"`, `"4800"`) or comma-separated `0x`/`0b`/decimal tokens, and
//! returned as a [`Bytes`] struct carrying both a hex string and the decoded
//! array.

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

/// Validate a non-zero timeout in milliseconds.
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
}
