# Zephyr UART recording-fake suite M6 specification

Date: 2026-09-11  
Branch baseline: `issue-152` at `21c87e792a2c`  
Milestone: M6 of 7 — sample, executed recording-fake suite, and CI wiring  
Status: implemented and verified; reconciled with the delivered M6 implementation

## 1. Context, outcome, and scope

M5 added the polling `odp,pico-de-gallo-uart` driver and proved only that it compiles and links. M6 adds two consumers:

- `zephyr/samples/uart_bridge/` is a board-attached bounded polling example. Twister builds but never runs it.
- `zephyr/tests/pdg_fake/uart/` is hardware-free. Strong host-context fakes replace the UART topology's bottom functions, and Twister executes two policy scenarios.

Standing constraints:

- **NEVER bump any `[package].version`.**
- **Nothing under `crates/` changes.**
- **The driver's behaviour must not change.** A discovered driver bug is a finding to escalate, never an M6 fix.
- Every test loop is bounded by a fixed compile-time cap and fails cleanly when exhausted.
- All text remains LF; run `dos2unix` on new or edited text files when needed.
- No book chapter is added. The `zephyr/` carve-out in AGENTS.md §15.1 makes `zephyr/README.md` and `zephyr/CHANGELOG.md` authoritative.

M6 changes no wire type, FFI, firmware, Cargo file, release version, or production UART behaviour. The only production-driver-area change is a comment correction in `pdg_uart_bottom.h`; `pdg_uart.c` is not modified.

## 2. Source-verified findings and scope rulings

1. **Private diagnostics stay private.** The exact `tx_dropped`, `rx_errors`, `last_errno`, and `link_failed` values are not needed to prove the public contract. No `CONFIG_ZTEST` accessor and no test-only production header are added.
2. **Pre-kernel guards are not runtime-testable here.** Ztest runs after kernel initialization. `irq_offload()` supplies real ISR context on `native_sim/native/64`; it cannot restore pre-kernel state.
3. **`pdg_uart_bottom.h` has stale prose.** The driver passes timeout 0, and `uart_read_bound()` applies the ordinary call bound; the comment still describes 1 ms and a 30-minute zero case. Correct the comment only.
4. **Initialization has two UART bottom calls.** Every enabled child calls `pdg_uart_bottom_has_uart()` and, after a positive result and validation, `pdg_uart_bottom_set_config()`.
5. **`ROADMAP.md` is stale but out of M6 scope.** It still omits the delivered Zephyr UART and describes I2C as the sole executed fake. Record and escalate this finding; do not edit the roadmap in M6.
6. **Workflow prose becomes false after M6.** `.github/workflows/zephyr.yml` may receive comments-only corrections to “BUILD ONLY”, “nothing executes”, and the `tests.yaml` count. No workflow logic changes: its existing `--testsuite-root` arguments already discover the new sample and suite.

### 2.1 Kconfig facts

`CONFIG_IRQ_OFFLOAD` depends on `TEST`; `CONFIG_ZTEST=y` supplies the test context. The pinned POSIX architecture implements `arch_irq_offload`, so `irq_offload()` synchronously invokes the callback with `k_is_in_isr()` true.

The full scenario enables `CONFIG_UART_USE_RUNTIME_CONFIGURE=y`, `CONFIG_UART_ASYNC_API=y`, `CONFIG_UART_WIDE_DATA=y`, and `CONFIG_IRQ_OFFLOAD=y`, ensuring all explicit PDG callback slots exist. **Do not enable `CONFIG_ZTEST_SHUFFLE`.** RX/TX transport-latch tests consume dedicated instances and no test hook re-arms them; shuffled execution would invalidate ownership assumptions rather than test independence. The driver source makes the latch device-lifetime state, but the runtime evidence is deliberately narrower (§8.1).

## 3. Fake boundary and link enforcement

### 3.1 Mechanically derived override set

Derive, rather than copy, the required set:

1. Parse all externally declared `pdg_uart_bottom_*` functions from `zephyr/drivers/serial/pdg_uart_bottom.h`.
2. Parse all externally declared `pdg_common_bottom_*` functions from `zephyr/drivers/common/common_bottom.h`.
3. Search `pdg_uart.c` and `pdg_mfd.c` to classify reachability.

The resulting six-symbol declaration/gate set is:

| Symbol | Role |
|---|---|
| `pdg_common_bottom_open` | Reachable: unconditional MFD parent initialization. |
| `pdg_common_bottom_close` | Not reachable in this topology. The shared fake defines it defensively so an ownership regression is countable, but the verified executable discards it as unreferenced. |
| `pdg_uart_bottom_has_uart` | Reachable: unconditional per-child capability probe. |
| `pdg_uart_bottom_set_config` | Reachable: initial and accepted runtime configuration. |
| `pdg_uart_bottom_read` | Reachable: empty-ring polling refill. |
| `pdg_uart_bottom_write` | Reachable: healthy thread-context polling output. |

Five symbols are genuinely reachable. The six-symbol gate set is intentionally **not** called an exhaustive reachable set: `close` remains a declared contract member and has a defensive fake definition, but is absent from the verified linked executable because no call site references it.

The UART fake includes `pdg_uart_bottom.h`, supplies all four UART definitions, and ignores rather than dereferences `ctx`. The shared parent fake supplies and records open/close. `uart_config_get`, `uart_err_check`, async/wide stubs, and `poll_out_u16` make no bottom call; the UART bottom contract declares no readiness, open, close, flush, or get-config function.

### 3.2 Link and include mechanism

Mirror the existing I2C fake mechanism:

- add `../common/pdg_fake_bottom.c` and the UART fake to `native_simulator INTERFACE` with `target_sources`;
- include production declaration headers so signature drift fails compilation;
- provide native header paths using `target_compile_options(native_simulator INTERFACE "-I${DIR}")`, not `target_include_directories`, because native-simulator consumes `INTERFACE_COMPILE_OPTIONS`.

Strong-over-weak ELF resolution alone is insufficient: omitting an override silently selects the weak production definition.

### 3.3 Post-link symbol gate — implemented, with a non-obvious attach point

`verify_overrides.cmake` reads the two declaration headers named in §3.1, derives the six bottom-function names with a declaration regex, and runs `${CMAKE_NM}` (falling back to `nm`) with `-g --defined-only` on the final native-simulator executable. It classifies each declaration by binding:

- **absent is safe**: the symbol is unreferenced and was discarded, so the dangerous production cast cannot execute;
- uppercase **`T` is correct**: the strong fake definition won;
- **weak is fatal**: a `W`/`V`/lowercase weak binding means the production definition survived and could cast the fake token to `PicoDeGallo *`;
- an unexpected binding is fatal.

The script also refuses an all-absent or stripped-symbol-table result, because absence is evidence only when the symbol table is usable and at least one expected strong override is visible. Tooling or input failures remain explicit best-effort warnings, never silent passes. `pdg_common_bottom_close` is genuinely absent in this topology because no code references it; the earlier “every declaration must be `T`” rule was a false-positive requirement.

The obvious CMake form was tried twice and silently did nothing: `add_custom_command(TARGET zephyr_final POST_BUILD ...)` is invalid for this need because `zephyr_final` is created in a different directory scope, while target-form custom commands are supported only for targets created in the current scope. It also observes the wrong stage: `zephyr_final` produces `zephyr.elf`; the native-simulator Makefile creates `zephyr.exe` later through `native_runner_executable` (`${ZEPHYR_BASE}/CMakeLists.txt:2098-2114`).

The delivered mechanism therefore owns its attachment point: `CMakeLists.txt` creates `pdg_verify_overrides` with `add_custom_target(... ALL)` in the suite's directory and orders it after `native_runner_executable` with `add_dependencies(...)`. It passes the executable and absolute header paths via `-D` arguments. This runs under both Twister and the `ci-build.sh` `uart_fake` build and prints a positive summary on every effective invocation.

### 3.4 Recorder and policy contract

The FFI-free fake accessors expose bounded records for:

- separately latched parent open and close counts from the shared fake;
- capability, set-config, read, and write call counts from the UART fake;
- one global bounded UART-only event sequence covering capability, set-config, read, and write; parent open/close are not events in this sequence;
- read count/timeout, ordered write bytes/lengths, and set-config scalar histories;
- scripted RX bytes up to `PDG_UART_RX_BUFFER_SIZE`;
- independently programmable read/write/set-config returns and reported read length, including 1015.

Suite setup runs after device initialization and freezes the UART capability/config counts, UART event sequence, and initial configuration scalars into immutable boot-snapshot storage **before any test case runs**. Parent open/close counters are not copied into that snapshot: they live in the shared fake and are independently latched across resets. Because every child receives the same borrowed parent context and `HAS_UART` carries no discriminator, capability events cannot be assigned to a child. Distinct devicetree baud rates do attribute each frozen set-config record to one child.

A separate runtime recorder holds per-test scripts/events. Before each test, reset only that runtime recorder and take the test's own counter snapshot. Every test asserts deltas from that snapshot except test 1, which asserts the immutable boot snapshot. Recorder overflow is a test failure, never silent truncation.

A ztest per-test `before` hook clears the UART runtime recorders and scripts. The `after` hook restores the reusable general and configuration-failure instances to their devicetree baseline configuration after first restoring successful fake return values. Restoration is not placed in test bodies, because teardown must run after a failed assertion. Dedicated transport-latch instances are never restored or reused.

## 4. Scenarios, topology, and isolation

One source tree defines two Twister scenarios selected by validated CMake cache variable `PDG_FAKE_UART_CAPABILITY`:

- `1` (default): healthy runtime suite;
- `0`: capability refusal and failed-init direct dispatch.

The fake reads the compile-time value during POST_KERNEL initialization; a ztest setter would be too late.

The overlay enables one fake parent and **five** UART children:

| Instance | Sole owning test(s) / purpose |
|---|---|
| `uart_general` | Tests 1, 4–6, 10, 12, 13, and 15; baseline restored by `after`. |
| `uart_backoff` | Tests 7 and 8 only; test 7 may leave an active simulated-time backoff. |
| `uart_rx_latch` | Test 9 only; four immediate follow-up iterations observe suppression after the injected latch. |
| `uart_tx_latch` | Test 11 only; four immediate follow-up iterations observe suppression after the injected latch. |
| `uart_config_failure` | Test 14 only; baseline restored by `after`, even after assertion failure. |

Test 2 and test 3 use the capability=0 binary's whole topology; no later healthy tests run in that process. Test 1 observes initialization for all five healthy children. GPIO/I2C/SPI siblings are disabled.

The five-instance design prevents simulated-time backoff, latched transport state, and failed-config cache state from leaking into unrelated tests. It does not make arbitrary shuffled order safe; `CONFIG_ZTEST_SHUFFLE` remains disabled because the suite has no supported way to re-arm latch devices.

## 5. Sample specification

Create `zephyr/samples/uart_bridge/` following the I2C bridge conventions:

- one enabled parent with an explicit placeholder serial and one UART at 115200 8N1;
- require device readiness;
- send one fixed greeting with a compile-time-bounded `uart_poll_out()` loop;
- perform at most 64 `uart_poll_in()` attempts, echoing received bytes;
- terminate cleanly;
- mark its native scenario `build_only: true`; running reaches strict open and USB.

## 6. Runtime test catalogue

All runtime assertions use their test-start recorder delta unless explicitly identified as frozen boot evidence. Every loop has a fixed cap and clean failure. The fake tests only top-half parameters, buffering, and local state transitions; firmware, host timeout policy, USB, and physical wire behaviour do not execute.

### 6.1 Initialization and failed readiness

1. **Strong override and init sequence.** Against the immutable UART boot snapshot and separately latched parent counters, assert the parent fake opened at least once, zero closes, exactly one capability probe and one initial set-config per enabled healthy child, exact UART event counts, aggregate prefix ordering (`probes >= configs` at every set-config event), and each child's expected initial baud/8N1 scalars in the frozen config log. Assert all children ready. This does **not** prove capability-before-config ordering for each individual child: `HAS_UART` has no child discriminator. That narrower per-child ordering remains source-review evidence. The §3.3 gate independently proves that every referenced bottom definition is strong and that an unreferenced declaration may safely be absent.
2. **Capability refusal.** With capability=0, assert every child not ready, capability-probe count equals **exactly** the number of enabled UART children, and set-config/read/write counts are exactly zero. **Fails if** refusal is ignored or any later bottom operation runs.
3. **NULL-context direct dispatch.** In capability=0, invoke poll-in/out, err-check, configure/get, all byte/wide async stubs, and poll-out-u16 despite failed readiness. Integer returns match documented `-1`/`-ENODEV`; both void calls return safely; read/write/set-config deltas are all zero and close remains zero. **Fails if** a failed-init callback dereferences context, crashes, or reaches a bottom.

### 6.2 Context guards

4. **ISR refusal matrix.** Invoke each supported callback shape through bounded `irq_offload()` cases. Outside ISR, assert poll-in `-1`, integer callbacks `-EWOULDBLOCK`, void callbacks return synchronously, and capability/set-config/read/write/close deltas are zero. This proves documented ISR results and no bottom call. It **does not prove the guard precedes the mutex**: an uncontended lock immediately before the guard is behaviourally indistinguishable here, and deliberate contention risks deadlocking single-threaded ztest. Guard-before-mutex remains source-review evidence only.

Pre-kernel guards are not claimed as executed coverage.

### 6.3 Receive buffering and errors

5. **Exact empty result.** Script success with zero bytes, seed output `0xCC`, call once, and assert `-1`, unchanged output, one read with count 1014 and timeout 0, and no other bottom-call delta.
6. **One refill serves many polls.** Script eight distinct bytes. Eight polls return them in order with exactly one read at count 1014/timeout 0; a ninth scripted-empty poll makes exactly one further read and returns -1.
7. **Implausible length.** On `uart_backoff`, report 1015 without overrunning the supplied buffer. Assert `poll_in == -1`, output unchanged, and exactly one read. Immediate retries return -1, leave output unchanged, and make no read. Sleep 11 ms using Zephyr simulated uptime, script one valid byte, and require the post-deadline retry to reach the fake and succeed. This required recovery arm distinguishes bounded backoff from an incorrect permanent latch.
8. **Non-transport backoff and recovery.** On `uart_backoff`, independently script a fresh `-EIO` result; four immediate bounded polls make no further read. After 11 ms, script one byte and require success plus exactly one new read. This proves suppression before the deadline and retry after it. It does **not** prove that line 574 explicitly clears `rx_backoff_until`: after expiry, retaining the expired timestamp is behaviourally identical.
9. **RX transport latch.** On `uart_rx_latch`, inject `-ECOMM` from an empty refill. Assert `poll_in == -1`; later `uart_err_check()` and `uart_configure()` both return `-EIO`; four immediate follow-up poll-in/poll-out/configure iterations produce zero read/write/set-config deltas. This proves bounded local suppression after an injected errno, not device-lifetime permanence or real reconnect generations.

### 6.4 Transmit, configuration, and stubs

10. **One write per byte.** Emit A/B/C and assert exactly three fake writes, each length one, in global-event and payload order. This proves top-half call cardinality/order only, not remote queueing or wire transmission.
11. **TX transport latch and staged-ring discard.** On `uart_tx_latch`, stage two RX bytes and consume one; inject `-ETIMEDOUT` on one output. The next `poll_in` returns `-1`, not the staged byte, and makes no read. Later err-check/configure return `-EIO`; four immediate follow-up poll-in/poll-out/configure iterations make no bottom calls. This proves local ring discard and bounded suppression after an injected errno—not device-lifetime permanence, a genuinely lost ACK, or a reset generation.
12. **Configuration transcription matrix.** Vary one field at a time through data 5/6/7/8, parity none/odd/even/mark/space, and stop 1/2. Each accepted call records one exact baud/data/parity/stop neutral tuple. The `after` hook restores baseline. This proves Zephyr→neutral scalar transcription; neutral→FFI correspondence remains `_Static_assert` evidence, and physical framing belongs to M7.
13. **Rejected configurations are side-effect-free.** Exercise data=9, stop=0.5/1.5, RTS/CTS, DTR/DSR, RS485, and baud=0. Assert the documented errno, no set-config delta, and unchanged cached config.
14. **Cache follows acknowledged success only.** On `uart_config_failure`, successfully set A and read A. Script set-config `-EIO`, request B, assert failure and config-get still A. Pre-stage two RX bytes before failure and require the remaining staged byte without another read. Phrase the evidence precisely: **config-get causes no bottom-call delta**. Teardown restores the baseline even if an assertion aborts the body.
15. **Async and wide stubs.** Invoke byte and wide async stubs plus `uart_poll_out_u16()`. Integer stubs return `-ENOTSUP`; `uart_poll_out_u16()` returns safely and records no write; all bottom deltas remain zero. NULL-slot callback-set and poll-in-u16 return `-ENOSYS` rather than crash.

## 7. Invariants and failure modes

1. For each declared bottom symbol, the post-link gate accepts either a strong fake `T` or genuine absence due to no reference, and rejects any weak production binding. On the verified topology, five are strong and `pdg_common_bottom_close` is absent.
2. Init capability policy is compile-time selected; ztest setup is too late.
3. Production diagnostics remain private; M6 adds no production introspection.
4. Successful refill preserves order and amortizes one bottom read across staged bytes.
5. `poll_in` exposes only 0/-1 and writes output only on 0.
6. Injected transport-class errors suppress bottom calls across four immediate follow-up iterations; source review, not this bounded run, establishes device-lifetime latch permanence. Endpoint errors back off and recover after the deadline.
7. Rejected or non-transport-failed configuration preserves the acknowledged cache and staging. Successful reconfiguration clears staging, and transport-class failures also discard staging as a generation boundary.
8. ISR-context calls return their documented values and make no bottom calls. Guard-before-mutex ordering is not proved dynamically; it is source-review evidence only.
9. Fake scripts, records, and loop iteration counts are bounded. A single callback may still block forever on the driver's `K_FOREVER` mutex; the explicit Twister process timeout is that containment.
10. The sample and `ci-build.sh` remain build-only. Only fake Twister scenarios execute.
11. Children borrow the parent context and never close it; the close counter stays zero.

Missing an override can touch USB or crash. A wrong capability default fails every child. Shared latched or time-based state causes order dependence; dedicated instances and per-test teardown constrain it. Simulated time does not advance merely because the CPU executes assertions.

## 8. Unobservability, timeout containment, and M7 hand-off

### 8.1 Explicitly unobserved in M6

The exact values of `tx_dropped`, `rx_errors`, `last_errno`, and the internal `link_failed` flag are **not observed by the M6 suite**, because observing them would require widening the production driver. Their effects are observed behaviourally instead.

M6 also does not establish:

- per-child capability-before-configuration ordering; the suite proves only aggregate running-prefix ordering because `HAS_UART` events have no child discriminator;
- device-lifetime permanence of RX/TX transport latches; runtime coverage proves suppression across four immediate follow-up iterations, while permanence is source-review evidence;
- that a successful refill explicitly clears backoff rather than leaving an expired timestamp;
- guard-before-mutex ordering, mutex contention, queued-caller semantics, or the pre-lock/post-lock latch checks under a genuinely waiting thread;
- recursive-console safety: the suite does not make PDG UART the active console, so “poll_out never logs” remains static-review evidence;
- strict MFD open semantics: the parent fake ignores the serial and performs no USB open or schema validation;
- real init-failure ordering for no board, schema mismatch, permissions, the 300-second capability-query bound, a `has_uart` error return, or an initial set-config error;
- FFI `Status`→errno mapping, because the fake directly injects `-EIO`, `-ECOMM`, and `-ETIMEDOUT`;
- partial/indeterminate remote effects such as “byte applied, acknowledgement lost”;
- host/embedded memory separation, opaque-pointer lifetime, or Rust ownership correctness;
- firmware-ring residue after successful reconfiguration;
- real backpressure, firmware RX/TX ring overflow, interrupt drain/fill, or USB concurrency;
- real USB enumeration, detach/reconnect, reset generations, scheduling, latency, host timeout policy, or lost replies;
- physical polarity, baud, word length, parity, stop bits, noise, peer compatibility, achieved-baud clamp, or framing;
- `k_is_pre_kernel()` behaviour;
- real impossibility of a read result over 1014; only local defensive handling is tested.

### 8.2 Timeout containment

Both fake scenarios set `timeout: 10` explicitly in `tests.yaml`. Twister's default is 60 seconds, but the synchronous fake should finish well below ten. Loop caps contain accidental iteration; they cannot contain a callback blocked in `k_mutex_lock(..., K_FOREVER)`. Twister process termination at ten seconds is the containment for a defective unlock or blocked callback.

### 8.3 M7 hand-off: required board-attached evidence

Each obligation below requires a record containing firmware build ID, board serial, hardware revision, peer/tooling and wiring, exact procedure, and pass/fail result. Claims without that evidence remain unverified.

1. Verify hw-rev2 advertises UART and the Zephyr child reaches ready state after strict open.
2. Verify hw-rev1 capability=false leaves the UART child not ready without driving TX.
3. Exercise strict-open failures for no board, wrong serial, schema mismatch, and host permission denial; record ordering and errno.
4. Inject a real capability-query failure and bound/measure the advertised worst-case 300-second path.
5. Inject initial set-config failure and determine observable local/remote state and initialization ordering.
6. Verify real FFI `Status`→errno mapping for representative endpoint, transport, timeout, and invalid-argument statuses.
7. Measure zero-timeout idle `poll_in` latency and confirm the ordinary host call bound on a lost reply.
8. Verify physical 5/6/7/8-bit, none/odd/even/mark/space parity, and 1/2-stop-bit framing with an independent analyser/peer.
9. Characterize requested versus achieved baud, including the approximately 143-baud lower clamp.
10. Inject framing, parity, and overrun errors; prove Embassy consumes the latch and RX interrupts recover.
11. Stress RX backpressure and the firmware 1024-byte RX ring through overflow and recovery.
12. Stress TX backpressure and the firmware 1024-byte TX ring through fill, acknowledgement, drain, and overflow behaviour.
13. Reconfigure after queued RX data and determine whether old-framing residue remains in the firmware ring.
14. Exercise unplug, USB re-enumeration, firmware reset, and subsequent process generations; record which operations recover and which local instances remain latched.
15. Model a partial remote effect/lost acknowledgement with external observation; prove no retry duplicates an indeterminately applied byte/configuration.
16. Use at least two Zephyr threads to contend the mutex and verify queued callers perform the post-lock latch check without a later bottom call.
17. Make PDG UART the active non-early console and fault output paths; prove no recursive logging/deadlock, then separately exercise direct pre-kernel callback refusal where feasible.
18. Audit with runtime tooling appropriate to native_sim plus board runs that host/embedded buffers, borrowed opaque context lifetime, and Rust ownership remain valid across open/use/reset/teardown.

## 9. Scenario table

| Scenario | Covers | `build_only` | Timeout | Overlay / extra args |
|---|---|---:|---:|---|
| `drivers.pico_de_gallo.uart.fake` | Healthy init, ISR, buffering, TX, config, stubs, backoff, local latches | no | 10 s | `fake.overlay`; capability=1 |
| `drivers.pico_de_gallo.uart.fake_no_capability` | Capability refusal and direct failed-init dispatch | no | 10 s | `fake.overlay`; `PDG_FAKE_UART_CAPABILITY=0` |
| `sample.pico_de_gallo.uart_bridge` | Sample compile/link and binding warnings | yes | n/a | sample `app.overlay` |

No scenario uses `depends_on: uart`; native_sim/native/64 does not advertise it and Twister would silently filter the scenario.

## 10. Verified M6 results

The completed milestone produced the following measured record:

- `west twister` over `zephyr/samples` and `zephyr/tests` on `native_sim/native/64`: **11 scenarios, 0 failed, 0 errored**; 3 executed and 8 build-only; **19/19 executed test cases passed**.
  - `drivers.pico_de_gallo.uart.fake`: 14/14.
  - `drivers.pico_de_gallo.uart.fake_no_capability`: 3/3.
  - `drivers.pico_de_gallo.i2c.fake`: 2/2, retained as the regression control for the shared-fake change.
- `ci-build.sh --self-test`: **17 passed, 0 failed**. A full run confirmed all **12** targets met their declared contracts.
- Symbol gate: **6 bottom symbols checked: 5 strongly overridden, 1 absent (unreferenced), 0 weak**. Strong: `pdg_common_bottom_open`, `pdg_uart_bottom_has_uart`, `pdg_uart_bottom_read`, `pdg_uart_bottom_set_config`, and `pdg_uart_bottom_write`. Absent: `pdg_common_bottom_close`.
- Mutation proof: disabling the staging-ring fast path in `pdg_uart_poll_in()` made `test_one_refill_serves_eight_polls` fail at its byte-order assertion and also made `test_cache_and_staged_ring_survive_a_failed_set_config` fail. The driver was restored byte-identically: SHA-256 `fbc5da2626019555d021d6aa82feecb6905afd62969f92022d5e897039b57471`, with empty `git diff`.
- The exact read-count assertion was independently observed to fire during development: an accidental second `uart_poll_in()` in a `zassert` message argument caused a third bottom read and reported “must be EXACTLY 2, got 3”. Durable rule: **no side-effecting call may appear in a `zassert` message argument**. `z_zassert()` is a function, so it evaluates message varargs unconditionally whether the assertion passes or fails.

## 11. `ci-build.sh` target table

The delivered table contains 12 targets. `uart_bridge` is grouped with the samples, while `uart_fake` is the final target:

```text
uart_bridge|pass|zephyr/samples/uart_bridge||pdg_mfd.c,pdg_uart.c|gallo_registry,pdg_uart_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_UART_PICO_DE_GALLO
...
uart_fake|pass|zephyr/tests/pdg_fake/uart|zephyr/tests/pdg_fake/uart/fake.overlay|pdg_mfd.c,pdg_uart.c|gallo_registry,pdg_uart_bottom,pdg_uart_fake_bottom|CONFIG_MFD_PICO_DE_GALLO,CONFIG_UART_PICO_DE_GALLO
```

`ci-build.sh` builds capability=1 and never runs it. Its fake build executes the CMake symbol gate through the self-owned target described in §3.3. The script's self-test contains 17 assertions, including the exact 12-target order.

## 12. Documentation contract

- `zephyr/README.md`: sample, five-instance topology, exact executed fake coverage, explicit gaps, scenario/target counts, and M7 boundary.
- `zephyr/CHANGELOG.md`: Unreleased/Added entry for sample, recording fake, two executed scenarios, and hardware limits.
- `pdg_uart_bottom.h`: correct timeout comment only; no behavioural edit.
- `.github/workflows/zephyr.yml`: comments only. Remove claims that all work is build-only/nothing executes and update stale `tests.yaml` counts. Do not alter logic: the existing sample/test roots discover M6 automatically.
- No `ROADMAP.md` or book edit.

## 13. Final file manifest

**Create**

- `zephyr/samples/uart_bridge/CMakeLists.txt`
- `zephyr/samples/uart_bridge/prj.conf`
- `zephyr/samples/uart_bridge/app.overlay`
- `zephyr/samples/uart_bridge/tests.yaml`
- `zephyr/samples/uart_bridge/src/main.c`
- `zephyr/tests/pdg_fake/uart/CMakeLists.txt`
- `zephyr/tests/pdg_fake/uart/verify_overrides.cmake`
- `zephyr/tests/pdg_fake/uart/prj.conf`
- `zephyr/tests/pdg_fake/uart/fake.overlay`
- `zephyr/tests/pdg_fake/uart/tests.yaml`
- `zephyr/tests/pdg_fake/uart/src/main.c`
- `zephyr/tests/pdg_fake/uart/pdg_uart_fake_bottom.c`
- `zephyr/tests/pdg_fake/uart/pdg_uart_fake_bottom.h`

**Modify**

- `zephyr/tests/pdg_fake/common/pdg_fake_bottom.c` — count defensive parent-close calls.
- `zephyr/tests/pdg_fake/common/pdg_fake_bottom.h` — expose close count without FFI types.
- `zephyr/drivers/serial/pdg_uart_bottom.h` — stale timeout comment only.
- `zephyr/scripts/ci-build.sh` — two rows and 12-target self-tests.
- `zephyr/README.md` — sample, fake coverage, counts, isolation, and gaps.
- `zephyr/CHANGELOG.md` — M6 entry.
- `.github/workflows/zephyr.yml` — stale comments only; no logic change.

**Do not modify** `zephyr/drivers/serial/pdg_uart.c`, `ROADMAP.md`, any book file, any Cargo file/lockfile, or anything under `crates/`. Never bump `[package].version`. No production UART behaviour change is authorized.

## 14. Reproduction commands

These commands document how to reproduce focused M6 checks; they were **not rerun during this specification reconciliation**. The Zephyr venv must be first on PATH. Full-suite runs should place `--outdir`/`--build-root` on a filesystem larger than the development WSL instance's 16 GiB `/tmp` tmpfs (§16).

```powershell
# One executable scenario
wsl -d Ubuntu-26.04 -- bash -c 'export PATH=$HOME/zephyr-venv/bin:$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin; export ZEPHYR_BASE=$HOME/zephyrproject/zephyr; export ZEPHYR_TOOLCHAIN_VARIANT=host; cd /mnt/d/workspace/pico-de-gallo; west twister -T zephyr/tests/pdg_fake/uart -p native_sim/native/64 -s drivers.pico_de_gallo.uart.fake --outdir /tmp/pdg-uart-one --jobs 1 --inline-logs --verbose'

# Both fake policies
wsl -d Ubuntu-26.04 -- bash -c 'export PATH=$HOME/zephyr-venv/bin:$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin; export ZEPHYR_BASE=$HOME/zephyrproject/zephyr; export ZEPHYR_TOOLCHAIN_VARIANT=host; cd /mnt/d/workspace/pico-de-gallo; west twister -T zephyr/tests/pdg_fake/uart -p native_sim/native/64 --outdir /tmp/pdg-uart-fake --jobs 1 --inline-logs --verbose'

# Build gate only
wsl -d Ubuntu-26.04 -- bash -c 'export PATH=$HOME/zephyr-venv/bin:$HOME/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin; export ZEPHYR_BASE=$HOME/zephyrproject/zephyr; export ZEPHYR_TOOLCHAIN_VARIANT=host; cd /mnt/d/workspace/pico-de-gallo; zephyr/scripts/ci-build.sh --targets uart_driver,uart_bridge,uart_fake --build-root /tmp/pdg-uart-m6 --summary /tmp/pdg-uart-m6.md'
```

The repository-wide workflow already names both `zephyr/samples` and `zephyr/tests` as testsuite roots. Never run the board-attached sample in CI.

## 15. Alternatives considered

- **Production diagnostic accessor/header:** rejected; behavioural assertions are equal or stronger and avoid widening `pdg_uart.c`.
- **Contended ISR mutex test:** rejected; it risks deadlocking the single-threaded test and still belongs to M7's threaded obligation.
- **Strong-over-weak without symbol inspection:** rejected as silent when a referenced override is missing. The delivered post-link gate distinguishes safe absence from a surviving weak production definition.
- **Runtime capability setter:** rejected; setup occurs after POST_KERNEL init.
- **Separate capability source trees:** rejected; one compile-time policy bit does not justify duplication.
- **Reset production latch/backoff state from tests:** rejected; dedicated instances preserve the state machine.
- **Shuffle tests:** rejected; the suite has no supported hook to re-arm latch devices, regardless of the bounded duration proved dynamically.
- **Claim firmware/USB/framing evidence from a synchronous fake:** rejected; those are M7 obligations.

## 16. Findings / escalations and open questions

### Findings / escalations

1. `pdg_uart_bottom.h` contradicted the implemented zero-timeout path; M6 corrected its comment without changing behaviour.
2. Pre-kernel refusal, guard-before-mutex ordering, successful-backoff-clear assignment, per-child init ordering, and device-lifetime latch permanence are not behaviourally observable in this suite.
3. A missing strong override can silently select real FFI; §3.3 records the delivered mechanical gate and the failed `POST_BUILD` attach-point attempts that preceded it.
4. Capability=false is only one init failure. Capability-query error and initial-config error need later evidence.
5. Twister's ten-second process timeout, not bounded loops, contains a callback blocked on `K_FOREVER`.
6. The shared parent fake currently ignores serial selection and does not exercise strict open/schema validation.
7. `zephyr/tests/pdg_fake/i2c/` was never wired into `zephyr/scripts/ci-build.sh`. This pre-existing coverage gap was discovered during M6 and remains unaddressed.
8. `ROADMAP.md` remains stale: it omits Zephyr UART and describes the I2C fake as the only executed suite. Updating it was deliberately outside M6 scope.
9. The development WSL instance mounts `/tmp` as a 16 GiB tmpfs. A full Twister run exhausted it and a pre-existing target failed at link with “No space left on device”; this was an environment-capacity failure, not a code defect. Full runs need a build root on a roomier filesystem.
10. No side-effecting call may appear in a `zassert` message argument; `z_zassert()` evaluates varargs unconditionally. M6 observed this defect class create an extra bottom read and a truthful cardinality failure.

### Open questions

None remain for M6; implementation and the measured verification record are complete. M7 retains the board-attached obligations in §8.3.
