//! Mapping from `pico-de-gallo-lib` errors to `rmcp::ErrorData`.
//!
//! Device-selection errors are the exception: they are mapped by
//! [`crate::select::map_select_err`], co-located with
//! [`crate::select::SelectError`] so that adding a variant breaks an
//! exhaustive match in the file that defines it.

use pico_de_gallo_lib::host_client::HostErr;
use pico_de_gallo_lib::{PicoDeGalloError, ValidateError, WireError};
use rmcp::ErrorData;

/// True when a comms error means "the device is not currently reachable".
///
/// Both call sites are post-open: `map_pdg_err` runs on a device `connect`
/// already returned, and `map_validate_err` runs inside `connect` on an
/// already-claimed interface. Selection has therefore proved a board was
/// attached, so the message must not deny it — see `vanished_board_msg`.
fn is_no_device(err: &HostErr<WireError>) -> bool {
    matches!(err, HostErr::Closed)
}

/// Map a [`PicoDeGalloError`] to an [`ErrorData`] for a tool result.
pub fn map_pdg_err<E: core::fmt::Display>(err: PicoDeGalloError<E>) -> ErrorData {
    match err {
        PicoDeGalloError::Comms(e) if is_no_device(&e) => ErrorData::internal_error(
            "the device stopped responding mid-call; check the USB connection \
             and retry"
                .to_string(),
            None,
        ),
        PicoDeGalloError::Comms(e) => {
            ErrorData::internal_error(format!("communication error: {e:?}"), None)
        }
        PicoDeGalloError::Endpoint(e) => {
            ErrorData::invalid_params(format!("device error: {e}"), None)
        }
    }
}

/// Map a [`ValidateError`] to an [`ErrorData`].
pub fn map_validate_err(err: ValidateError) -> ErrorData {
    match err {
        ValidateError::Comms(e) if is_no_device(&e) => ErrorData::internal_error(
            "the device stopped responding mid-call; check the USB connection \
             and retry"
                .to_string(),
            None,
        ),
        other => ErrorData::internal_error(format!("{other}"), None),
    }
}

/// Convert a validation-string error (from `encoding` validators) into `ErrorData`.
pub fn invalid_arg(msg: impl Into<String>) -> ErrorData {
    ErrorData::invalid_params(msg.into(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pico_de_gallo_lib::host_client::HostErr;
    use pico_de_gallo_lib::{PicoDeGalloError, ValidateError};

    #[test]
    fn comms_closed_does_not_deny_the_board() {
        // Only reachable after `connect` returned `Ok`, so selection already
        // proved a board was attached and the interface claim succeeded.
        let e: PicoDeGalloError<std::convert::Infallible> =
            PicoDeGalloError::Comms(HostErr::Closed);
        let data = map_pdg_err(e);
        let msg = data.message.to_string();
        let lower = msg.to_lowercase();
        assert!(!lower.contains("no device"), "message was: {msg}");
        assert!(!lower.contains("not attached"), "message was: {msg}");
        assert!(lower.contains("retry"), "message was: {msg}");
    }

    #[test]
    fn endpoint_error_is_surfaced_via_display() {
        let e: PicoDeGalloError<String> = PicoDeGalloError::Endpoint("NoAcknowledge".into());
        let data = map_pdg_err(e);
        assert!(
            data.message.contains("NoAcknowledge"),
            "message was: {}",
            data.message
        );
    }

    #[test]
    fn validate_schema_mismatch_includes_versions() {
        let e = ValidateError::SchemaMismatch {
            expected_major: 0,
            actual_major: 0,
            expected_minor: 7,
            actual_minor: 6,
        };
        let data = map_validate_err(e);
        assert!(data.message.contains('7') && data.message.contains('6'));
    }

    #[test]
    fn validate_legacy_firmware_message() {
        let data = map_validate_err(ValidateError::LegacyFirmware);
        assert!(data.message.to_lowercase().contains("firmware"));
    }
}
