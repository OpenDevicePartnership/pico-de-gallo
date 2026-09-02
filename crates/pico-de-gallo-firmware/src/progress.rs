//! Dispatch progress supervision.
//!
//! The watchdog feeder used to be an independent task, so it proved
//! **executor liveness** rather than **dispatcher progress**: postcard-rpc
//! dispatches handlers serially on one `&mut Context`, so a handler that never
//! returns blocks every endpoint while the feeder keeps being scheduled and
//! keeps feeding. Three device-wide wedges survived it (AGENTS.md §13.17).
//!
//! This module publishes an exact idle/in-flight edge from wrappers around
//! postcard-rpc's [`WireRx`] and [`WireTx`], which the supervisor task in
//! `main.rs` polls. See
//! `docs/superpowers/specs/2026-08-31-dispatcher-progress-watchdog-design.md`.

// TEMPORARY: every item below gains a caller in Tasks 2-6; this allow is
// deleted once the supervisor and handlers are wired up.
#![allow(dead_code)]

use core::cell::Cell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Instant};

/// Ceiling for any caller-supplied handler timeout.
///
/// `timeout_ms == 0` no longer means "wait forever": both `0` and oversized
/// values clamp to this. The handler's own `with_timeout` still fires and
/// still returns its normal `Timeout` error, so callers get a clean error
/// rather than a reboot.
pub(crate) const MAX_HANDLER_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Budget for a dispatch that declares nothing.
///
/// Sized to cover `i2c/scan`'s worst case (128 addresses × 50 ms = 6.4 s,
/// `handlers/i2c.rs`) with margin. Every other undeclared handler is µs–ms.
pub(crate) const DEFAULT_DISPATCH_BUDGET: Duration = Duration::from_secs(10);

/// Added to **declared** budgets only, never to [`DEFAULT_DISPATCH_BUDGET`].
///
/// Absorbs the handler's own `with_timeout` firing plus reply serialisation,
/// so a handler that legitimately runs to its declared ceiling completes
/// normally instead of tripping the supervisor.
pub(crate) const DISPATCH_SLACK: Duration = Duration::from_secs(30);

/// Budget for an in-flight `WireTx` operation.
///
/// Covers TX-mutex starvation only. postcard-rpc already bounds the endpoint
/// writes themselves at `frames * timeout_ms_per_frame`, clamped to
/// `1..=60000` ms.
pub(crate) const TX_BUDGET: Duration = Duration::from_secs(60);

/// Supervisor poll period. Worst-case reset latency is budget + this.
pub(crate) const SUPERVISOR_POLL: Duration = Duration::from_millis(250);

/// Wake-gap above which the supervisor assumes a time discontinuity.
///
/// `pause_on_debug(true)` stops the watchdog counter while a debugger holds
/// the core halted, but the embassy time driver keeps counting. Without this,
/// resuming from a breakpoint would look identical to a wedge and reset the
/// board, making debugging worse rather than better.
pub(crate) const DISCONTINUITY: Duration = Duration::from_millis(500);

/// Which slot tripped.
///
/// `#[repr(u32)]` because the supervisor casts this into a watchdog scratch
/// register with `as u32`.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
#[repr(u32)]
pub(crate) enum SlotKind {
    /// The dispatcher: a frame was received and its handler never returned.
    Dispatch,
    /// The wire transmitter, including the `gpio_monitor_task` topic paths.
    Tx,
}

/// One supervised slot.
#[derive(Clone, Copy)]
pub(crate) struct Slot {
    /// `None` means idle — blocking here is expected, not a fault.
    pub(crate) deadline: Option<Instant>,
    /// First four header bytes of the frame in flight, as a breadcrumb.
    pub(crate) key: u32,
}

impl Slot {
    const IDLE: Self = Self {
        deadline: None,
        key: 0,
    };
}

/// Full supervised state. `Copy` so it can live in a `Cell`.
#[derive(Clone, Copy)]
pub(crate) struct State {
    pub(crate) dispatch: Slot,
    pub(crate) tx: Slot,
    /// Concurrent `WireTx` senders. The TX slot arms on 0→1 and disarms on
    /// 1→0, because `AppTx` is cloned into four `gpio_monitor_task`s.
    pub(crate) tx_inflight: u16,
}

impl State {
    const INIT: Self = Self {
        dispatch: Slot::IDLE,
        tx: Slot::IDLE,
        tx_inflight: 0,
    };
}

/// What the supervisor should do this tick.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub(crate) enum Action {
    /// Progress is plausible. Feed the watchdog.
    Feed,
    /// The supervisor itself was not scheduled for an implausibly long time.
    /// Re-arm the live slots and feed; do not treat this as a wedge.
    Discontinuity,
    /// A slot blew its deadline. Log, stash a breadcrumb, reset.
    Expired(SlotKind, u32),
}

/// The supervision policy, as a pure function.
///
/// No hardware access and no `Instant::now()`, so it is unit-testable once the
/// crate grows a test harness (spec §6.4). Keep it that way.
pub(crate) fn decide(now: Instant, last_wake: Option<Instant>, state: &State) -> Action {
    if let Some(last) = last_wake
        && now.saturating_duration_since(last) > DISCONTINUITY
    {
        return Action::Discontinuity;
    }
    if let Some(deadline) = state.dispatch.deadline
        && now > deadline
    {
        return Action::Expired(SlotKind::Dispatch, state.dispatch.key);
    }
    if let Some(deadline) = state.tx.deadline
        && now > deadline
    {
        return Action::Expired(SlotKind::Tx, state.tx.key);
    }
    Action::Feed
}

static STATE: Mutex<CriticalSectionRawMutex, Cell<State>> = Mutex::new(Cell::new(State::INIT));

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.lock(|cell| {
        let mut state = cell.get();
        let out = f(&mut state);
        cell.set(state);
        out
    })
}

/// Read the current state. Used by the supervisor.
pub(crate) fn snapshot() -> State {
    STATE.lock(Cell::get)
}

/// Mark the dispatcher idle. Blocking after this point is expected.
pub(crate) fn dispatch_disarm() {
    with_state(|state| state.dispatch = Slot::IDLE);
}

/// Arm the dispatch slot for a newly received frame.
pub(crate) fn dispatch_arm(budget: Duration, key: u32) {
    let deadline = Instant::now() + budget;
    with_state(|state| {
        state.dispatch = Slot {
            deadline: Some(deadline),
            key,
        }
    });
}

/// Push the dispatch deadline out, returning the previous value so a
/// [`BudgetGuard`] can restore it.
fn dispatch_extend(budget: Duration) -> Option<Instant> {
    let deadline = Instant::now() + budget;
    with_state(|state| {
        let previous = state.dispatch.deadline;
        // Never shorten an existing deadline.
        if previous.is_none_or(|p| deadline > p) {
            state.dispatch.deadline = Some(deadline);
        }
        previous
    })
}

fn dispatch_restore(previous: Option<Instant>) {
    with_state(|state| state.dispatch.deadline = previous);
}

/// Re-arm every live slot relative to now, after a detected discontinuity.
pub(crate) fn rearm_live_slots() {
    let now = Instant::now();
    with_state(|state| {
        if state.dispatch.deadline.is_some() {
            state.dispatch.deadline = Some(now + DEFAULT_DISPATCH_BUDGET);
        }
        if state.tx.deadline.is_some() {
            state.tx.deadline = Some(now + TX_BUDGET);
        }
    });
}

/// A `WireTx` operation started. Arms the TX slot on the 0→1 edge.
pub(crate) fn tx_enter() {
    let deadline = Instant::now() + TX_BUDGET;
    with_state(|state| {
        state.tx_inflight = state.tx_inflight.saturating_add(1);
        if state.tx_inflight == 1 {
            state.tx = Slot {
                deadline: Some(deadline),
                key: 0,
            };
        }
    });
}

/// A `WireTx` operation finished. Disarms the TX slot on the 1→0 edge.
pub(crate) fn tx_exit() {
    with_state(|state| {
        state.tx_inflight = state.tx_inflight.saturating_sub(1);
        if state.tx_inflight == 0 {
            state.tx = Slot::IDLE;
        }
    });
}

/// Restores the previous dispatch deadline when dropped, including when the
/// handler future is cancelled.
///
/// Returned by value rather than as `impl Drop` so callers can name the type
/// and so the lifetime is obvious at the binding site.
pub(crate) struct BudgetGuard {
    previous: Option<Instant>,
}

impl Drop for BudgetGuard {
    fn drop(&mut self) {
        dispatch_restore(self.previous);
    }
}

/// The caller-supplied timeout expired.
pub(crate) struct Expired;

/// Clamp a caller-supplied timeout, declare the matching supervisor budget,
/// and run `fut` under it.
///
/// Clamping and declaring happen in one call so the two cannot drift apart.
/// `requested_ms == 0` clamps to [`MAX_HANDLER_TIMEOUT`] — it no longer means
/// "wait forever".
pub(crate) async fn bounded<F: core::future::Future>(requested_ms: u32, fut: F) -> Result<F::Output, Expired> {
    let requested = if requested_ms == 0 {
        MAX_HANDLER_TIMEOUT
    } else {
        Duration::from_millis(u64::from(requested_ms)).min(MAX_HANDLER_TIMEOUT)
    };
    let _guard = BudgetGuard {
        previous: dispatch_extend(requested + DISPATCH_SLACK),
    };
    embassy_time::with_timeout(requested, fut).await.map_err(|_| Expired)
}

/// Declare a supervisor budget for a handler whose duration is bounded but
/// not expressed as a caller timeout, such as `spi/batch`'s accumulated
/// `DelayNs` ops or `uart/flush`'s buffer drain.
///
/// The returned guard restores the previous deadline when dropped, so bind it
/// with `let _budget = ...` — a bare `let _ = ...` drops it immediately and
/// silently does nothing.
pub(crate) fn declare(budget: Duration) -> BudgetGuard {
    BudgetGuard {
        previous: dispatch_extend(budget.min(MAX_HANDLER_TIMEOUT) + DISPATCH_SLACK),
    }
}

use postcard_rpc::server::WireRx;

/// Wraps a [`WireRx`] so the dispatch slot arms exactly when a frame is
/// received and disarms exactly when the server goes back to waiting.
///
/// Blocking inside `receive()` is legitimate idle, not a wedge, which is why
/// the slot is disarmed *before* delegating.
pub(crate) struct WatchedRx<R> {
    inner: R,
}

impl<R> WatchedRx<R> {
    pub(crate) const fn new(inner: R) -> Self {
        Self { inner }
    }
}

/// First four header bytes of a frame, as a breadcrumb.
///
/// Deliberately not a decoded `VarKey`: the key width varies with
/// `VarKeyKind`, and this value only has to survive a reset and identify the
/// culprit in a log, not round-trip.
fn frame_breadcrumb(frame: &[u8]) -> u32 {
    let mut bytes = [0u8; 4];
    let n = frame.len().min(4);
    bytes[..n].copy_from_slice(&frame[..n]);
    u32::from_le_bytes(bytes)
}

impl<R: WireRx> WireRx for WatchedRx<R> {
    type Error = R::Error;

    async fn wait_connection(&mut self) {
        dispatch_disarm();
        self.inner.wait_connection().await;
    }

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        dispatch_disarm();
        let result = self.inner.receive(buf).await;
        if let Ok(frame) = &result {
            dispatch_arm(DEFAULT_DISPATCH_BUDGET, frame_breadcrumb(frame));
        }
        result
    }
}