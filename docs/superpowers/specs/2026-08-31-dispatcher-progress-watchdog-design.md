# Dispatcher progress supervision

Design for [#157](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/157).

Status: approved, not implemented.
Date: 2026-08-31.

## 1. Problem

`watchdog_feeder_task` (`crates/pico-de-gallo-firmware/src/main.rs:552-560`) is
an independent embassy task:

```rust
async fn watchdog_feeder_task(mut watchdog: Watchdog) {
    watchdog.start(Duration::from_secs(2));
    watchdog.pause_on_debug(true);
    loop {
        Timer::after(Duration::from_millis(800)).await;
        watchdog.feed(Duration::from_secs(2));
    }
}
```

It shares nothing with the postcard-rpc dispatcher. postcard-rpc 0.12.1
dispatches handlers serially on one `&mut Context`, so a handler that never
returns blocks every endpoint — while the feeder keeps being scheduled and
keeps feeding. The watchdog therefore proves **executor liveness**, not
**dispatcher progress**.

Three device-wide wedges have survived it, from three unrelated triggers, all
recorded in AGENTS.md §13.17:

| Date       | Trigger                                       |
|------------|-----------------------------------------------|
| 2026-06-03 | `gpio/wait-*` on a pin that never transitions |
| 2026-08-19 | `spi/transfer` at the packet-framing boundary |
| 2026-08-26 | Zero-length I2C write                         |

Each trigger has since been guarded individually. The mechanism that let all
three become device-wide and unrecoverable has not been addressed. This design
is the backstop for the next trigger nobody has thought of yet; it does not
replace the per-trigger guards.

### 1.1 Verification that the issue is still live

Checked against `main` at `417739147e1c`:

- `main.rs:552-560` — feeder unchanged.
- `main.rs:526-530` — server loop is still `loop { let _ = server.run().await; }`.
- postcard-rpc 0.12.1 `src/server/mod.rs:455-491` — no progress hook.
- `ROADMAP.md:194`, `ROADMAP.md:240` — still listed as open work.

## 2. The measured long-dispatch tail

A supervisor needs to know what a *legitimate* slow dispatch looks like.

| Endpoint                  | Worst legitimate case                                | Bounded by     |
|---------------------------|------------------------------------------------------|----------------|
| `gpio/wait-*` (×5)        | 49.7 days (`u32` ms), or unbounded at `timeout_ms == 0` | caller       |
| `uart/read`               | 49.7 days (`u32` ms), or 1 ms poll at `0`            | caller         |
| `spi/batch`               | 274.9 s — `MAX_BATCH_OPS` 64 × `DelayNs{u32}` 4.295 s | caller         |
| `onewire/write-pullup`    | 65.5 s (`u16` ms)                                    | caller         |
| `i2c/scan`                | 6.4 s — 128 × 50 ms (`handlers/i2c.rs:124`)          | fixed          |
| `uart/write`, `uart/flush`| ~27 s — 1024-byte buffer at 300 baud                 | caller (baud)  |
| the other ~40 handlers    | µs–ms                                                | fixed          |

34 of the 47 handlers are `async`; 13 are `blocking` and cannot park on an
await at all.

`timeout_ms == 0` is documented wait-forever behaviour in all five
`gpio/wait-*` handlers and in `uart/read`, for example `handlers/gpio.rs:101-103`:

```rust
if req.timeout_ms == 0 {
    gpio.wait_for_high().await;   // unbounded, by design
    Ok(())
}
```

That is simultaneously a supported feature and the 2026-06-03 wedge trigger,
and a supervisor cannot tell the two apart. Resolving that tension is decision
§3.1.

## 3. Decisions

### 3.1 Every dispatch is capped at a hard ceiling

`timeout_ms == 0` stops meaning "forever". Both `0` and oversized values clamp
to `MAX_HANDLER_TIMEOUT` = **30 minutes**. The handler's own `with_timeout`
still fires and still returns its normal `GpioError::Timeout` /
`UartError::Timeout`, so callers get a clean error rather than a reboot.

Rejected: honouring wait-forever and protecting only the other handlers. It
leaves the 2026-06-03 trigger unguarded, so the issue's acceptance criterion
would only be partly met.

### 3.2 The supervisor resets the device, and leaves a breadcrumb

On expiry: `defmt::error!`, write a reason word plus the in-flight key to
watchdog scratch registers, then `watchdog.trigger_reset()`.

Reset drops USB and every GPIO subscription. That is a deliberate, documented
loss, and strictly better than today's wedge, which requires physical
re-enumeration or a power cycle.

Rejected: merely ceasing to feed. It adds up to 2 s of watchdog slop and gives
no opportunity to log or record a cause.

### 3.3 Progress is observed by wrapping `WireTx` and `WireRx`

Not by forking postcard-rpc and not by instrumenting 47 handlers.

### 3.4 Reset reporting is defmt-only for now

Surfacing the last reset reason over the wire belongs to
[#159](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/159)
(firmware build identity in `device/info`) and is out of scope here.

### 3.5 No version bumps in this work

Versions are bumped by hand at release time. See §8 for the risk this creates
and the obligation it places on the pre-release checklist.

## 4. Mechanism

### 4.1 Where the signal comes from

postcard-rpc's `Server::run()` (`src/server/mod.rs:455-491`) is:

```rust
rx.wait_connection().await;        // idle
tx.tx.wait_connection().await;     // idle
let used = rx.receive(buf).await;  // idle until a frame lands
let fut = d.handle(tx, &hdr, body);
fut.await;                         // the only place a wedge has ever lived
```

`WireRx` (`server/mod.rs:115-131`) is public, unsealed and two methods wide.
`WireTx` (`server/mod.rs:45-77`) is public, unsealed and five methods wide, all
taking `&self`. Newtype wrappers therefore give an exact idle/in-flight edge
with no fork and no per-handler boilerplate.

```rust
struct WatchedRx<R: WireRx>(R);

impl<R: WireRx> WireRx for WatchedRx<R> {
    type Error = R::Error;

    async fn wait_connection(&mut self) {
        progress::dispatch_disarm();
        self.0.wait_connection().await
    }

    async fn receive<'a>(&mut self, buf: &'a mut [u8]) -> Result<&'a mut [u8], Self::Error> {
        progress::dispatch_disarm();     // blocking here is idle, not a wedge
        let r = self.0.receive(buf).await;
        if let Ok(frame) = &r {
            // key_of() parses the header with VarHeader::take_from_slice,
            // the same public call Server::run() uses on the next line.
            progress::dispatch_arm(DEFAULT_DISPATCH_BUDGET, key_of(frame));
        }
        r
    }
}
```

`Server<AppTx, WatchedRx<AppRx>, WireRxBuf, PicoDeGallo>`; one line changes at
`main.rs:523`.

### 4.2 The TX slot

`EUsbWireTx` is `Clone + Copy` and `AppTx` is cloned into four
`gpio_monitor_task`s at `main.rs:520` and into `Sender`. So `WatchedTx` derives
`Clone`, shares one global slot, and concurrent senders force an **in-flight
counter** rather than a flag: arm on 0→1, disarm on 1→0. `wait_connection()`
disarms, because a host that is not attached is legitimate idle.

TX gets its **own** slot, checked independently of the dispatch slot. Keeping
them separate stops `bounded()`'s 30-minute dispatch extension (§4.4) from
silently widening the TX budget as well.

### 4.3 What TX wrapping actually covers

Narrower than "catches stalled sends", and worth writing down so nobody
overclaims it later.

postcard-rpc already bounds the endpoint writes: `send_all`
(`server/impls/embassy_usb_v0_5.rs:507-524`) computes
`frames * timeout_ms_per_frame`, default 2 ms/frame clamped to `1..=60000`, and
times out. What it does *not* bound, per its own documentation at
`embassy_usb_v0_5.rs:272-276`:

> "This timer is not started until the sender has exclusive access to the
> underlying USB connection... the second sender's timer will not start until
> the first sender has completed."

So `self.inner.lock().await` on `send()`'s first line is unbounded, and so is
`wait_connection()`'s `ep_in.wait_enabled().await`.

The TX slot's **unique** coverage is therefore the four `gpio_monitor_task`
topic paths, which run entirely outside `Server::run()` and which the dispatch
slot cannot see at all, plus TX-mutex starvation. Handler-initiated sends are
already inside the dispatch slot.

**Amended during implementation.** The slot is shared by all senders and is
refreshed whenever any one of them completes, because otherwise sustained
healthy traffic expires a deadline pinned to the first sender's arm time and
resets a working device. The consequence is that the slot measures
**aggregate** TX progress, not per-sender progress: one permanently starved
sender is masked for as long as some other sender completes at least once per
[`TX_BUDGET`](#5-numbers). Complete starvation — no sender completing at all —
is still detected. This is a weaker guarantee than "detects TX-mutex
starvation" and is stated as such in the `WatchedTx` doc comment and in the
AGENTS.md §13.17 row.

### 4.4 Declared budgets

One primitive does the clamping and the declaring together, so the two cannot
drift apart:

```rust
/// Clamps `requested_ms` (0 → MAX) to MAX_HANDLER_TIMEOUT, extends the
/// dispatch deadline to match, runs `fut` under `with_timeout`, and restores
/// the previous deadline on drop — including on cancellation.
pub async fn bounded<F: Future>(requested_ms: u32, fut: F) -> Result<F::Output, Expired>;
```

Ten handlers declare:

| Handler(s)                                          | Declaration                                                     |
|-----------------------------------------------------|-----------------------------------------------------------------|
| `gpio/wait-{high,low,rising,falling,any}`, `uart/read` | `bounded(req.timeout_ms, fut)`                               |
| `spi/batch`                                          | sum of `DelayNs` ops during the validation pass it already runs |
| `onewire/write-pullup`                               | `req.pullup_duration_ms`                                        |
| `uart/write`, `uart/flush`                           | fixed 60 s                                                      |

`uart/write` and `uart/flush` use a fixed value rather than computing from the
configured baud rate; the coupling is not worth the precision.

Everything else runs on the default budget. A new slow handler that forgets to
declare resets the board at 10 s — loud, immediate, caught the first time
anyone runs it, never silent.

### 4.5 Shared state

`embassy_sync::blocking_mutex::Mutex<CriticalSectionRawMutex, Cell<...>>`
holding, per slot, an `Option<Instant>` deadline and the in-flight key.

No new dependency: `critical-section` 1.2.0 and `embassy-sync` 0.7.2 are
already direct dependencies and `CriticalSectionRawMutex` is already imported
at `main.rs:71`.

Deliberately **not** an atomic. RP2350 is a Cortex-M33 with no 64-bit atomic
instructions, and truncating `Instant` to `u32` ticks reintroduces wraparound
bugs on the recovery path.

### 4.6 Supervisor task

`watchdog_feeder_task` becomes the supervisor. It already owns the `Watchdog`
(`main.rs:386`), which is exactly what `set_scratch` and `trigger_reset`
require, so no ownership rework is needed.

It polls at 250 ms, feeds the watchdog unless a slot has expired, and on expiry
performs §4.7.

### 4.7 Debugger interaction

`pause_on_debug(true)` stops the watchdog counter while a debugger has the core
halted, but the embassy time driver keeps counting. On resume the supervisor
would see an expired deadline and reset, making debugging worse rather than
better.

The supervisor timestamps its own wakes. If the observed gap exceeds 2× its
poll period, it treats that as a time discontinuity — debugger halt or executor
starvation — re-arms the affected slots and does not fire. Self-contained; no
configuration.

### 4.8 Breadcrumb

```text
defmt::error!(...)                      // slot, in-flight key, armed-at, budget
watchdog.set_scratch(0, MAGIC | reason)
watchdog.set_scratch(1, key_low_32)
watchdog.trigger_reset()
```

`trigger_reset()` sets `CTRL.TRIGGER`, which `reset_reason()` reports as
`ResetReason::Forced`. That distinguishes it from `TimedOut` (a genuine
watchdog expiry, meaning the executor itself died) and from `None` (cold boot).
The `MAGIC` word separates our forced reset from a `picotool reboot`, which
goes through the same trigger path.

`main()` constructs the `Watchdog` before spawning, so it reads
`reset_reason()` and the scratch registers, logs, clears the scratch, and then
hands the peripheral to the supervisor.

**Scratch indices 0 and 1, pending verification.** On RP2040 the bootrom owns
SCRATCH4–7 for the watchdog reboot vector. RP2350's bootrom scratch usage has
**not** been confirmed. Confirming it against the datasheet is an explicit
implementation step, not an assumption.

## 5. Numbers

| Knob                      | Value      | Justification                                                            |
|---------------------------|------------|--------------------------------------------------------------------------|
| `MAX_HANDLER_TIMEOUT`     | 30 minutes | Ceiling for `timeout_ms == 0` and for oversized values                   |
| `DEFAULT_DISPATCH_BUDGET` | 10 s       | Covers `i2c/scan`'s 6.4 s worst case with margin                         |
| `DISPATCH_SLACK`          | +30 s      | Added to **declared** budgets only, not to the default. Absorbs the handler's own `with_timeout` firing and reply serialisation |
| `TX_BUDGET`               | 60 s       | Absence of *aggregate* TX completion (see §4.3); a real send is already ≤60 s by postcard-rpc |
| supervisor poll           | 250 ms     | Worst-case reset latency is budget + 250 ms                              |
| watchdog period           | 2 s        | Unchanged. We fire via `trigger_reset()`, not by starving the feed       |

## 6. Acceptance

Reproducing a historical trigger on a deliberately-unguarded build must reset
the device within a bounded time rather than wedging until USB re-enumeration.

### 6.1 Test hatch

Corrected during planning. The hatch cannot live in `ping_handler`:
`main.rs:321` registers `PingEndpoint` as `blocking`, and a blocking handler
cannot `.await`, so `core::future::pending()` there does not compile.

Instead, `wedge-test` disables the zero-length I2C write guard (#101,
`handlers/i2c.rs`):

```rust
#[cfg(not(feature = "wedge-test"))]
if req.contents.is_empty() {
    warn!("i2c write: empty payload refused (addr={=u8:#x})", req.address);
    return Err(I2cError::ZeroLengthWrite);
}
```

This reproduces the actual 2026-08-26 trigger and corrupts nothing: the whole
defect is that `write_async_internal` queues no command and starts no
transaction, then awaits a `STOP_DET`/`TX_ABRT` interrupt that only a started
transaction can raise. No I2C target is required.

Reachable from `gallo-mcp`'s `i2c_write` with `data: ""` and from
`pico-de-gallo-lib`'s `#[ignore]`d #135 regression tests. **Not** reachable
from the `gallo` CLI, whose clap `num_args(1..)` requires at least one byte.

The `i2c/batch` guard stays enabled: one trigger is enough, and that guard is
the one preventing partial bus writes.

Removing the #128 batch-atomicity behaviour instead was rejected: it means
shipping a build that can corrupt an I2C device, for no extra signal.

### 6.2 Expected result

`defmt::error!`, forced reset, USB re-enumeration, and at next boot
`reset_reason() == Forced` with `MAGIC` in scratch — within
`DEFAULT_DISPATCH_BUDGET` + 250 ms, about 10 s.

### 6.3 What is not covered

- **A wedge inside `receive()` itself** is indistinguishable from legitimate
  idle and is not covered. All three documented triggers are handler-side, so
  all three are covered, but the mechanism is not complete and must not be
  described as such.
- **The TX slot has no hardware trigger.** Starving the TX mutex on demand
  needs a second contrived hook, which does not pay for itself. TX-slot
  correctness rests on inspection and on §6.4.
- **The TX slot measures aggregate, not per-sender, progress** — see the
  amendment in §4.3. One starved sender is masked while another completes.
- **Reaching the trigger needs a host-side mutation too.** Discovered during
  implementation: `check_i2c_write_payload` (`pico-de-gallo-lib/src/lib.rs:316`)
  rejects an empty payload before it reaches the wire, and every host surface —
  FFI, Python, MCP, Zephyr — routes through it. That guard is correct and is
  #135's whole point, but it means §6.1's hatch is not reachable from an
  unmodified host. The acceptance run therefore requires **two** temporary
  uncommitted mutations: the host guard *and* the Task 8 supervisor control.

### 6.4 Unit coverage

The firmware crate has no test harness at all — AGENTS.md §5.5 lists zero
firmware tests, and `cargo test` does not link against a `no_std` / `no_main`
embassy crate as configured.

The policy is therefore factored as one pure function with no hardware and no
`embassy-time` dependency:

```rust
fn decide(now: Instant, last_wake: Instant, dispatch: Slot, tx: Slot) -> Action
```

so that the deadline arithmetic, the 30-minute clamp and the discontinuity rule
*can* be unit tested.

Factoring `decide()` as a pure function is **required** by this change.
Building the harness that actually runs those tests is **out of scope** and
becomes a follow-up issue. That leaves recovery-path code shipping without
automated coverage, which is where the last three bugs lived — a named,
accepted gap, not an oversight.

### 6.5 CI

`nostd.yml` gains `wedge-test` as a clippy-only configuration alongside the
existing `[rev1, rev2]` matrix, so the hatch cannot bit-rot. It must never
appear in `release-firmware.yml`.

## 7. Documentation

`timeout_ms` is documented as `0 = wait forever` in every host surface —
`internal`, `lib`, `hal`, `ffi`, `app`, `mcp` (`gpio.rs`, `uart.rs`,
`encoding.rs`) and `pyco`. All need updating.

Book, per §15.1:

- `book/src/interfaces/uart.md`, `book/src/crates/{ffi,lib,mcp}.md` — already
  document the `0` semantics.
- `book/src/interfaces/gpio.md` — apparently does not mention `timeout_ms` at
  all. Pre-existing gap; close it here.
- `book/src/internals/firmware.md` — the supervisor and the watchdog section.
- `book/src/appendix/troubleshooting.md` — new entry for "the board rebooted
  itself".

Repo bookkeeping:

- New AGENTS.md §13.17 row.
- `ROADMAP.md:240` — remove #157 from the open-work table, and rewrite the
  dispatcher-wedge narrative at `ROADMAP.md:190-194`, which currently asserts
  the watchdog cannot catch this.
- `crates/pico-de-gallo-firmware/CHANGELOG.md`, plus the CHANGELOGs of any
  crate whose documented `timeout_ms` semantics change.

**Zephyr is unaffected.** No `gallo_gpio_wait*` or `gallo_uart_read` call
exists anywhere under `zephyr/`. §15.1's reverse-direction obligation is
satisfied by inspection; state that in the PR body, because a green
`zephyr.yml` run would otherwise be mistaken for evidence it does not provide.

## 8. Risks

**The schema version will be dishonest until release.** Per §3.5 no versions
move in this work. `pico-de-gallo-internal` stays at `0.7.0`, and
`internal-v0.7.0` is already tagged and published, so the tree will contain
firmware that reports schema 0.7 while interpreting `timeout_ms == 0`
differently from the published 0.7.

The wire *shape* is unchanged, which is exactly what makes this dangerous:
`validate()` compares versions, so it cannot detect the difference. This is the
same hazard as the 2026-08-26 row in AGENTS.md §13.17 ("`validate()` cannot
tell the two builds apart... that misidentified a flash during this very
verification, so track the flashed image yourself").

Consequences to accept deliberately:

1. Host and firmware must be built from the same tree until the bump lands.
2. AGENTS.md §16 step 2 becomes load-bearing: the pre-release checklist must
   catch this and bump `internal` to `0.8.0` in lockstep across all eight
   released crates, with dep-spec rewrites, CHANGELOGs and both lockfiles,
   before any artifact is built or tagged.
3. Nothing automated enforces (2). It is a documented obligation on a human.

**A reset drops GPIO subscriptions.** A host holding subscriptions sees them
vanish on re-enumeration. Acceptable: the alternative is a wedge that loses
them anyway and does not come back.

**A misjudged budget resets a legitimate operation.** Mitigated by the
30-minute ceiling, the +30 s slack and the discontinuity rule, and bounded by
the fact that the declaring set is enumerable at ten handlers.

## 9. Commit split

Each commit builds cleanly on its own, per §13.12.

1. `feat(firmware): Add dispatch progress supervision` — `progress` module,
   `WatchedRx` / `WatchedTx`, supervisor task, breadcrumb.
2. `feat(firmware): Clamp handler timeouts to a 30 minute ceiling` —
   `bounded()` and the ten declaring handlers.
3. `feat(firmware): Add wedge-test acceptance hatch` — plus the `nostd.yml`
   configuration.
4. `docs(...)` — book, AGENTS.md, ROADMAP, CHANGELOGs, and the `timeout_ms`
   rustdoc on every host surface listed in §7. Scoped to the crates it touches,
   not `repo` alone.
