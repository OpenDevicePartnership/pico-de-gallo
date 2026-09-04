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

use core::cell::Cell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Instant};
use pico_de_gallo_internal::{MAX_HANDLER_TIMEOUT_MS, UNDECLARED_DISPATCH_BUDGET_MS};

/// Ceiling for any caller-supplied handler timeout.
///
/// `timeout_ms == 0` no longer means "wait forever": both `0` and oversized
/// values clamp to this. The handler's own `with_timeout` still fires and
/// still returns its normal `Timeout` error, so callers get a clean error
/// rather than a reboot.
///
/// Derived from [`MAX_HANDLER_TIMEOUT_MS`] so the host, which uses the same
/// constant to bound its own waits, cannot drift from the firmware.
pub(crate) const MAX_HANDLER_TIMEOUT: Duration = Duration::from_millis(MAX_HANDLER_TIMEOUT_MS as u64);

/// Budget for a dispatch that declares nothing.
///
/// Sized to cover `i2c/scan`'s worst case (128 addresses × 50 ms = 6.4 s,
/// `handlers/i2c.rs`) with margin. Every other undeclared handler is µs–ms.
///
/// Derived from [`UNDECLARED_DISPATCH_BUDGET_MS`] so the host, which uses the
/// same constant to bound `i2c/scan`, cannot drift from the firmware.
pub(crate) const DEFAULT_DISPATCH_BUDGET: Duration = Duration::from_millis(UNDECLARED_DISPATCH_BUDGET_MS as u64);

/// Added to **declared** budgets only, never to [`DEFAULT_DISPATCH_BUDGET`].
///
/// Absorbs the handler's own `with_timeout` firing plus reply serialisation,
/// so a handler that legitimately runs to its declared ceiling completes
/// normally instead of tripping the supervisor.
pub(crate) const DISPATCH_SLACK: Duration = Duration::from_secs(30);

/// Budget for an in-flight `WireTx` operation.
///
/// Detects absence of aggregate TX completion, including complete TX-mutex
/// starvation. A sender completing while others remain refreshes the deadline.
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
    const IDLE: Self = Self { deadline: None, key: 0 };
}

/// Full supervised state. `Copy` so it can live in a `Cell`.
#[derive(Clone, Copy)]
pub(crate) struct State {
    pub(crate) dispatch: Slot,
    pub(crate) tx: Slot,
    /// Concurrent `WireTx` senders. The TX slot arms on 0→1, refreshes whenever
    /// one sender completes while others remain, and disarms on 1→0.
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

/// Extend the dispatch deadline without ever shortening a live declaration.
fn dispatch_extend(budget: Duration) {
    let deadline = Instant::now() + budget;
    with_state(|state| {
        if state.dispatch.deadline.is_none_or(|current| deadline > current) {
            state.dispatch.deadline = Some(deadline);
        }
    });
}

/// Guarantee at least [`DISPATCH_SLACK`] remains on a live dispatch slot.
///
/// Never arms an idle slot and never shortens a longer live deadline.
fn dispatch_ensure_reply_budget() {
    let deadline = Instant::now() + DISPATCH_SLACK;
    with_state(|state| {
        if let Some(current) = state.dispatch.deadline
            && deadline > current
        {
            state.dispatch.deadline = Some(deadline);
        }
    });
}

/// Shift every live deadline by the observed scheduling discontinuity.
///
/// This preserves each slot's remaining declared budget across debugger halts
/// or severe executor starvation.
pub(crate) fn rearm_live_slots(elapsed: Duration) {
    with_state(|state| {
        if let Some(deadline) = state.dispatch.deadline {
            state.dispatch.deadline = Some(deadline + elapsed);
        }
        if let Some(deadline) = state.tx.deadline {
            state.tx.deadline = Some(deadline + elapsed);
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

/// A `WireTx` operation finished.
///
/// Disarms on the 1→0 edge. If other senders remain, completion is observable
/// TX progress and refreshes their shared deadline.
pub(crate) fn tx_exit() {
    let deadline = Instant::now() + TX_BUDGET;
    with_state(|state| {
        state.tx_inflight = state.tx_inflight.saturating_sub(1);
        if state.tx_inflight == 0 {
            state.tx = Slot::IDLE;
        } else {
            state.tx.deadline = Some(deadline);
        }
    });
}

/// Ensures reply-serialization slack remains after guarded work finishes.
///
/// The active declaration is never shortened. On drop, including cancellation,
/// the dispatch retains at least [`DISPATCH_SLACK`] from the current instant.
pub(crate) struct BudgetGuard;

impl Drop for BudgetGuard {
    fn drop(&mut self) {
        dispatch_ensure_reply_budget();
    }
}

/// The caller-supplied timeout expired.
pub(crate) struct Expired;

/// Clamp a caller-supplied timeout, declare the matching supervisor budget,
/// and run `fut` under it.
///
/// Clamping and declaring happen in one call so the two cannot drift apart.
/// `requested_ms == 0` clamps to [`MAX_HANDLER_TIMEOUT`]. The declaration
/// remains live through response serialization.
pub(crate) async fn bounded<F: core::future::Future>(requested_ms: u32, fut: F) -> Result<F::Output, Expired> {
    let requested = if requested_ms == 0 {
        MAX_HANDLER_TIMEOUT
    } else {
        Duration::from_millis(u64::from(requested_ms)).min(MAX_HANDLER_TIMEOUT)
    };

    dispatch_extend(requested + DISPATCH_SLACK);
    let _guard = BudgetGuard;

    embassy_time::with_timeout(requested, fut).await.map_err(|_| Expired)
}

/// Declare a supervisor budget for a handler whose duration is bounded but
/// not expressed as a caller timeout, such as `spi/batch`'s accumulated
/// `DelayNs` ops or `uart/flush`'s buffer drain.
///
/// The returned guard preserves reply-serialization slack when dropped, so
/// bind it with `let _budget = ...`; a bare `let _ = ...` drops it immediately.
pub(crate) fn declare(budget: Duration) -> BudgetGuard {
    dispatch_extend(budget.min(MAX_HANDLER_TIMEOUT) + DISPATCH_SLACK);
    BudgetGuard
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
use core::fmt::Arguments;

use postcard_rpc::header::{VarHeader, VarKeyKind};
use postcard_rpc::server::WireTx;
use serde::Serialize;

/// Wraps a [`WireTx`] so the TX slot is armed while any send is outstanding.
///
/// Coverage is narrower than it looks. postcard-rpc already bounds the
/// endpoint writes themselves (`send_all` in
/// `server/impls/embassy_usb_v0_5.rs` times out at
/// `frames * timeout_ms_per_frame`). What it does not bound is
/// `self.inner.lock().await` on `send()`'s first line — its own docs say the
/// timer "is not started until the sender has exclusive access".
///
/// So this wrapper's *unique* coverage is the four `gpio_monitor_task` topic
/// paths, which run outside `Server::run()` entirely and which the dispatch
/// slot cannot see at all, plus TX-mutex starvation. Handler-initiated sends
/// are already inside the dispatch slot.
#[derive(Clone, Copy)]
pub struct WatchedTx<T> {
    inner: T,
}

impl<T> WatchedTx<T> {
    pub const fn new(inner: T) -> Self {
        Self { inner }
    }
}

/// Arms the TX slot on construction and disarms on drop, so a cancelled send
/// cannot leave the slot armed forever.
struct TxGuard;

impl TxGuard {
    fn new() -> Self {
        tx_enter();
        Self
    }
}

impl Drop for TxGuard {
    fn drop(&mut self) {
        tx_exit();
    }
}

impl<T: WireTx> WireTx for WatchedTx<T> {
    type Error = T::Error;

    async fn wait_connection(&self) {
        // A host that is not attached is legitimate idle, not a fault.
        self.inner.wait_connection().await;
    }

    async fn send<M: Serialize + ?Sized>(&self, hdr: VarHeader, msg: &M) -> Result<(), Self::Error> {
        let _guard = TxGuard::new();
        self.inner.send(hdr, msg).await
    }

    async fn send_raw(&self, buf: &[u8]) -> Result<(), Self::Error> {
        let _guard = TxGuard::new();
        self.inner.send_raw(buf).await
    }

    async fn send_log_str(&self, kkind: VarKeyKind, s: &str) -> Result<(), Self::Error> {
        let _guard = TxGuard::new();
        self.inner.send_log_str(kkind, s).await
    }

    async fn send_log_fmt<'a>(&self, kkind: VarKeyKind, a: Arguments<'a>) -> Result<(), Self::Error> {
        let _guard = TxGuard::new();
        self.inner.send_log_fmt(kkind, a).await
    }
}
