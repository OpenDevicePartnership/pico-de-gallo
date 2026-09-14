//! Pins the skew-detection property the UART framing design relies on.
//!
//! `validate()` compares schema versions only, and this change rides an
//! already-unreleased 0.8 bump, so firmware built either side of it reports
//! the same version. The thing that actually distinguishes them is the
//! endpoint key: postcard-rpc derives `REQ_KEY`/`RESP_KEY` from the message
//! type's schema, so adding fields re-keys the endpoint and a mismatched
//! firmware does not dispatch the request at all.
//!
//! If any key-difference test here fails, the design's §4.5 mitigation is
//! wrong and the change needs a different one — do not delete the test.
//!
//! # The asymmetry between the two UART config endpoints
//!
//! `uart/set-config` is protected on the request side: its request type gains
//! fields, so `REQ_KEY` moves and an old firmware never dispatches.
//!
//! `uart/get-config` is **not**. Its request type is `()`, which does not
//! change, so `REQ_KEY` is untouched and an old firmware happily dispatches.
//! What moves is `RESP_KEY`, because `UartConfigurationInfo` is the response
//! payload. The old firmware answers under the *old* `RESP_KEY`, the new host
//! is waiting on the new one, the frame is dropped unmatched, and the caller
//! sees an opaque timeout rather than a refusal. That is quiet, not loud —
//! the same class of blind spot as the `DeviceInfo` hazard in AGENTS.md
//! §13.17's 2026-09-01 row. The tests below record it explicitly rather than
//! letting §4.5's "fails loudly" claim stand unqualified.

use pico_de_gallo_internal::{
    UartDataBits, UartGetConfiguration, UartParity, UartSetConfiguration,
    UartSetConfigurationRequest, UartStopBits,
};
use postcard_rpc::{Endpoint, Key, Key2};
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

/// The UART config shapes as they existed before this change.
///
/// Declared inside a module so each shim can carry the *same* type name as
/// the real type it stands in for. If `postcard-schema` ever started hashing
/// type names, a differently-named shim would make every comparison below
/// pass for the wrong reason; identical names remove that confound entirely.
/// `req_key_ignores_the_request_type_name` pins the assumption independently.
mod old {
    use super::*;

    /// The request shape as it existed before this change.
    #[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
    pub struct UartSetConfigurationRequest {
        pub baud_rate: u32,
    }

    /// The info shape as it existed before this change.
    #[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
    pub struct UartConfigurationInfo {
        pub baud_rate: u32,
    }
}

/// The old `uart/get-config` response payload, in its `Result` wrapper.
type OldUartGetConfigurationResponse =
    Result<old::UartConfigurationInfo, pico_de_gallo_internal::UartError>;

// --- uart/set-config: the request side, which IS protected -----------------

#[test]
fn req_key_changes_when_the_request_gains_fields() {
    let old = Key::for_path::<old::UartSetConfigurationRequest>("uart/set-config");
    let new = <UartSetConfiguration as Endpoint>::REQ_KEY;

    assert_ne!(
        old.to_bytes(),
        new.to_bytes(),
        "REQ_KEY did not change when the request type gained fields \
         (old={:02x?}, new={:02x?}). The design's §4.5 skew mitigation \
         assumes it does. Without it, an old firmware would accept a new \
         host's 6-byte request: postcard ignores trailing bytes, so the \
         old 1-field decoder reads baud_rate correctly, never looks at the \
         three framing bytes, and returns success. The requested baud rate \
         would take effect and the framing would not — a caller asking for \
         7M2 would be left transmitting 8N1, with no error anywhere. That \
         silent-drop direction is the dangerous one; the reverse (an old \
         host's 3-byte request reaching a new firmware) is three bytes \
         short and fails the decode loudly.",
        old.to_bytes(),
        new.to_bytes()
    );
}

#[test]
fn req_key_differs_in_the_truncated_two_byte_form() {
    let old = Key2::from_key8(Key::for_path::<old::UartSetConfigurationRequest>(
        "uart/set-config",
    ));
    let new = <UartSetConfiguration as Endpoint>::REQ_KEY2;

    assert_ne!(
        old.to_bytes(),
        new.to_bytes(),
        "REQ_KEY differs at full width but collides after truncation to two \
         bytes (both {:02x?}), which is what the dispatcher actually matches \
         on, so the full-width difference protects nothing.",
        old.to_bytes()
    );
}

// --- uart/get-config: the response side, which is the WEAKER path ----------

#[test]
fn get_config_resp_key_changes_when_the_info_gains_fields() {
    let old = Key::for_path::<OldUartGetConfigurationResponse>("uart/get-config");
    let new = <UartGetConfiguration as Endpoint>::RESP_KEY;

    assert_ne!(
        old.to_bytes(),
        new.to_bytes(),
        "RESP_KEY did not change when UartConfigurationInfo gained fields \
         (old={:02x?}, new={:02x?}). Without it the two skew directions \
         differ sharply. A NEW firmware's 6-byte reply reaching an OLD host \
         is the dangerous one: postcard ignores trailing bytes, so the old \
         1-field decoder returns the right baud rate, silently drops the \
         framing, and reports success — the caller is told the port is 8N1 \
         when it may not be. An OLD firmware's 3-byte reply reaching a NEW \
         host is three bytes short and fails the decode instead, which at \
         least surfaces as an error.",
        old.to_bytes(),
        new.to_bytes()
    );
}

#[test]
fn get_config_resp_key_differs_in_the_truncated_two_byte_form() {
    let old = Key2::from_key8(Key::for_path::<OldUartGetConfigurationResponse>(
        "uart/get-config",
    ));
    let new = <UartGetConfiguration as Endpoint>::RESP_KEY2;

    assert_ne!(
        old.to_bytes(),
        new.to_bytes(),
        "RESP_KEY differs at full width but collides after truncation to two \
         bytes (both {:02x?}), which is what the host actually matches \
         replies on.",
        old.to_bytes()
    );
}

/// Records the asymmetry as an executable fact rather than a comment.
///
/// `uart/get-config`'s request type is `()` and does not change, so its
/// `REQ_KEY` is untouched: an old firmware dispatches the request happily and
/// the skew surfaces only as a dropped reply and a timeout. This is the
/// documented limit of the §4.5 mitigation, not an oversight.
#[test]
fn get_config_req_key_is_unchanged_because_its_request_is_unit() {
    assert_eq!(
        <UartGetConfiguration as Endpoint>::REQ_KEY.to_bytes(),
        Key::for_path::<()>("uart/get-config").to_bytes(),
        "uart/get-config's request type is no longer `()`. If it now carries \
         a payload, this endpoint gained request-side skew detection and the \
         module docs above must be corrected."
    );
}

// --- Controls -------------------------------------------------------------

/// Control on the *type* input. Without this,
/// `req_key_changes_when_the_request_gains_fields` cannot distinguish "the
/// request type is hashed" from "the design's §4.5 mitigation is wrong".
#[test]
fn req_key_is_sensitive_to_the_request_type() {
    #[derive(Serialize, Deserialize, Schema)]
    struct OneField {
        baud_rate: u32,
    }
    #[derive(Serialize, Deserialize, Schema)]
    struct TwoFields {
        baud_rate: u32,
        extra: u32,
    }

    assert_ne!(
        Key::for_path::<OneField>("uart/set-config").to_bytes(),
        Key::for_path::<TwoFields>("uart/set-config").to_bytes(),
        "Key::for_path is not sensitive to the request type at all. The \
         endpoint key would then be a function of the path alone and no \
         wire-shape change is detectable through it."
    );
}

/// Control on the *path* input, for symmetry. Together with the above this
/// rules out a degenerate or constant hash, which comparing the same
/// arguments twice cannot.
#[test]
fn req_key_is_sensitive_to_the_endpoint_path() {
    assert_ne!(
        Key::for_path::<old::UartSetConfigurationRequest>("uart/set-config").to_bytes(),
        Key::for_path::<old::UartSetConfigurationRequest>("uart/get-config").to_bytes(),
        "Key::for_path ignores the path, so endpoints sharing a request type \
         would collide."
    );
}

/// The shim technique in this file is only valid because `postcard-schema`
/// does not hash a type's name into the key — only its data-model shape and
/// its field names. (postcard-schema 0.2.5, `src/key/hash.rs:193-200`:
/// `hash_named_type` carries the name-hashing line commented out, with a
/// NOTE explaining it allows `Vec<u8>`/`&[u8]` punning between std and
/// no_std peers.) If that ever changes, every "old shape" shim here would be
/// measuring a name difference rather than a shape difference, and
/// `req_key_changes_when_the_request_gains_fields` would pass for the wrong
/// reason. This pins the assumption. Fix the methodology; do not delete this.
#[derive(Serialize, Deserialize, Schema, Debug, PartialEq)]
struct ShapeTwinOfTheNewRequest {
    baud_rate: u32,
    data_bits: UartDataBits,
    parity: UartParity,
    stop_bits: UartStopBits,
}

#[test]
fn req_key_ignores_the_request_type_name() {
    let twin = Key::for_path::<ShapeTwinOfTheNewRequest>("uart/set-config");
    let real = <UartSetConfiguration as Endpoint>::REQ_KEY;
    assert_eq!(
        twin.to_bytes(),
        real.to_bytes(),
        "a field-for-field twin of UartSetConfigurationRequest keyed \
         differently from the real type. postcard-schema must have started \
         hashing type names, which invalidates the old-shape shims in this \
         file."
    );
}

// --- Premise checks -------------------------------------------------------

/// Guards the premise: the new request really does carry framing, in the
/// declared order, under the declared field names.
///
/// Asserting the *encoding* rather than reading one field is deliberate.
/// Field names and field order are both hashed into `REQ_KEY`, so a rename
/// or a reorder is as much a wire break as an added field — and unlike an
/// added field it changes no byte count. The three framing indices below are
/// pairwise distinct, so no permutation of the three framing fields produces
/// these bytes. (A triple like 7E2 would not do: `Seven` and `Even` are both
/// index 2, so a data_bits/parity swap would encode identically.)
#[test]
fn uart_set_configuration_request_carries_framing() {
    let req = UartSetConfigurationRequest {
        baud_rate: 115_200,
        data_bits: UartDataBits::Seven,
        parity: UartParity::Mark,
        stop_bits: UartStopBits::Two,
    };
    // 115200 = 7*128^2 + 4*128 + 0 = 0b111_0000100_0000000, so the LEB128
    // groups low-to-high are 0, 4, 7 -> 0x80, 0x84, 0x07. Then
    // Seven = 2, Mark = 3, Two = 1.
    assert_eq!(
        postcard::to_allocvec(&req).unwrap(),
        [0x80, 0x84, 0x07, 0x02, 0x03, 0x01]
    );
}

/// Postcard cannot detect this skew; only the endpoint key can.
///
/// Truncation is caught — the new shape cannot decode from the old
/// encoding. The other direction is silent: postcard ignores trailing
/// bytes, so an old peer decodes a new peer's payload successfully and
/// simply loses the framing. There is no decode error to notice, which is
/// exactly why the endpoint key has to carry the whole burden of skew
/// detection.
#[test]
fn uart_set_configuration_request_encodings_are_not_mutually_safe() {
    let new = UartSetConfigurationRequest {
        baud_rate: 115_200,
        data_bits: UartDataBits::Seven,
        parity: UartParity::Mark,
        stop_bits: UartStopBits::Two,
    };
    let new_bytes = postcard::to_allocvec(&new).unwrap();

    let old = old::UartSetConfigurationRequest { baud_rate: 115_200 };
    let old_bytes = postcard::to_allocvec(&old).unwrap();

    // New reader, old writer: refused, because the framing bytes are absent.
    assert!(
        postcard::from_bytes::<UartSetConfigurationRequest>(&old_bytes).is_err(),
        "the new request decoded from the old 3-byte encoding, which means \
         framing was synthesised from nothing"
    );

    // Old reader, new writer: SUCCEEDS, silently discarding the framing.
    let round_tripped: old::UartSetConfigurationRequest = postcard::from_bytes(&new_bytes).expect(
        "postcard is expected to ignore trailing bytes here; if it now \
             rejects them, the silent-skew hazard documented above is gone \
             and this test should be rewritten to record that",
    );
    assert_eq!(round_tripped.baud_rate, 115_200);
}
