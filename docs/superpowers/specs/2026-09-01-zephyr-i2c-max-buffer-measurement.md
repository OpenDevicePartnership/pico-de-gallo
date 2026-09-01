# Measuring the I2C transfer ceiling (issue #146)

**Status:** design, approved 2026-09-01. Uncommitted by request — the
measurement harness and this document are one-off validation artifacts.
Only the resulting constant and the documentation land.

**Board under test:** serial `49742081C885AC69`, firmware 0.11.0, schema
0.7, hardware revision 2, `num_gpios` 4. One I2C target on the bus: a
TMP102 at `0x48`.

---

## 1. Problem

`PDG_I2C_MAX_BUFFER` in `zephyr/drivers/i2c/pdg_i2c.c:101` is `4096U`,
inherited from `pico_de_gallo_internal::MAX_TRANSFER_SIZE`. That is a
packet-buffer and argument bound, not a measured end-to-end ceiling.

The sibling SPI driver was built on the same mental model and the model
was wrong twice: on `spi/transfer`, 4096 TX-only failed `-ECOMM`, a
reasoned 3072-byte full-duplex guess also failed `-ECOMM`, and 1015
TX-only wedged the firmware dispatcher device-wide.
`PDG_SPI_MAX_BUFFER` was consequently lowered to 1013 (AGENTS.md
§13.17, 2026-08-19).

`i2c/write` carries its whole payload in the request frame exactly as
`spi/transfer` does, and `i2c/write-read` carries a payload in each
direction, which is structurally what defeated the 3072-byte duplex
guess. No equivalent measurement has ever been taken for I2C.

## 2. Goal

Measure, on attached hardware, the largest `i2c/write` and
`i2c/write-read` payloads for which the call returns at all. Set
`PDG_I2C_MAX_BUFFER` from that measurement, with the same explicitly
"measured, not derived" framing `PDG_SPI_MAX_BUFFER` carries.

Out of scope, and deliberately so: guarding the host surfaces (CLI,
Rust, C, Python, MCP), and deriving the ceiling from framing as a shared
contract in `pico-de-gallo-internal`. Both are named in issue #146 as the
longer-term fix; both carry schema and lockstep-release implications.

## 3. Harness

A throwaway binary at `/tmp/opencode/i2c-probe/`, depending on
`pico-de-gallo-lib` by path. Not committed.

### 3.1 `probe(kind, w_len, r_len) -> Outcome`

Opens the board by serial number, issues exactly one RPC under a
`tokio::time::timeout`, and closes the connection. One connection per
probe, so a hang cannot leak state into the next probe.

Four outcomes:

| Outcome | Meaning |
|---|---|
| `Ok` | the call returned success |
| `Nack` | `I2cError::NoAcknowledge` — the expected healthy answer at an unpopulated address |
| `Err(e)` | any other error, recorded verbatim; transport failures live here |
| `Hang` | the timeout fired |

`Ok` and `Nack` both count as "the call returned", which is the property
under measurement. `Err` and `Hang` are distinguished because they mean
very different things: an error is a working transport refusing a
request, a hang is a wedged dispatcher.

**Timeout: 5 s.** 4096 bytes at 100 kHz standard-mode I2C is roughly
0.4 s of bus time, so this is about a 10x margin. A false `Hang` would
corrupt the boundary, so the bias is deliberately toward over-waiting.

### 3.2 `recover()`

Run after any `Hang`:

1. Re-resolve the device by VID:PID `045e:067d`. The bus path changes
   across re-enumeration, so it must be looked up fresh, never cached.
2. Issue `USBDEVFS_RESET` (`0x5514`) on the device node. Verified
   available unprivileged: the node carries an ACL for the invoking
   user.
3. Wait for the node to reappear.
4. Confirm liveness with `ping`.

If `ping` still fails after a bounded number of attempts, the harness
stops and asks for a power-cycle rather than silently producing garbage
from a still-wedged board.

### 3.3 Liveness check between every probe

A `ping` before each probe. Without it, one missed wedge silently
poisons every subsequent data point by attributing the wedge's failures
to the lengths being probed.

### 3.4 Output

One CSV line per probe:
`kind,w_len,r_len,outcome,detail,elapsed_ms`. Appended and flushed
immediately, so a crash or power-cycle does not lose probes already
taken.

## 4. Sweep procedure

### Phase A — write-only edge

`i2c_write(0x50, [0u8; N])` against an unpopulated address. The full
payload still crosses USB and is decoded, so request framing is fully
exercised, but nothing is written to any real device.

Coarse ladder first: 1, 64, 256, 512, 1013, 1015, 1024, 2048, 3072,
4096. Chosen so results are directly comparable to the SPI table and so
the SPI edge pair 1013/1015 is checked explicitly even if the real I2C
edge is elsewhere.
Then bisect between the largest passing and smallest failing rung to the
exact byte.

The healthy outcome here is `Nack`, not `Ok`. An `Ok` at `0x50` would
mean something is actually present; stop and re-scan the bus.

### Phase B — read-only edge

`i2c_write_read(0x48, [0x00], N)` against the TMP102. The 1-byte write
is the pointer register; the TMP102 clocks out its register pair
repeatedly for arbitrary read lengths, so a genuine N-byte response is
produced and response framing is really exercised. Same ladder, same
bisection.

### Phase C — the W+R frontier

For each of roughly five write lengths spanning Phase A's range (1, 25%,
50%, 75%, and just under the Phase A edge), bisect the largest read
length that still returns. Against `0x48`, so both directions are real.

Phase C is the phase that earns its keep. If the frontier is a rectangle
(`max_w` and `max_r` independent), the constraint is per-direction. If
it is a diagonal (`w + r <= K`), it is a shared budget — the shape that
made the SPI 3072-byte duplex guess wrong. Two independent 1-D
bisections cannot distinguish those, which is why they were rejected.

### Bisection and hangs

Bisection treats `Hang` as failure. Every `Hang` costs a `recover()`. If
a hang window exists — a band where the call neither returns nor errors
— record both its edges, and explicitly record any length inside it that
was **not** probed, in the same plainly-stated style the SPI comment
uses.

### TMP102 state

Read back CONFIG (`0x01`), TLOW (`0x02`), THIGH (`0x03`) before Phase B;
restore and verify after Phase C. Phases B and C only ever write the
1-byte pointer, so this should be a no-op. It is there to catch the case
where it is not.

## 5. Derivation rule

Fixed in advance, so the data cannot be rationalised after the fact.

**`PDG_I2C_MAX_BUFFER` becomes the largest length that returned across
every phase** — the minimum of the Phase A edge, the Phase B edge, and
the frontier's worst case.

If Phase C shows a shared budget rather than independent per-direction
limits, a single constant cannot express it faithfully. In that case the
constant takes the conservative value and the comment says outright that
the real constraint is a sum, that the driver's two separate checks
approximate it, and what a caller must do to stay safe. Overstating what
one number can encode is how this constant reached 4096 in the first
place.

Three publishable outcomes:

- **A real ceiling below 4096.** Lower the constant. Issue #146's
  "lowering would regress working behaviour" objection dissolves,
  because those writes are now known not to work.
- **4096 passes everything.** Keep the constant, replace its comment: it
  stops being an unmeasured bound kept out of caution and becomes
  measured and holding. This is a real result, not a null one.
- **A ceiling and a hang window.** Constant goes below the window; the
  comment documents the window's edges and the unprobed lengths inside
  it.

The comment is modelled on `PDG_SPI_MAX_BUFFER`'s, with the same four
parts: what was measured and on what hardware; what is still unknown,
stated plainly; the firmware hang and its recovery procedure if one was
found; and the follow-up warning against raising or lowering by
guesswork. It cites the date, board serial, firmware and schema
versions, and the fact that write probes ran against an unpopulated
address — a future reader needs to know the bus never ACKed in order to
judge whether the number transfers.

No claim is made that the number generalises beyond `i2c/write` and
`i2c/write-read` on firmware 0.11.0 / schema 0.7 on hardware revision 2.
One board, one firmware build, one host stack.

## 6. Deliverables

Branch `issue-146`.

**Code.** `PDG_I2C_MAX_BUFFER` set from the measurement, with the
rewritten comment. The check sites at `pdg_i2c.c:318` (running total for
writes) and `:342` (read check) need no logic change — that is still the
right shape — unless Phase C shows a shared budget, in which case the
read check's error message must stop implying the two limits are
independent.

**Documentation.** This is **not** a book-exempt Zephyr-only change. The
AGENTS.md §15.1 carve-out covers changes the book does not describe, and
the book already publishes claims this measurement will falsify or
sharpen.

| File | Change |
|---|---|
| `zephyr/drivers/i2c/pdg_i2c.c` | constant and comment |
| `zephyr/README.md` | I2C limitation table (`-EMSGSIZE` row, line 713) and a measured-limits paragraph mirroring the SPI one at line 547 |
| `zephyr/CHANGELOG.md` | under Changed, with full reasoning, matching how the SPI lowering was recorded |
| `book/src/interfaces/batching.md` | the "no general ceiling is published" row (line 278) is no longer true for I2C |
| `book/src/interfaces/i2c.md` | the I2C equivalent of the SPI containment note |
| `book/src/appendix/troubleshooting.md` | ditto, including the host-surface reachability warning — a Zephyr constant contains nothing outside Zephyr |
| `AGENTS.md` §13.17 | a new row, if a hang is found: same class as the three existing dispatcher-wedge rows |
| issue #146 | the measurement CSV and a results table, so the raw data outlives the session |

## 7. Verification

In order:

1. `cargo fmt --check` and clippy — unaffected, no Rust changes land.
2. `west build` of `zephyr/tests/pdg_i2c_burst` and the M5 targets
   against `~/zephyrproject`, proving the module still compiles and
   links with the new constant.
3. `twister` on `zephyr/tests/pdg_fake/i2c` — the one hardware-free
   suite that actually executes.
4. `mdbook build book` clean.

**Not claimed as verified:** that the Zephyr driver behaves correctly at
the new boundary on real hardware. `pdg_i2c_burst` is board-attached and
CI only links it. If the new constant is materially lower than 4096,
run `pdg_i2c_burst` on the board manually and say so; if that is not
possible, say that instead. A green build must not be allowed to imply a
behavioural result.

## 8. Open risks

- **A USB reset may not clear the wedge.** The SPI work observed
  recovery via `usbipd detach`/attach on Windows. `USBDEVFS_RESET` on
  Linux is the analogous operation, not the same operation. If it does
  not work, the sweep degrades to manual power-cycles; stop and report
  rather than pressing on.
- **Phase C may show a shape a single constant cannot express.**
  Handled by the derivation rule in §5, but it changes the
  documentation burden.
- **Repeated wedge/reset cycles are the one genuinely aggressive part
  of this plan.** Firmware-level hangs recovered by USB reset, on a
  development board. Judged acceptable for this board and this
  measurement.

## 9. References

- Issue #146; issue #102, the burst-write fix this was split out of
- AGENTS.md §13.17, 2026-08-19 — the SPI framing measurement
- `zephyr/drivers/spi/pdg_spi.c`, the `PDG_SPI_MAX_BUFFER` comment
  (lines 194–234)
- `zephyr/CHANGELOG.md`, "Known Issues" and "Breaking Changes"

## 10. Outcome (2026-09-01)

The measurement ran on board `49742081C885AC69`, firmware 0.11.0,
schema 0.7, hardware revision 2, from a Linux host. The throwaway
harness drove `pico-de-gallo-lib` directly, issued one RPC per probe
with a 5 s timeout and a fresh connection, checked liveness before each
probe, and verified successful response lengths exactly.

Phase A probed `i2c_write` at unpopulated address `0x50`. Lengths 1,
64, 256, 512, 1013, 1015, 1024, 2048, 3072 and 4096 all returned
`NoAcknowledge` in 1–4 ms; no failing length was found. This establishes
that a 4096-byte request can be framed, delivered, decoded and used to
initiate a transaction. It does **not** establish that 4096 payload
bytes can be clocked to an acknowledging target: the address NACK
aborted each transaction at the address phase, before any payload byte
reached the bus. Nothing above 4096 was expressible because 4096 is
`MAX_TRANSFER_SIZE`, the wire-representable maximum.

Phase B and its addendum measured
`i2c_write_read(0x48, [0x00], r)` against the TMP102. The exact read
edge was `r = 1014`: responses through 1014 succeeded with their full
length, while 1015, 1016, 1024, 2048, 3072 and 4096 all failed on the
host with `Postcard(DeserializeUnexpectedEnd)`. Failure latency scaled
from about 99 ms at 1015 to about 391 ms at 4096. The repeated failing
samples support the monotone-failure assumption used by bisection, and
two independent bisections, in Phases B and C, reproduced the 1014/1015
edge. Exact returned lengths were checked at 1, 64, 256, 512, 1000,
1013 and 1014; no truncation was observed.

Section 4's Phase C procedure could not measure the planned `w × r`
frontier. It rested on the false assumption that the TMP102 tolerates
arbitrary write lengths as it tolerates arbitrary read lengths. The
device instead has a one-byte pointer and two-byte registers and NACKed
writes beyond three bytes. Probes at `w = 64, 256, 512, 1000`, each
with `r = 1`, therefore returned `I2C bus error` in 2–3 ms. No other
target was present, so only the `w = 1` frontier row was obtained.
Whether the frontier is rectangular or diagonal was consequently not
observed. The rectangular interpretation is an inference from the
mechanism: a shared `w + r <= ~1015` budget is incompatible with the
4096-byte request reaching transaction initiation, and the measured
failure occurs while decoding the response whereas `w` travels in the
request.

No probe in any phase hung. This records that no hang was found, not
that none exists. Section 8's reset risk therefore remains unresolved:
`USBDEVFS_RESET` recovery was never exercised or validated on this
host.

The TMP102 state was unchanged across the sweep: CONFIG `0x60A0`, TLOW
`0x4B00` and THIGH `0x5000` both before and after.

Section 5's single-constant derivation rule is superseded. Taking one
minimum across the phases would conceal that the two directions differ
by roughly a factor of four and that Phase A found a request-framing
bound rather than a bus payload ceiling. It was replaced with separate
directional constants:

- `PDG_I2C_MAX_READ = 1014U`, the measured response edge.
- `PDG_I2C_MAX_WRITE = 4096U`, the largest wire-representable request
  observed to reach transaction initiation, not a discovered ceiling
  for clocking payload bytes to an acknowledging target.

Verification built and linked `pdg_i2c_burst` and
`pdg_mfd_m5/acceptance` for `native_sim/native/64`, executed
`pdg_fake/i2c` successfully (2/2), and built the mdBook cleanly. The
first two suites are `build_only`; this is compile-and-link evidence,
not behavioural evidence for the new limits against a real board.
