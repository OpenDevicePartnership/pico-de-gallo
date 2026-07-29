//! Device selection policy.
//!
//! Which board a tool call targets is decided here, by a pure function over
//! the attached serials, the server's `--serial-number` pin, and the call's
//! optional `serial_number` argument. Keeping this free of USB lets the whole
//! zero/one/many matrix — and every error string an agent will read — be
//! unit-tested with no hardware present.

use rmcp::ErrorData;

/// The optional per-call device selector.
///
/// Shared by every device tool that takes no other argument. Tools with their
/// own parameters declare an identical `serial_number` field instead.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct TargetParams {
    /// USB serial number of the board to use. Required when two or more
    /// boards are attached; optional when exactly one is.
    #[serde(default)]
    pub serial_number: Option<String>,
}

/// Why device selection failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectError {
    /// No Pico de Gallo is attached at all.
    NoDevice,
    /// Two or more boards are attached and the call named none of them.
    Ambiguous {
        /// Serials that can be addressed.
        available: Vec<String>,
        /// Attached boards reporting no USB serial, which cannot be named.
        unaddressable: usize,
    },
    /// The requested serial is not among the attached boards.
    NotFound {
        /// The serial the call asked for.
        requested: String,
        /// Serials that can be addressed.
        available: Vec<String>,
    },
    /// The server is pinned and the call asked for a different board.
    PinConflict {
        /// The server's `--serial-number`.
        pinned_serial: String,
        /// The serial the call asked for.
        requested: String,
    },
    /// The server is pinned to a board that is not attached.
    PinnedNotFound {
        /// The server's `--serial-number`.
        pinned_serial: String,
        /// Serials that can be addressed.
        available: Vec<String>,
    },
    /// Two or more attached boards report the same serial number.
    Duplicate {
        /// The serial more than one board answers to.
        serial: String,
        /// How many boards report it.
        count: usize,
    },
}

/// Render a serial list for an error message.
///
/// One formatter for every message, so the list format cannot drift between
/// error cases.
fn format_serials(available: &[String]) -> String {
    if available.is_empty() {
        "(none addressable)".to_string()
    } else {
        available.join(", ")
    }
}

/// How many attached boards answer to `serial`.
///
/// More than one is refused rather than resolved: opening it would bind to
/// whichever board enumerates first, which is the silent wrong-board guess
/// this module exists to prevent. Reachable in practice — the firmware falls
/// back to an all-zero serial when the OTP chip-ID read fails.
fn count_matching(available: &[String], serial: &str) -> usize {
    available.iter().filter(|s| s.as_str() == serial).count()
}

impl std::fmt::Display for SelectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDevice => {
                write!(
                    f,
                    "No Pico de Gallo device attached: connect one and retry."
                )
            }
            Self::Ambiguous {
                available,
                unaddressable: 0,
            } => {
                let serials = format_serials(available);
                write!(
                    f,
                    "Multiple Pico de Gallo devices attached; `serial_number` is required.\n\
                     Available: {serials}"
                )
            }
            Self::Ambiguous {
                available,
                unaddressable,
            } => {
                let total = available.len() + unaddressable;
                let serials = format_serials(available);
                write!(
                    f,
                    "{total} Pico de Gallo devices attached; `serial_number` is required, \
                     but {unaddressable} of them report no USB serial number and cannot \
                     be addressed.\nAvailable: {serials}"
                )
            }
            Self::NotFound {
                requested,
                available,
            } => {
                let serials = format_serials(available);
                write!(
                    f,
                    "No Pico de Gallo with serial number '{requested}' is attached.\n\
                     Available: {serials}"
                )
            }
            Self::PinConflict {
                pinned_serial,
                requested,
            } => write!(
                f,
                "This server is pinned to serial number '{pinned_serial}' (--serial-number); \
                 it cannot address '{requested}'. Omit serial_number, or pass '{pinned_serial}'."
            ),
            Self::PinnedNotFound {
                pinned_serial,
                available,
            } => {
                let serials = format_serials(available);
                write!(
                    f,
                    "This server is pinned to serial number '{pinned_serial}' (--serial-number), \
                     which is not attached.\nAvailable: {serials}"
                )
            }
            Self::Duplicate { serial, count } => write!(
                f,
                "{count} attached Pico de Gallo devices report serial number \
                 '{serial}'; they cannot be told apart. Detach all but one and retry."
            ),
        }
    }
}

impl std::error::Error for SelectError {}

/// Map a [`SelectError`] to an MCP tool error.
///
/// Errors the agent can fix by changing its arguments are `invalid_params`;
/// errors about the environment are `internal_error`, because no argument
/// change helps.
///
/// [`SelectError::Ambiguous`] and [`SelectError::NotFound`] sit on both sides
/// of that line: they are argument-fixable only while something is
/// addressable. With an empty `available` list there is no serial the agent
/// could pass, so a human has to intervene and they are environmental too.
pub fn map_select_err(err: SelectError) -> ErrorData {
    let msg = err.to_string();
    match err {
        SelectError::NoDevice
        | SelectError::PinnedNotFound { .. }
        | SelectError::Duplicate { .. } => ErrorData::internal_error(msg, None),
        SelectError::Ambiguous { ref available, .. }
        | SelectError::NotFound { ref available, .. }
            if available.is_empty() =>
        {
            ErrorData::internal_error(msg, None)
        }
        SelectError::Ambiguous { .. }
        | SelectError::NotFound { .. }
        | SelectError::PinConflict { .. } => ErrorData::invalid_params(msg, None),
    }
}

/// Decide which board a tool call targets.
///
/// `attached` is the serial of every attached board in enumeration order;
/// a `None` entry is a board that reports no USB serial number.
///
/// Returns the serial to open. `Ok(None)` means "open the sole attached
/// board, which reports no serial" — the only case where a target cannot be
/// named.
///
/// The pinned serial wins over an omitted argument; a pin/argument conflict is
/// reported before the pinned board's absence, because the conflict is the
/// agent's actionable mistake. Any `Some` returned is byte-identical to an
/// entry in `attached`, so the caller can open it by exact serial match.
pub fn resolve_target(
    attached: &[Option<String>],
    pinned_serial: Option<&str>,
    requested: Option<&str>,
) -> Result<Option<String>, SelectError> {
    if attached.is_empty() {
        return Err(SelectError::NoDevice);
    }
    // Boards that report no USB serial cannot be named, so they never appear
    // in an error's `available` list — but they still count toward ambiguity.
    let available: Vec<String> = attached.iter().flatten().cloned().collect();

    if let Some(pinned_serial) = pinned_serial {
        if let Some(req) = requested
            && req != pinned_serial
        {
            return Err(SelectError::PinConflict {
                pinned_serial: pinned_serial.to_string(),
                requested: req.to_string(),
            });
        }
        return match count_matching(&available, pinned_serial) {
            0 => Err(SelectError::PinnedNotFound {
                pinned_serial: pinned_serial.to_string(),
                available,
            }),
            1 => Ok(Some(pinned_serial.to_string())),
            count => Err(SelectError::Duplicate {
                serial: pinned_serial.to_string(),
                count,
            }),
        };
    }

    match requested {
        Some(req) => match count_matching(&available, req) {
            0 => Err(SelectError::NotFound {
                requested: req.to_string(),
                available,
            }),
            1 => Ok(Some(req.to_string())),
            count => Err(SelectError::Duplicate {
                serial: req.to_string(),
                count,
            }),
        },
        // Exactly one board is unambiguous even when it reports no serial,
        // which is the only way `Ok(None)` is produced.
        None if attached.len() == 1 => Ok(attached[0].clone()),
        None => Err(SelectError::Ambiguous {
            unaddressable: attached.len() - available.len(),
            available,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an `attached` slice from string serials.
    fn attached(serials: &[&str]) -> Vec<Option<String>> {
        serials.iter().map(|s| Some((*s).to_string())).collect()
    }

    const A: &str = "9A54ED7E3A1D9D98";
    const B: &str = "5256657D8A5D7F03";

    #[test]
    fn no_devices_is_no_device_regardless_of_arguments() {
        assert_eq!(resolve_target(&[], None, None), Err(SelectError::NoDevice));
        assert_eq!(
            resolve_target(&[], None, Some(A)),
            Err(SelectError::NoDevice)
        );
        assert_eq!(
            resolve_target(&[], Some(A), None),
            Err(SelectError::NoDevice)
        );
    }

    #[test]
    fn sole_device_resolves_without_an_argument() {
        assert_eq!(
            resolve_target(&attached(&[A]), None, None),
            Ok(Some(A.to_string()))
        );
    }

    #[test]
    fn sole_device_accepts_its_own_serial() {
        assert_eq!(
            resolve_target(&attached(&[A]), None, Some(A)),
            Ok(Some(A.to_string()))
        );
    }

    #[test]
    fn sole_device_rejects_a_different_serial() {
        assert_eq!(
            resolve_target(&attached(&[A]), None, Some(B)),
            Err(SelectError::NotFound {
                requested: B.to_string(),
                available: vec![A.to_string()],
            })
        );
    }

    #[test]
    fn two_devices_without_an_argument_are_ambiguous() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), None, None),
            Err(SelectError::Ambiguous {
                available: vec![A.to_string(), B.to_string()],
                unaddressable: 0,
            })
        );
    }

    #[test]
    fn two_devices_resolve_either_by_serial() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), None, Some(A)),
            Ok(Some(A.to_string()))
        );
        assert_eq!(
            resolve_target(&attached(&[A, B]), None, Some(B)),
            Ok(Some(B.to_string()))
        );
    }

    #[test]
    fn two_devices_reject_an_unknown_serial() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), None, Some("BOGUS")),
            Err(SelectError::NotFound {
                requested: "BOGUS".to_string(),
                available: vec![A.to_string(), B.to_string()],
            })
        );
    }

    #[test]
    fn pin_wins_without_an_argument_even_with_two_attached() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), Some(A), None),
            Ok(Some(A.to_string()))
        );
    }

    #[test]
    fn pin_accepts_a_matching_argument() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), Some(A), Some(A)),
            Ok(Some(A.to_string()))
        );
    }

    #[test]
    fn pin_rejects_a_conflicting_argument() {
        assert_eq!(
            resolve_target(&attached(&[A, B]), Some(A), Some(B)),
            Err(SelectError::PinConflict {
                pinned_serial: A.to_string(),
                requested: B.to_string(),
            })
        );
    }

    #[test]
    fn pin_conflict_is_reported_before_absence() {
        // B is attached, A (the pin) is not. The conflict is the agent's
        // actionable mistake, so it must win over PinnedNotFound.
        assert_eq!(
            resolve_target(&attached(&[B]), Some(A), Some(B)),
            Err(SelectError::PinConflict {
                pinned_serial: A.to_string(),
                requested: B.to_string(),
            })
        );
    }

    #[test]
    fn pinned_board_not_attached_is_reported() {
        assert_eq!(
            resolve_target(&attached(&[B]), Some(A), None),
            Err(SelectError::PinnedNotFound {
                pinned_serial: A.to_string(),
                available: vec![B.to_string()],
            })
        );
    }

    #[test]
    fn sole_serialless_board_resolves_to_none() {
        assert_eq!(resolve_target(&[None], None, None), Ok(None));
    }

    #[test]
    fn sole_serialless_board_cannot_be_named() {
        assert_eq!(
            resolve_target(&[None], None, Some(A)),
            Err(SelectError::NotFound {
                requested: A.to_string(),
                available: vec![],
            })
        );
    }

    #[test]
    fn two_serialless_boards_are_ambiguous_with_nothing_to_list() {
        assert_eq!(
            resolve_target(&[None, None], None, None),
            Err(SelectError::Ambiguous {
                available: vec![],
                unaddressable: 2,
            })
        );
    }

    #[test]
    fn mixed_boards_list_only_the_addressable_ones() {
        let mixed = vec![Some(A.to_string()), None, None];
        assert_eq!(
            resolve_target(&mixed, None, None),
            Err(SelectError::Ambiguous {
                available: vec![A.to_string()],
                unaddressable: 2,
            })
        );
    }

    #[test]
    fn ambiguous_text_names_every_serial_and_the_argument() {
        let msg = resolve_target(&attached(&[A, B]), None, None)
            .unwrap_err()
            .to_string();
        // Issue #89 §3 specifies this wording; the agent recovers from it.
        assert!(msg.contains("`serial_number` is required"), "{msg}");
        assert!(msg.contains(A), "{msg}");
        assert!(msg.contains(B), "{msg}");
    }

    #[test]
    fn ambiguous_text_explains_unaddressable_boards() {
        let mixed = vec![Some(A.to_string()), None, None];
        let msg = resolve_target(&mixed, None, None).unwrap_err().to_string();
        assert!(msg.contains("3 Pico de Gallo devices"), "{msg}");
        assert!(
            msg.contains("2 of them report no USB serial number"),
            "{msg}"
        );
        assert!(msg.contains(A), "{msg}");
    }

    #[test]
    fn not_found_text_names_the_request_and_the_alternatives() {
        let msg = resolve_target(&attached(&[A, B]), None, Some("BOGUS"))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("'BOGUS'"), "{msg}");
        assert!(msg.contains(A) && msg.contains(B), "{msg}");
    }

    #[test]
    fn pin_conflict_text_tells_the_agent_what_to_do_instead() {
        let msg = resolve_target(&attached(&[A, B]), Some(A), Some(B))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("--serial-number"), "{msg}");
        assert!(msg.contains("Omit serial_number"), "{msg}");
        assert!(msg.contains(A) && msg.contains(B), "{msg}");
    }

    #[test]
    fn pinned_not_found_text_names_the_pin() {
        let msg = resolve_target(&attached(&[B]), Some(A), None)
            .unwrap_err()
            .to_string();
        assert!(msg.contains(A), "{msg}");
        assert!(msg.contains("not attached"), "{msg}");
    }

    #[test]
    fn agent_fixable_errors_map_to_invalid_params() {
        for err in [
            SelectError::Ambiguous {
                available: vec![A.to_string()],
                unaddressable: 0,
            },
            SelectError::NotFound {
                requested: B.to_string(),
                available: vec![A.to_string()],
            },
            SelectError::PinConflict {
                pinned_serial: A.to_string(),
                requested: B.to_string(),
            },
        ] {
            assert_eq!(
                map_select_err(err.clone()).code,
                rmcp::model::ErrorCode::INVALID_PARAMS,
                "{err:?} should be invalid_params"
            );
        }
    }

    #[test]
    fn environment_errors_map_to_internal_error() {
        for err in [
            SelectError::NoDevice,
            SelectError::PinnedNotFound {
                pinned_serial: A.to_string(),
                available: vec![],
            },
        ] {
            assert_eq!(
                map_select_err(err.clone()).code,
                rmcp::model::ErrorCode::INTERNAL_ERROR,
                "{err:?} should be internal_error"
            );
        }
    }

    #[test]
    fn unsatisfiable_not_found_maps_to_internal_error() {
        // Nothing is addressable, so no `serial_number` the agent could pass
        // would help; a human has to attach an identifiable board.
        assert_eq!(
            map_select_err(SelectError::NotFound {
                requested: A.to_string(),
                available: vec![],
            })
            .code,
            rmcp::model::ErrorCode::INTERNAL_ERROR
        );
    }

    #[test]
    fn empty_available_list_reads_sensibly() {
        let msg = resolve_target(&[None, None], None, None)
            .unwrap_err()
            .to_string();
        assert!(msg.contains("(none addressable)"), "{msg}");
    }

    #[test]
    fn no_device_text_says_nothing_is_attached() {
        let msg = resolve_target(&[], None, None).unwrap_err().to_string();
        assert!(msg.contains("No Pico de Gallo device attached"), "{msg}");
    }

    #[test]
    fn duplicate_serials_are_refused_rather_than_guessed() {
        // Opening this would bind to whichever board enumerates first, then
        // report a serial number that two boards answer to.
        assert_eq!(
            resolve_target(&attached(&[A, A]), None, Some(A)),
            Err(SelectError::Duplicate {
                serial: A.to_string(),
                count: 2,
            })
        );
    }

    #[test]
    fn duplicate_serials_are_refused_on_the_pinned_path_too() {
        assert_eq!(
            resolve_target(&attached(&[A, A]), Some(A), None),
            Err(SelectError::Duplicate {
                serial: A.to_string(),
                count: 2,
            })
        );
    }

    #[test]
    fn duplicate_text_names_the_serial_and_the_count() {
        let msg = resolve_target(&attached(&[A, A]), None, Some(A))
            .unwrap_err()
            .to_string();
        assert!(msg.contains("2 attached Pico de Gallo devices"), "{msg}");
        assert!(msg.contains(A), "{msg}");
        assert!(msg.contains("Detach all but one"), "{msg}");
    }

    #[test]
    fn target_params_default_to_no_serial() {
        let p: TargetParams = serde_json::from_str("{}").unwrap();
        assert_eq!(p.serial_number, None);
        let p: TargetParams =
            serde_json::from_str(r#"{"serial_number":"9A54ED7E3A1D9D98"}"#).unwrap();
        assert_eq!(p.serial_number.as_deref(), Some("9A54ED7E3A1D9D98"));
    }
}
