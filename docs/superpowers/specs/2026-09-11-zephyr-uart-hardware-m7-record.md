# Zephyr UART driver M7 hardware acceptance record

Date: 2026-09-11  
Branch: `issue-152`  
Issue: [#152](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/152)  
Campaign: 2026-09-11 20:38–21:01 UTC

## 1. Headline result

**The `uart_bridge` sample ran on real hardware — the first ever execution of
this driver against a board.** It returned all 27 transmitted bytes exactly:

```text
uart connected to pseudotty: /dev/pts/6
[00:00:00.000,000] <inf> uart_pico_de_gallo: uart: ready on Pico de Gallo serial-number "5256657D8A5D7F03" at 115200 baud (requested, not measured).
*** Booting Zephyr OS build v4.4.0-6123-g26f811ee9d0d ***
Sending 27 bytes
Polling for input, up to 64 attempts
Received 27 bytes: 48 65 6c 6c 6f 20 66 72 6f 6d 20 50 69 63 6f 20 64 65 20 47 61 6c 6c 6f 21 0d 0a

Stopped at 120.024s
```

`run_exit=124` is the correct bounded-run outcome, not a failure. The
`native_sim` process does not exit when `main()` returns; it idles until killed
and retains the board's USB claim. The converse trap also applies: the sample's
`No bytes received in 64 attempts` path exits 0. Acceptance must be based on the
bytes, never the process exit code.

The serial number was injected through a scratch overlay outside the repository
and confirmed in generated `zephyr.dts` before execution. The committed
`app.overlay` placeholder was not edited.

## 2. Rig and identity continuity

| Item | Value |
|---|---|
| Board serial | `5256657D8A5D7F03` |
| Firmware build ID | `firmware-v0.11.0-109-g6c4d42fd0796` |
| Firmware version | 0.12.0 |
| Schema version | 0.8.0 |
| Hardware revision | 2 |
| Runtime GPIO count | 4 |
| Capabilities | all seven present |
| UART wiring | TX GPIO 0 physically shorted to RX GPIO 1 |
| Additional equipment | no resistor, peer, analyser, debugger, or reflash access |

The build ID was read seven times and always matched the expected value:

| Gate | UTC | Result |
|---|---:|---|
| PHASE0-start | 20:39:34 | exact match |
| POST-PHASE1 | 20:44:24 | exact match |
| POST-PHASE2 | 20:47:19 | exact match |
| POST-PHASE3 | approximately 20:53 | exact match |
| POST-PHASE4 | 20:56:29 | exact match |
| POST-PHASE5 | 20:59:24 | exact match |
| FINAL | 21:00:53 | exact match |

There was no reset, USB re-enumeration, or dispatcher wedge during the
campaign.

## 3. Verdict scale and obligation merge

- **VERIFIED** requires positive, falsifiable evidence.
- **NOT VERIFIED** means the procedure ran and contradicted or failed to
  establish the claim.
- **INCONCLUSIVE** means part of the procedure ran, but the instrument or
  coverage was insufficient for the complete claim.
- **UNVERIFIABLE-ON-THIS-HARDWARE** means this rig cannot perform the required
  experiment; the missing equipment is named.

M5 §12.3's seven bullets do not add seven independent verdicts. They merge into
the M6 §8.3 obligations as follows:

| M5 §12.3 risk | M6 §8.3 obligation(s) |
|---|---|
| hw-rev2 ready and hw-rev1 refusal | 1, 2 |
| zero-timeout error consumption and recovery | 10, 11 |
| loopback polling/throughput and idle cost | 7, plus the sample headline |
| independent-peer framing distinctions | 8 |
| unplug/reset during operations and latch behaviour | 14, 16, 17 |
| timed-out TX/lost acknowledgement | 15 |
| configuration boundary after quiesce/drain | 13 |

## 4. M6 §8.3 obligation verdicts

### 1. hw-rev2 capability and ready child — VERIFIED

**Claim.** hw-rev2 advertises UART and the Zephyr child reaches ready state
after strict open.

**Procedure.** Inject the exact board serial through a scratch overlay, verify
it in generated `zephyr.dts`, start `uart_bridge`, and require the driver-ready
line before accepting payload output.

**Raw result.** The device reported hw revision 2 and all seven capabilities.
The driver printed `uart: ready ... at 115200 baud`, then returned the exact
27-byte greeting.

**Negative control.** Obligation 3 used a nonexistent serial and demonstrated
that strict open does not silently select the only attached board.

**Verdict: VERIFIED.** Readiness and real data flow were positively observed.

### 2. hw-rev1 capability refusal — UNVERIFIABLE-ON-THIS-HARDWARE

**Claim.** A hw-rev1 capability=false response leaves the child not ready and
does not drive TX.

**Procedure.** No valid procedure was available: the only board reports
hw-rev2 and UART capability present.

**Raw result.** None.

**Negative control.** M6's recording fake covers capability=false, but it is
not board-attached evidence and cannot show a physical TX line stayed idle.

**Verdict: UNVERIFIABLE-ON-THIS-HARDWARE.** Requires a v1.0 board and, for the
no-TX clause, an independent peer or logic analyser.

### 3. Strict-open failure matrix — INCONCLUSIVE

**Claim.** Exercise no-board, wrong-serial, schema-mismatch, and permission
failures, recording ordering and errno.

**Procedure.** Run strict open with selector `"0000000000000000"` while the
known board remains attached.

**Raw result.** Refusal arrived in 511 ms, named the exact requested selector,
and did not fall back to board `5256657D8A5D7F03`.

**Negative control.** The valid selector opened the same attached board and ran
the sample. No-board, schema-skew, and permission-denial arms were not run.

**Verdict: INCONCLUSIVE.** Wrong-serial hard-pin semantics are verified; the
four-case obligation is not. Completing it needs permitted detach, a deliberately
skewed firmware image, and a host permission/USB-access fault.

### 4. Capability-query failure and 300-second bound — UNVERIFIABLE-ON-THIS-HARDWARE

**Claim.** Inject a real capability-query failure and measure its advertised
worst-case path.

**Procedure.** Not run; ordinary hardware cannot selectively drop the second
`device/info` reply.

**Raw result.** None.

**Negative control.** Successful capability query and ready initialization were
observed under obligation 1, but success says nothing about the failure bound.

**Verdict: UNVERIFIABLE-ON-THIS-HARDWARE.** Requires a fault-injecting USB
proxy or console-controlled firmware fault injection.

### 5. Initial set-config failure — UNVERIFIABLE-ON-THIS-HARDWARE

**Claim.** Inject initial configuration failure and determine initialization
ordering and local/remote residue.

**Procedure.** Not run; the rig cannot fail only the initial `uart/set-config`
request.

**Raw result.** None.

**Negative control.** Successful initial 115200 8N1 configuration was observed.
That does not establish failure ordering.

**Verdict: UNVERIFIABLE-ON-THIS-HARDWARE.** Requires a fault-injecting USB
proxy or a firmware/console fault-injection hook with external TX observation.

### 6. Real FFI `Status` to errno mapping — INCONCLUSIVE

**Claim.** Verify representative endpoint, transport, timeout, and
invalid-argument mappings through the real FFI.

**Procedure.** Through the Zephyr driver, submit baud 0, data bits 9, and raw
data bits `0x7f`; separately provoke a UART RX overrun.

**Raw result.** Baud 0 returned `-EINVAL` (-22). Data bits 9 and `0x7f` each
returned `-ENOTSUP` (-95). Overflow surfaced `uart read failed /
Endpoint(Other)` exactly once in each of three trials, then recovered.

**Negative control.** Valid configurations returned 0. Two no-overflow trials
produced no endpoint error. No real transport or timeout status was injected.

**Verdict: INCONCLUSIVE.** Invalid-argument/capability distinctions and a real
endpoint error were observed; transport and timeout mappings remain owed.

### 7. Zero-timeout latency and lost-reply bound — INCONCLUSIVE

**Claim.** Measure idle `poll_in` latency and confirm the ordinary host bound
when its reply is lost.

**Procedure.** Exercise the zero-timeout polling driver during loopback and use
the established in-process idle measurement; do not disconnect the sole board.

**Raw result.** The zero-timeout path had previously measured approximately
359 us per empty poll on this same board/firmware lineage. M7 observed normal
polling and no wedge, but did not create a lost reply or drive the five-second
bound.

**Negative control.** The earlier 1 ms firmware-wait path measured about
1489 us. No lost-reply negative arm was safe on this rig.

**Verdict: INCONCLUSIVE.** The normal single-poll cost is measured; the
lost-reply bound is still derived, not observed. It needs a fault-injecting USB
proxy capable of dropping one selected reply.

### 8. Physical framing matrix — INCONCLUSIVE

**Claim.** Verify 5/6/7/8 data bits, all five parity modes, and one/two stop bits
with an independent peer or analyser.

**Procedure.** Use matched loopback, predict byte masks before measurement, and
time 64 bytes at 9600 baud under 8N1, 7N1, 8N2, and 8E1.

**Raw result.** Data widths returned: 8 → `ff 00 55 aa`; 7 →
`7f 00 55 2a`; 6 → `3f 00 15 2a`; 5 → `1f 00 15 0a`. The 64-byte timing
results were 8N1 72 ms, 7N1 65 ms, 8N2 78 ms, and 8E1 77 ms. A least-squares
fit gives slope 6.1818 ms per bit-per-character and intercept 9.6364 ms; the
physical slope is 6.6667 ms, for a 92.7% ratio. The two independent 11-bit
routes agree within 1 ms. A stale framing register predicts slope exactly 0.

`0x00` and `0x55` are fixed points under seven-bit masking and prove nothing
alone. `0xff` and `0xaa` are the discriminating bytes.

**Negative control.** The stale-register prediction was explicit before the
run and did not occur. For parity, 8E1/8O1/8M1/8S1 all looped back cleanly with
medians 511.5/511.1/411.5/514.3 ms; one PL011 driving both ends cannot
distinguish their parity values.

**Verdict: INCONCLUSIVE.** Word length and frame length are verified, but the
complete obligation requires an independent UART peer or logic analyser for
odd/even and mark/space parity. M4's parity ruling stands.

### 9. Requested versus achieved baud and lower clamp — VERIFIED

**Claim.** Characterize requested versus achieved baud, including the expected
approximately 143-baud floor.

**Procedure.** Request eight rates, send a 16-byte 8N1 block, measure the receive
window, and convert the calibrated window to achieved baud. Predict saturation
before interpreting the low-rate points.

**Raw result.** The campaign table was:

| Requested baud | Observed achieved baud |
|---:|---:|
| 300 | above the clamp; exact estimate not retained in the hand-off |
| 200 | above the clamp; exact estimate not retained in the hand-off |
| 150 | above the clamp; exact estimate not retained in the hand-off |
| 143 | above the converged floor; exact estimate not retained in the hand-off |
| 100 | approximately 147 |
| 75 | approximately 147 |
| 50 | approximately 147 |
| 1 | approximately 147 |

Convergence begins between requests 143 and 100. At a genuine 1 baud, 16 8N1
bytes need 160 seconds; the complete block arrived in 0.612 seconds. A separate
window-calibration calculation agreed with the approximately 147-baud estimate
within about 5%.

**Negative control.** The physical 1-baud prediction is 160 seconds, over 260
times the observed window. The unclamped hypothesis is decisively false.

**Verdict: VERIFIED.** The floor previously inferred from embassy-rp was
measured for the first time, at approximately 147 baud.

### 10. RX-error latch consumption and recovery — VERIFIED

**Claim.** Provoke a real RX error, surface it once, and prove later reads
resume without reboot.

**Procedure.** Use RX-ring overrun rather than reframing: issue five successive
256-byte writes, 1280 bytes total, with no intervening read against the
firmware's 1024-byte RX ring. Repeat three times, then send and receive a fresh
64-byte indexed pattern.

**Raw result.** All 3/3 overflow trials surfaced `uart read failed /
Endpoint(Other)` exactly once and then recovered. Each drained 1056 of 1280
bytes. There were exactly four discontinuities, all at write boundaries; writes
0–3, exactly 1024 bytes, arrived complete and ordered, while write 4 was
tail-truncated. The post-recovery 64-byte indexed pattern was byte-exact.

**Negative control.** Both 2/2 no-overflow trials completed without an error.
M4's three bounded reframing attempts had produced zero errors because
`apply_framing` disables UARTEN/TXE/RXE together; that failed induction is why
M7 used overrun.

**Verdict: VERIFIED.** The externally visible consequences of latch consumption
and RX recovery are measured. Embassy's private `rx_error` field and RX
interrupt-enable bits were not observed; proving that internal mechanism needs
RTT or a debugger. The mechanism remains inferred, and no stronger claim is
made.

### 11. RX backpressure and ring overflow — VERIFIED

**Claim.** Stress the 1024-byte firmware RX ring through overflow and recovery.

**Procedure.** The same five-by-256-byte campaign used for obligation 10,
including byte-index analysis and a post-error nonce.

**Raw result.** The first four writes filled exactly 1024 bytes and arrived
complete and ordered. Only the fifth write was truncated. Four discontinuities
landed exactly at write boundaries; there was no mid-stream loss or reordering.
Recovery succeeded in all three trials.

**Negative control.** Two control trials stayed below overflow and produced no
error.

**Verdict: VERIFIED.** This is the expected signature of ring overrun rather
than arbitrary transport corruption.

### 12. TX backpressure and TX-ring saturation — UNVERIFIABLE-ON-THIS-HARDWARE

**Claim.** Fill the 1024-byte TX ring and characterize acknowledgement, drain,
and overflow.

**Procedure.** Deliberately not run. A large low-baud write can exceed the
firmware supervisor budget: `uart/write` and `uart/flush` declare 60 seconds
plus 30 seconds slack, so reset is expected at approximately 90 seconds.

**Raw result.** None.

**Negative control.** Short bounded transmissions succeeded, but do not stress
the ring.

**Verdict: UNVERIFIABLE-ON-THIS-HARDWARE.** The only board could not be
recovered without a BOOTSEL press. This requires permitted reset/reflash access,
preferably plus an independent peer or analyser.

### 13. Reconfiguration with queued RX residue — INCONCLUSIVE

**Claim.** Reconfigure after queued RX data and determine whether bytes decoded
under the old framing remain in the firmware ring.

**Procedure.** The harness quiesced and drained between configuration changes,
then configured 7N1 and restored 8N1, checking both the cached configuration and
freshly transmitted probe bytes.

**Raw result.** Baseline was 115200 8N1. Configure 7N1 returned 0 and produced
`7f 00 55 2a`; restore 8N1 returned 0 and produced `ff 00 55 aa`.

**Negative control.** The differing 7-bit and 8-bit masks prove that fresh bytes
used the requested configurations. No arm deliberately left queued old-framing
bytes in the firmware ring.

**Verdict: INCONCLUSIVE.** The safe quiesce/drain procedure works, but firmware
residue across an unsafe boundary was not characterized. Completing it needs an
independent sender with precisely gated traffic, or analyser-triggered timing.

### 14. Unplug, re-enumeration, reset, and process generations — UNVERIFIABLE-ON-THIS-HARDWARE

**Claim.** Exercise detach and reset, then identify which operations and local
latches recover in later generations.

**Procedure.** Not run; unplug, `usbipd`, re-enumeration, reset, and flash
operations were prohibited.

**Raw result.** Seven identity gates remained stable and no spontaneous reset or
re-enumeration occurred.

**Negative control.** Identity continuity proves the positive campaign stayed
on one image; it does not exercise generation changes.

**Verdict: UNVERIFIABLE-ON-THIS-HARDWARE.** Requires permitted cable/USB
unbind-rebind control or a fault-injecting USB proxy, plus recovery access.

### 15. Partial remote effect and lost acknowledgement — UNVERIFIABLE-ON-THIS-HARDWARE

**Claim.** Lose an acknowledgement after an externally observed byte or
configuration effect and prove the driver does not retry it.

**Procedure.** Not run; the rig cannot drop one selected reply while retaining
external observation of the wire.

**Raw result.** None.

**Negative control.** Normal writes were not duplicated. That is not evidence
for the lost-ack path.

**Verdict: UNVERIFIABLE-ON-THIS-HARDWARE.** Requires a fault-injecting USB
proxy and an independent UART peer or logic analyser.

### 16. Two-thread mutex contention and post-lock latch check — UNVERIFIABLE-ON-THIS-HARDWARE

**Claim.** Queue at least two Zephyr callers, inject a transport latch, and show
the waiter makes no later bottom call.

**Procedure.** Not run; there was no safe way to hold one real USB call while
selectively failing it and observing the second call.

**Raw result.** None.

**Negative control.** M6 proves bounded immediate suppression with recording
fakes, not a genuinely queued real caller.

**Verdict: UNVERIFIABLE-ON-THIS-HARDWARE.** Requires a fault-injecting USB
proxy or console fault injection, plus an instrumented call counter.

### 17. Active console faults and direct pre-kernel refusal — UNVERIFIABLE-ON-THIS-HARDWARE

**Claim.** Make PDG UART the active non-early console, fault output, prove no
recursive logging/deadlock, and exercise direct pre-kernel refusal where
feasible.

**Procedure.** Not run; making the sole board's UART the console while injecting
transport faults risks retaining the USB claim or wedging the only recovery
path.

**Raw result.** None.

**Negative control.** Source and M6 fake evidence show `poll_out` does not log,
but neither executes the real recursive-console failure.

**Verdict: UNVERIFIABLE-ON-THIS-HARDWARE.** Requires console-selectable fault
injection and permitted process/reset recovery; direct pre-kernel execution may
also require a purpose-built Zephyr harness.

### 18. Runtime ownership and buffer audit — UNVERIFIABLE-ON-THIS-HARDWARE

**Claim.** Use runtime tooling across open/use/reset/teardown to validate
host/embedded buffer separation, borrowed opaque-context lifetime, and Rust
ownership.

**Procedure.** Normal open/use/termination ran, but no sanitizer, debugger,
USB-fault proxy, reset generation, or ownership instrumentation was available.

**Raw result.** The sample and harness transferred correct data without a crash;
the process retained the board claim until killed.

**Negative control.** M6's strong fake checks symbol substitution and local
state, but bypasses Rust, USB, and the opaque real context.

**Verdict: UNVERIFIABLE-ON-THIS-HARDWARE.** Requires native runtime
instrumentation such as ASan/Valgrind where applicable, a fault-injecting USB
proxy, and permitted reset/re-enumeration.

The two-image `REQ_KEY` A/B is no longer owed. It was performed on
2026-09-11 with two BOOTSEL presses and is recorded in section 6.

## 5. Findings — not fixed in M7

### F1 — `gallo uart read -c N -t T` returns short before timeout

This is the campaign's most important new defect. `read -c 64 -t 5000`
returned 63 bytes in 206–306 ms; an immediate follow-up read retrieved the 64th
byte, so it was not lost. The reproduction rate was 7/100 at 115200 with
64-byte payloads. At low baud, reads returned after about 306 ms with only
4–12 of 16 bytes despite a 12,000 ms timeout.

Any caller treating `count` as “collect this many bytes within `timeout`” can
silently mis-frame. The Zephyr driver is not exposed because it polls one byte
at a time. The CLI, `pico-de-gallo-lib`, C FFI, and Python surfaces are exposed.
M7 did not patch the contradiction; file a separate issue. Three earlier
`rx=3` observations probably have the same cause, but their raw text was not
captured, so that magnitude remains unexplained rather than reconstructed.

### F2 — `native_sim` elapsed time is simulated

`CONFIG_NATIVE_SIM_SLOWDOWN_TO_REAL_TIME=y` was enabled. Host work costs zero
simulated milliseconds, explaining `tx_ms=0` across 64 USB round trips. Drain
time advances through the 2 ms polling `k_sleep`. The physical frame-length
slope remains meaningful and reached 92.7% of prediction; absolute milliseconds
must not be presented as wall-clock time, and the measurement quantum is 2 ms.

### F3 — separate CLI processes cannot measure wire timing

Every CLI timing measurement landed near 411/511/612 ms, dominated by roughly
100 ms process quantization. The decisive control held framing and payload
constant and changed only baud: 9600 should have taken 61.11 ms longer than
115200, but measured 99.88 ms shorter, the wrong sign. `write` and `read` run in
separate processes 200–300 ms apart, so the line drains before `read` begins.
This is an instrument limitation, not a driver fault. Future timing work must
keep write and read in one process.

### F4 — an RX error costs exactly one byte

In both nonce trials, `de ad be ef` returned as `ad be ef`. The leading `de` was
consumed with the error. The result was deterministic: an error surfaces instead
of, not alongside, the byte in flight.

### F5 — `native_sim` binaries retain the board after `main()`

The simulator idles rather than exits and retains the exclusive USB claim until
killed. Running the sample without a bound blocks every other tool from the only
board. This is an operational hazard, not a sample failure.

### F6 — stale LSP diagnostics are not a finding

Two agents independently saw editor diagnostics in
`crates/pico-de-gallo-lib/src/lib.rs` and `crates/pyco-de-gallo/src/lib.rs`.
They did not reproduce: `gallo` and the FFI static library compiled cleanly
throughout. This was a stale index and is recorded only to prevent a false alarm.

## 6. Method traps to retain in the regression register

1. `gallo version` renders a `tabled` box with Unicode `│` (U+2502), not only
   ASCII `|`. An identity gate must accept both and report parser failure
   separately from firmware mismatch. A broken parser must neither halt a
   healthy campaign nor pass a mismatched image.
2. PowerShell expands `$VAR` inside inline `wsl ... bash -c '...'` commands.
   `$REPO` arrived empty, while `cd "$REPO"` appeared to succeed only because
   WSL already had the repository as its current directory; the same mistake
   emitted a bogus `exit=127` beside correct output. Put the logic in a `.sh`
   file and do not put shell `$` expressions in a PowerShell-level string.
3. Loopback alone cannot prove framing because TX and RX share one PL011. Use
   data-width masking and frame-length timing. `0x00` and `0x55` are fixed
   points; `0xff` and `0xaa` discriminate.
4. For `native_sim`, exit 0 is not a pass and nonzero is not necessarily a
   failure. Judge the payload.

## 7. Residual obligations

Verdict tally across the 18 M6 obligations: **4 VERIFIED, 0 NOT VERIFIED,
5 INCONCLUSIVE, 9 UNVERIFIABLE-ON-THIS-HARDWARE.**

Still owed:

- obligation 2: a hw-rev1 board and external TX observation;
- obligation 3: no-board, schema-skew, and permission-denial strict-open arms;
- obligations 4–7 and 14–17: selective USB/console fault injection, permitted
  detach/re-enumeration/reset, and external wire observation where applicable;
- obligation 8: independent parity-capable peer or logic analyser;
- obligation 12: recovery/reflash access before deliberately saturating TX;
- obligation 13: a precisely gated independent sender for queued old-framing
  residue;
- obligation 18: sanitizer/debugger and ownership instrumentation across real
  reset generations;
- (the two-image `REQ_KEY` A/B is CLOSED - performed 2026-09-11, section 6)

File a separate issue for F1. It affects four supported host surfaces and was
not fixed under the M7 evidence-only rule.

## 6. The two-image REQ_KEY A/B - CLOSED 2026-09-11

Owed since M1 and closed after M7 with two BOOTSEL presses.

**Setup.** Board `5256657D8A5D7F03` flashed with firmware built from
`main@b6c209153df0`, whose `UartSetConfigurationRequest` carries one field.
Host built from `issue-152`, whose request carries four. **Both images report
schema 0.8.0**, because the unreleased bump was already on `main`, so
`validate()` cannot distinguish them - the blind spot reproduced deliberately.

**Result.**

| Endpoint | Shape change | Observed |
|---|---|---|
| `ping`, `i2c scan`, `adc info`, `spi get-config`, `i2c get-config` | none | all succeed |
| `uart read`, `uart write`, `uart flush` | none | all succeed |
| `uart set-config` | `REQ_KEY` moved | `Comms(Wire(UnknownKey))` |
| `uart get-config` | `RESP_KEY` moved | `Timeout { waited: 5s }` |

**Verdict: VERIFIED.** Eight untouched endpoints establish board health; only
the two whose wire shapes moved fail. The dangerous outcome - the old handler
decoding `baud_rate`, ignoring the three trailing framing bytes, and returning
`Ok` - did not occur.

The design's asymmetry claim is now hardware-proven rather than reasoned:
`set-config` fails loudly and names the unknown key; `get-config`, whose
request is `()` so its `REQ_KEY` never moves, fails as an opaque timeout
indistinguishable from a dead board.

**Method note.** The first negative control chosen was `uart get-config`, which
was never a valid control: its request is unchanged but its response type
gained fields, making it a second experiment wearing a control's hat. It was
caught because it failed where a control must succeed, and replaced with five
endpoints this branch never touched.

**Restoration.** The board was reflashed to
`firmware-v0.11.0-109-g6c4d42fd0796` and re-verified: both endpoints work,
7E2 loopback returns the predicted 7-bit-masked `7f 00 55 2a`, 8N1 returns
unmasked `ff 00 55 aa`, config restored to 115200 8N1, line drained, ping OK.