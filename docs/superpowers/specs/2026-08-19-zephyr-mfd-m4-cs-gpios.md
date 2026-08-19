# Zephyr MFD restructure M4 — standard `cs-gpios` specification and implementation plan

Date: 2026-08-19
Branch baseline: `zephyr` at `8147e207efd2`
Milestone: M4 — replace the Zephyr-only chip-select mapping with ordinary `cs-gpios`
Status: amended after parallel architecture and reliability review

## 1. Context, scope, and final inventory

M4 is SP1's semantic milestone. The SPI child stops asking firmware `spi/batch`
to own chip-select and uses M3's `odp,pico-de-gallo-gpio` child for each edge
around ordinary `spi/transfer`. Child `reg` regains its standard Zephyr meaning:
index into the controller's `cs-gpios` array.

This trades one firmware-atomic batch for four fallible USB RPCs on an ordinary
successful transceive. It gains standard DT composition, `SPI_LOCK_ON`, and
safe `SPI_HOLD_ON_CS | SPI_LOCK_ON`, but cannot preserve CS across host death or
recover a non-returning host RPC. No parent lock, firmware/wire/crate change,
version bump, or lockfile update is allowed. Non-Zephyr batch APIs remain.

M4 verification is compile-time only. Nothing on this branch has ever executed;
M5 is the first runtime milestone.

### 1.1 Final inventory

**Create:** this specification; the tester may create an M4 acceptance/test
specification under `docs/superpowers/specs/`.

**Modify during implementation:**

- `zephyr/dts/bindings/spi/odp,pico-de-gallo-spi.yaml`
- `zephyr/drivers/spi/pdg_spi.c`
- `zephyr/drivers/spi/pdg_spi_bottom.{c,h}`
- `zephyr/drivers/spi/Kconfig`
- `zephyr/samples/{spi_bridge,spi_nor_id,combined_i2c_spi_bridge}/app.overlay`
- `book/src/interfaces/{spi,gpio}.md`
- `zephyr/README.md`
- `zephyr/CHANGELOG.md`

The shield overlay remains disabled and unchanged. No `crates/`, firmware, wire,
Cargo manifest, or lockfile change.

## 2. Decisions

| Decision | Contract and rationale |
| --- | --- |
| `cs-gpios` required | Every enabled PDG SPI controller has at least one entry. Missing CS is a DT/build error; the bridge has no native-CS fallback. Inherited `cs-gpios` may be tightened with local `required: true`; edtlib ORs inherited/local required and checks required properties only on status-okay nodes, so the disabled shield remains valid. |
| Same-parent PDG GPIO only | Every entry targets an enabled `odp,pico-de-gallo-gpio` sibling under the exact same MFD. Foreign/cross-parent CS would split physical identity. |
| Explicit parent identity | Enabled SPI requires parent `serial-number`, as M3 requires for physical GPIO actuation. Samples use an unmistakable placeholder, not a real fixture serial. |
| Checked CS edges | Transceive bypasses void `spi_context_cs_control()` and checks `gpio_pin_set_dt()`. The local helper reproduces upstream GPIO-CS delay/HOLD behavior. |
| Defanged stock unlock | Before `spi_context_unlock_unconditionally()`, save `ctx.config` and set it NULL so stock force-off is a no-op. After unlock, restore it only after failed checked deassert so a latched fault can be retried; successful release leaves it NULL. No unchecked second edge exists. |
| Fault latch | An unacknowledged force-deassert (as defined in §5.1) latches controller fault and the first originating errno. Later transfers return `-EHOSTDOWN` before I/O until checked `spi_release()` deasserts successfully. |
| HOLD requires LOCK | HOLD without LOCK returns `-ENOTSUP`. This prevents another config selecting a second slave while the first remains selected. |
| Transfer endpoint | Zephyr uses `gallo_spi_transfer`; remove batch shim/types/count. Read-only/write-only become zero-fill/discard full-duplex transfers of `max(tx_len, rx_len)`. |
| RX commit barrier | Do not unflatten scratch RX until checked ordinary deassert succeeds, or successful HOLD intentionally leaves CS asserted. |
| Dedicated priority | Add `CONFIG_SPI_PICO_DE_GALLO_INIT_PRIORITY=50`; readiness is authoritative and inversion fails loudly with `-ENODEV`. |
| Sample CS index 0 | Preserves the former intended mapping and avoids known fixture residue on 2/3; it is not a universal electrical-safety claim. |

## 3. Devicetree and binding contract

Delete `cs-gpio-indices` entirely. Carry forward direct enabled MFD-child and
inherited board-selection rules. Override inherited `cs-gpios` with
`required: true`.

```dts
&pdg0 {
	status = "okay";
	/* REQUIRED: replace this using the serial shown by `gallo list`. */
	serial-number = "REPLACE_WITH_YOUR_PICO_DE_GALLO_SERIAL";
};
&pdg_gpio0 { status = "okay"; };
&pdg_spi0 {
	status = "okay";
	cs-gpios = <&pdg_gpio0 0 GPIO_ACTIVE_LOW>;
	device@0 { compatible = "vendor,device"; reg = <0>; };
};
```

`reg = <N>` selects the Nth entry. Pin cells use firmware user indices 0–3,
not RP2350 or header numbering. `GPIO_ACTIVE_LOW` is typical. `GPIO_ACTIVE_HIGH`
is permitted because GPIO logical polarity determines the physical edge;
`SPI_CS_ACTIVE_HIGH` remains rejected because this driver does not implement the
separate operation-flag polarity contract. This asymmetry is coherent.

Each CS target must be compatible, status-okay, and have the same parent. Use
the reviewer-verified shape:

```c
DT_FOREACH_PROP_ELEM_VARGS(DT_DRV_INST(inst), cs_gpios,
			   PDG_SPI_CS_ASSERT, inst)
```

`PDG_SPI_CS_ASSERT(node_id, prop, idx, inst)` derives `ctlr` with
`DT_GPIO_CTLR_BY_IDX`, then asserts `DT_NODE_HAS_COMPAT`,
`DT_NODE_HAS_STATUS_OKAY`, and
`DT_SAME_NODE(DT_PARENT(ctlr), DT_INST_PARENT(inst))`. Diagnostics include the
CS array index where macro formatting permits. Assertions follow parent
compatible → status → serial → Kconfig and remain above `pdg_mfd.h`.

Missing `cs-gpios` fails DT validation. This also prevents the whole-compatible
`DT_SPI_CTX_HAS_NO_CS_GPIOS` path from compiling stock CS operations to no-ops.
Probe missing, foreign `gpio_emul`, disabled PDG GPIO, cross-parent PDG GPIO, and
valid sibling.

### 3.1 Atomicity, delay, ownership, and RPC limits

Ordinary success is:

```text
spi/set-config -> gpio/put(assert) -> spi/transfer -> gpio/put(deassert)
```

Four RPCs; CS is no longer firmware-atomic. Host death after assertion can leave
CS asserted. Zephyr collapses child `setup_ns` and `hold_ns` into one value:

```text
DIV_ROUND_UP(MAX(setup_ns, hold_ns), 1000) microseconds
```

and applies that same delay after assert and before deassert. The checked helper
must reproduce both `k_busy_wait(config->cs.delay)` calls. M4 preserves upstream
GPIO-CS semantics, not the old batch path's separately honored setup/hold delays.
Even preserved, microsecond waits between millisecond RPCs cannot provide
meaningful nanosecond timing.

The GPIO child is the sole **driver path** for pin mode, not an ownership
reservation. Applications must give SPI exclusive ownership of every declared CS
pin; direct GPIO consumers can otherwise reconfigure it between SPI operations.

The binding and user documentation also name `-EHOSTDOWN` as the latched-controller
refusal and explain that only successful checked release clears it.

D10 applies only to RPCs that return. `send_resp()` for set-config, transfer, and
GPIO edges has no host timeout. Cable loss/lost response may leave the call
pending forever after remote execution; no epilogue or latch update runs and the
SPI lock remains held. Bounded cancellation requires out-of-scope host-library
work. Operator recovery is process termination followed by deassert during a
fresh initialization/session, or power-cycle if state cannot be reclaimed.

## 4. Driver structures, assertions, and initialization

```c
struct pdg_spi_data {
	struct spi_context spi_ctx; /* MUST be first */
	void *ctx;
	bool cs_fault;
	int cs_fault_errno;
};
```

Config stores MFD and parent serial for diagnostics. Remove CS index arrays,
length, GPIO count, and mutex. Static data uses `SPI_CONTEXT_INIT_LOCK`,
`SPI_CONTEXT_INIT_SYNC`, and `SPI_CONTEXT_CS_GPIOS_INITIALIZE` in that order.
Preserve `SPI_ASYNC`/`SPI_RTIO` assertions. Parent assertion order is compatible
→ status → serial → Kconfig, then per-CS compatible → status → same parent.

### 4.1 Init sequence and exact residue documentation

Initialize latch to clear/zero by static initialization. Run readiness → accessor
→ invariant-NULL → a local indexed configure-all loop over `spi_ctx.cs_gpios`.
The loop reproduces `spi_context_cs_configure_all()` exactly—readiness check before
`gpio_pin_configure_dt(..., GPIO_OUTPUT_INACTIVE)`—but retains array index/pin for
the required diagnostics. It returns `-ENODEV` before configuration when a port is
unready, so priority inversion is loud and actuates no pin.

The stock helper cannot satisfy M5's diagnostic requirement because it returns only
errno and discards the failing iterator/index. Calling it and then re-probing would issue
duplicate state-changing/unbounded RPCs. The local indexed equivalent therefore
supersedes the original requirement to call the stock helper while preserving its
ordering and behavior.

The indeterminate states below are **residue documentation for operators, not
testable contract claims**: no observation can disprove them. Each
`GPIO_OUTPUT_INACTIVE` is two RPCs in M3: set-config then put. For entries in
ascending array order:

- earlier fully acknowledged entries are explicit outputs and inactive;
- on failed set-config at entry N, N's direction/pull are indeterminate because
  execution may precede lost ACK; its level is prior/HAL-defined; later entries
  are untouched;
- on failed inactive put at N, N is an explicit output but level is
  indeterminate and may be active; later entries are untouched;
- no rollback/force-deassert is attempted because init has no trustworthy prior
  config and another unbounded RPC can hang boot.

Log CS array index, GPIO pin, parent serial, phase, and errno. For `-EBUSY`, say
that a firmware GPIO subscription owns the pin and point to
`gallo_system_reset_subscriptions`; include index and pin. Clear child `ctx` and
fail init. Operator recovery: explicitly reset subscriptions after strict open
in an acceptance/reconnect setup, retry with a fresh process, or power-cycle;
inspect/deassert the affected CS before trusting the bus.

After the successful indexed configure-all loop, call `spi_context_unlock_unconditionally()`
while `spi_ctx.config == NULL`; stock CS guard therefore issues no edge and the
static zero-count semaphore is given. Latch remains clear.

### 4.2 Priority

Evidence: MFD resolves 40, GPIO 45, upstream SPI default 50. Add project SPI
priority 50 and require it greater than GPIO in help text. This knob adds a
misconfiguration surface; Kconfig cannot enforce arithmetic. Runtime readiness is authoritative: inversion causes the indexed configure-all
loop to see an unready GPIO and return `-ENODEV` before configuration, not
actuate a wrong pin.
## 5. D10 checked edges, lock ownership, and fault latch

### 5.1 Checked helper and mandatory comment

A private helper returns `gpio_pin_set_dt()` errno and mirrors upstream.

**Terminology.** In this specification, an **unacknowledged edge** means
`gpio_pin_set_dt()` returned nonzero. The driver cannot distinguish a remote edge
that did not execute from one that executed but whose response was lost. A
**non-returning edge** is different: the call remains pending and no errno or
epilogue is available. This terminology applies to every latch/residue statement
below.

The helper behavior is:

```text
assert: set logical 1; if acknowledged, busy_wait(delay)
deassert: if !force_off && HOLD return 0; busy_wait(delay); set logical 0
```

Never verify with `gpio_pin_get_dt()`: M3 masks explicit outputs and legacy reads
mutate direction. Mandatory source comment:

> Do not replace this with `spi_context_cs_control()`. PDG CS is a fallible,
> potentially non-returning USB GPIO operation; Zephyr's void helper discards
> errno. This helper preserves upstream delay/HOLD rules while making returning
> failures observable.

### 5.2 Flag validation and transfer sequence

Reject `SPI_HOLD_ON_CS` without `SPI_LOCK_ON` as `-ENOTSUP`, naming the missing
flag. Repeated HOLD transfers with the same LOCKed config remain valid. M5 must
set both. Thread/process death strands asserted CS and software ownership.

After context/config/buffer checks, acquire `spi_context`, then check the latch before
set-config, assert, or clocks. The latch is protected by this same controller lock;
checking only before lock would race a preceding transfer that faults while this caller
waits. On a latched result, defanged-unlock while preserving the retained fault config
and return `-EHOSTDOWN` with no hardware I/O. The diagnostic carries parent serial,
retained fault-config CS pin, latched originating errno, and says
`spi_release()` must successfully deassert to recover.

If the post-lock latch check passes, assign `spi_ctx.config = config`, then run
set-config, checked assert, transfer,
and checked deassert unless successful HOLD. Assert failure performs one checked
force-deassert, issues no clocks, preserves assert errno, and latches only if the
force-deassert returns nonzero. Every such unacknowledged force-deassert, as
defined in §5.1, latches fault.

RX remains scratch until cleanup is acknowledged. Ordinary success unflattens
only after successful deassert. On a successful deliberate HOLD, RX **must** be committed because no deassert is
pending to serve as the commit barrier. Together the rules are exhaustive: ordinary
non-HOLD success commits only after acknowledged deassert; successful HOLD+LOCK
commits immediately after transfer while intentionally retaining selection. Successful transfer plus
failed deassert returns deassert errno and does **not** commit RX; Zephyr has no
“valid data plus failed cleanup” result, the peripheral may remain selected, and
`spi_nor` collapses failures to `-ENODEV` anyway.

Every phase-specific failure log includes parent serial, CS GPIO pin, primary
errno, cleanup errno (0/not-attempted where applicable), and whether the fault
latch was entered. When cleanup supersedes transfer errno, the primary survives
in this log.

### 5.3 Defanged unconditional unlock (M1)

`spi_context_unlock_unconditionally()` first calls void force-off, then clears
owner/gives the semaphore. Calling it with a live GPIO config would recreate D10
and can hang before unlock. Use a private helper:

```text
saved = ctx->config
ctx->config = NULL
spi_context_unlock_unconditionally(ctx)  // stock edge guard is false; semaphore given
ctx->config = deassert_failed ? saved : NULL
```

The checked deassert occurs before this helper. Invoke defanged unlock regardless
of its result; uncertain hardware is never allowed to wedge software ownership.
Restore `saved` only when checked deassert failed and the fault remains latched.
That preserves the exact recovery target so a later `spi_release(dev, saved)` passes
the config check and can retry. On success leave config NULL, so a second release
is rejected. `saved` also supplies diagnostics/CS selection before clearing.

Verified in pinned upstream source: `_spi_context_cs_control()` does nothing when
`ctx->config == NULL`; the remainder of unconditional unlock reads only lock and
owner, sets owner NULL, and gives the semaphore. `spi_context_lock()` itself does
not assign `config`, but the specified next transceive assigns
`ctx->config = config` immediately after locking, before any configure/CS helper.
Thus clearing does not break unlock and the next driver path re-establishes it.
Mandatory source comment explains this defanging and warns not to restore the
idiomatic live-config call.

### 5.4 Fault latch (M2)

`cs_fault=false`, `cs_fault_errno=0` at init. Set the latch whenever a checked
**force-deassert returns nonzero** (an unacknowledged edge under §5.1): assert-cleanup, transfer cleanup, successful
transfer cleanup, or release. Preserving the first originating deassert errno is **documented best-effort
diagnostic behavior**, not an M4/M5 acceptance requirement: natural faults
collapse to `-ECOMM`/`-EIO`, so distinct successive errnos cannot be induced
without instrumentation. Implementations should keep the first errno while latched
and log the preserved origin plus each latest recovery errno, but M4 does not
claim this sub-property is verified.

While latched, transceive returns distinct `-EHOSTDOWN` before any hardware I/O.
`-EHOSTDOWN` means this controller cannot safely address another slave because a
previous CS may remain active; it is available in all Zephyr libc variants.

Only `spi_release()` may clear it. It first requires the matching retained fault
config, then acquires `spi_context`: this re-enters an already LOCKed matching
config or takes the released semaphore after a non-LOCK fault. Recheck config
after acquire to close the precheck/acquire race. Only a successful checked
force-deassert clears it. Then clear
bool/errno, perform defanged unconditional unlock, and leave config NULL. A failed
release unlocks but restores that config pointer only so recovery can be retried;
transceive remains blocked by the latch. A stock unchecked call can never clear it
because M1 suppresses that edge. Init starts clear only after all declared CS
were configured inactive; init failure leaves the device not ready rather than a
usable latched controller. If release cannot clear it, terminate/reinitialize
and explicitly deassert or power-cycle; no further transfers are allowed.

### 5.5 Testability boundary beyond uninstrumented M5

These properties are not testable even in M5 without new instrumentation. This is
an assurance boundary, not an M4 implementation commitment:

1. **First-errno preservation:** needs a `CONFIG`-gated fault-injection shim in
   `pdg_spi_bottom.c` that returns caller-selected distinct errnos on chosen calls.
2. **No second GPIO edge in defanged unlock:** needs a call counter around the
   GPIO bottom `put`; the duplicate logical deassert is electrically/API invisible.
3. **`-EHOSTDOWN` before any hardware I/O:** needs counters for set-config, GPIO
   put, and SPI transfer so absence of all RPCs is observable.
4. **Non-returning RPC residue:** cannot terminate by construction; only a
   process-timeout plus fresh-process recovery-procedure test can characterize it.
5. **Indeterminate init residue:** no observation can disprove “indeterminate”;
   counters can prove earlier-prefix/later-unissued ordering, while per-pin witness
   instrumentation can observe particular outcomes but not exhaust the residue set.

The acceptance document may propose such instrumentation for M5. None is added by
M4 unless separately approved.
## 6. Exact residue table

Rows explicitly marked **residue documentation** describe honest possible hardware
state but are not falsifiable acceptance criteria. Action ordering, returned errno,
latch behavior, and recovery instructions remain testable where instrumentation
exists.

| State/event | Driver action/result | Exact residue and recovery |
| --- | --- | --- |
| Init: GPIO port unready | Indexed configure-all returns `-ENODEV` before pin configuration | No CS touched; SPI not ready. Fix priority/readiness. |
| Init: set-config fails at N | Fail init; no rollback | **Residue documentation:** earlier entries acknowledged inactive; N direction/pull and level indeterminate; later entries untouched. Fresh process/deassert or power-cycle. |
| Init: inactive put fails at N | Fail init; special `-EBUSY` subscription diagnostic when applicable | **Residue documentation:** earlier entries inactive; N explicit output with indeterminate, possibly active level; later untouched. Reset subscription explicitly if relevant, then fresh init/deassert or power-cycle. |
| Returning assert fails | No clocks; checked force-deassert; preserve assert errno | Cleanup acknowledged: inactive, no latch. **Residue documentation:** cleanup returning nonzero leaves state indeterminate; latch set to cleanup errno. |
| Transfer fails after acknowledged assert | Checked force-deassert even with HOLD | Cleanup acknowledged: inactive, return transfer errno, no RX commit. **Residue documentation:** cleanup returning nonzero leaves state indeterminate; return cleanup errno, set latch, no RX commit; log both. |
| Transfer succeeds, ordinary deassert fails | Return deassert errno, set latch, no RX commit | Indeterminate: asserted if not executed, inactive if ACK lost. Later transfers return `-EHOSTDOWN`. |
| Successful HOLD+LOCK | Skip end deassert; commit RX | CS asserted and lock retained until release. Thread/process death strands both. |
| Release deassert succeeds | Clear latch if set; defanged unconditional unlock; leave config NULL; return 0 | CS acknowledged inactive; owner cleared; second release rejected. |
| Release deassert fails | Set/retain latch; defanged unconditional unlock; restore only the failed config pointer; return edge errno | **Residue documentation:** CS indeterminate. Software lock is released and traffic blocked by latch. Retry release with that exact config pointer or reinitialize/power-cycle. |
| Any set-config/assert/transfer/deassert RPC never returns | No subsequent code, latch, cleanup, RX commit, or unlock executes | **Out-of-scope non-returning boundary; residue documentation:** remote operation may have executed; CS and bus state indeterminate, software lock held. Terminate process; fresh session deassert/reinitialize or power-cycle. M4 provides no bounded recovery. |
| Host dies after acknowledged assert/HOLD | No cleanup | CS remains asserted; with LOCK ownership is also stranded until process termination. Fresh init/deassert or power-cycle. |

## 7. Data path and bottom half

Keep flattening validation and 4096-byte limit. Allocate valid `clock_len` TX/RX
scratch in every direction: TX-only gets discard RX; RX-only gets zero TX;
duplex gets both. Always clock `max(tx_len, rx_len)`. This is a behavior change
from batch selecting distinct READ, WRITE, or TRANSFER operations: Zephyr now
always invokes one full-duplex transfer, zero-filling absent TX and discarding
unrequested RX. Zero length succeeds before lock/RPC.

Bottom header contains only:

```c
int pdg_spi_bottom_set_config(void *ctx, uint32_t frequency,
			      bool phase, bool polarity);
int pdg_spi_bottom_transfer(void *ctx, const uint8_t *write_buf,
			    uint8_t *read_buf, size_t len);
```

Transfer forwards exactly to `gallo_spi_transfer((const struct PicoDeGallo *)ctx,
write_buf, read_buf, len)` through common status mapping. Relevant returns: 0,
`-EINVAL`, `-EMSGSIZE`, `-EPROTO`, `-EIO`, `-ECOMM`. Delete open/close/count/
batch, max-ops, batch enum/struct. Do not change non-Zephyr batch APIs.

## 8. Samples and M5 fixture

All three SPI overlays include GPIO bindings, enable `pdg_gpio0`, and use index
0 active-low. Index 0 is firmware user GPIO0 = header pin 11 = RP2350 GPIO8;
index 1 is header pin 12 = RP2350 GPIO9. Neither has a **recorded fixture
residue**, unlike monitored 2 and parked-high 3. This preserves the previous
intended mapping but does not promise electrical safety for arbitrary attached
hardware. Rewrite `spi_nor_id`'s “GPIO 8” comment as “firmware user GPIO0
(RP2350 GPIO8, header pin 11)” so index and physical numbering cannot be confused.

Each sample parent uses an unmistakable placeholder:

```dts
/*
 * REQUIRED: replace this placeholder with this board's serial from `gallo list`.
 * SPI CS actuates GPIO, so R11 forbids selector-less operation: with multiple
 * boards attached, it could drive the wrong board's pins.
 */
serial-number = "REPLACE_WITH_YOUR_PICO_DE_GALLO_SERIAL";
```

M5 uses CS 2/witness 3 and sets `SPI_HOLD_ON_CS | SPI_LOCK_ON`. In acceptance
setup, after strict open, explicitly call `gallo_system_reset_subscriptions()`,
check status, and log count reset. Keep it out of GPIO callbacks, SPI init, and
ordinary parent init: automatic reset is hidden global mutation and can destroy
deliberately retained subscriptions. Pin 3 would configure successfully and can
silently take over a pin another consumer believes it owns; M5 must establish
exclusive fixture ownership.
## 9. Documentation parity and search accounting

Update in the same change:

- `book/src/interfaces/spi.md`: standard CS, exclusive ownership, delay collapse,
  atomicity/non-returning limits, HOLD+LOCK, latch/recovery, RX commit barrier,
  and full-duplex read/write-only change; preserve host batch docs.
- `book/src/interfaces/gpio.md`: CS role, explicit output/inactive init,
  same-parent/exclusive ownership, monitored-pin/reset guidance.
- `zephyr/README.md`: placeholder overlay, mapping, limitations/errors,
  latch/release, priority readiness, and troubleshooting.
- `zephyr/CHANGELOG.md`: delete temporary mapping; record required standard CS,
  transfer endpoint, atomicity loss, HOLD+LOCK, latch, init coupling/residue.

Baseline `rg -n "cs-gpio-indices|cs_gpio_indices" .` accounting:

**Delete/rewrite live truth:** SPI driver lines 207,273,281,288,298,306,482,493,
494,502; SPI binding lines 18,20,34,46,54 and property prose; three SPI sample
properties; SPI book lines 122,132,145; README lines 337,354,358,362,378,414,
501–503; Unreleased changelog line 51.

**Preserve history/migration records:** `AGENTS.md:777`; dated #104 plan lines
173,177,318,326; dated #104 design lines 103,104,320,333; M2 spec lines
24,300,362,383,432,499,711,732,753,892; M3 spec lines 140,809; parent design
lines 24,69,154,161; parent plan lines 9,37,147,267; **this M4 specification**;
and any M4 acceptance/test specification later created. The corrected gate is
zero hits outside this explicit preserve set, not an impossible repository-wide
zero.

## 10. Verification and expected build outcomes

Never invoke `gallo_*` MCP, `probe-rs`, `cargo run -p gallo`, or an image. Run
one build at a time with the prescribed native/64 command and unique `/tmp/m4_*`.

Expected per sample, preserving measured R5 baseline:

- `i2c_bridge`: clean, 116/116.
- `spi_nor_id`: clean, 117/117.
- `spi_bridge`: `zephyr/zephyr.elf` links clean; native-simulator runner link
  fails only on exactly one undefined `__device_dts_ord_*`, resolved from that
  build to `/pico-de-gallo/spi/is31fl3743b@0`, compatible
  `issi,is31fl3743b`.
- `combined_i2c_spi_bridge`: same one runner-link failure and resolved path;
  `zephyr/zephyr.elf` links clean.

A failure while `Linking C executable zephyr/zephyr.elf` is new, not R5. Compare
symbol + resolved path + count from each build's generated header, never literal
ordinal. Enabling GPIO legitimately renumbers ordinals.

Prove `CONFIG_PICO_DE_GALLO=y`, expected MFD/GPIO/SPI config, and embedded TUs
using digit-bearing `pdg_[a-z0-9_]*\.c`. Bottom files never appear in compile
commands; verify objects and `nm`. Run workspace tests, mdBook, scoped `rg`, and
`git diff --check`. Compile-time evidence proves no runtime semantics.

Adversarial tests must cover: required property; same-parent assertion shape and
index diagnostics; placeholder comment; init residue and `-EBUSY`; collapsed
delay at both edges; HOLD-without-LOCK rejection; latch set/refuse/clear; defanged
unlock with zero second edge; unlock despite failed deassert; RX commit barrier;
non-returning boundary; exact build categories. Exclusive ownership remains a
documentation obligation, not a driver-behavior test.
Mutation controls reintroduce live config before stock unlock, omit latch/refusal,
commit RX before cleanup, or allow traffic while faulted.

## 11. Implementation plan

1. **RED binding/topology probes:** missing, foreign, disabled, cross-parent,
   missing serial, valid sibling; exact foreach macro shape.
2. **Binding/Kconfig:** required property, complete trade contract, dedicated
   priority and loud inversion behavior.
3. **RED bottom tests then rewrite:** exact transfer signature, no dead batch/
   count/open surface.
4. **RED init tests:** implement and test §4.1's local indexed loop; the stock
   `spi_context_cs_configure_all()` must not be called. Cover static context/latch,
   assertions, indexed residue, monitored diagnostic, and initial no-edge unlock.
5. **RED D10 tests:** returning and non-returning models, delays, assert cleanup,
   transfer cleanup, RX barrier, observability fields.
6. **RED lock/latch tests:** HOLD requires LOCK, repeated same-config HOLD,
   latch check after lock, synchronized release recovery and race recheck, `-EHOSTDOWN`,
   checked release clear/fail, config-NULL defanged unlock, no
   second GPIO call.
7. **Driver rewrite:** implement §§4–7 with both mandatory source comments.
8. **Samples/docs:** index 0, placeholder/comment, numbering correction, parity,
   scoped search.
9. **Integration:** expected per-sample outcomes, object/nm evidence, tests/book,
   LF/diff/inventory; integrator alone commits, never pushes.

## 12. Invariants

1. Enabled SPI has explicit CS and explicit placeholder/replaced board identity.
2. Every CS is enabled same-parent PDG GPIO. Exclusive ownership is a documented
   application obligation, not a driver-enforced invariant.
3. Priority inversion fails loudly before pin configuration.
4. Context and latch are initialized before every return.
5. No unchecked CS edge occurs in transceive or release.
6. Defanged unconditional unlock always releases software ownership after a
   returning checked release edge, even when that edge fails.
7. Failed force-deassert latches fault under the controller lock; later callers
   acquire then recheck the latch, clock nothing, and return `-EHOSTDOWN`.
8. Only successful checked `spi_release()` clears the latch.
9. HOLD requires LOCK; M5 sets both.
10. RX commits only after acknowledged ordinary deassert or deliberate HOLD.
11. **D10 guarantee:** for every CS RPC that returns, the driver unconditionally
    applies the checked-edge, cleanup, latch, RX-barrier, and unlock rules in §§5–6.
    A non-returning RPC is a separately named out-of-scope execution class: it
    provides no errno or epilogue and is governed only by the recovery boundary in
    §§3.1, 5.5, and 6.
12. No Zephyr batch/type/count/index residue; host batch remains unchanged.
13. Documentation lands with behavior; M6 consolidates only.
14. M4 assurance remains compile-time only.

## 13. Alternatives rejected

- Live-config stock unlock: issues an unchecked, potentially non-returning second
  edge before semaphore give; violates D10.
- Local copy of semaphore logic: unnecessary while upstream's config guard safely
  defangs only the edge and retains canonical owner/unlock behavior.
- Always restore config after unlock: makes successful release retain stale identity.
  Restore only for a latched failed release so checked recovery can be retried.
- No fault latch: later slave can succeed while an earlier slave remains selected.
- Reuse originating errno for refusal: not distinct; `-EHOSTDOWN` communicates
  controller-wide unsafe state rather than one operation's failure.
- HOLD without LOCK: permits simultaneous slave selection and stale release.
- Readback verification: explicit outputs are masked; legacy reads mutate state.
- Crate timeout work: required for bounded recovery but prohibited by M4/D7.
- Real fixture serial in samples: looks valid but fails every other board and
  publishes physical identity; obvious placeholder is safer interim policy.
- Claim pin 0 universally safe: only absence of recorded fixture residue is known.
- Automatic subscription reset: hidden global mutation.

## 14. Corrections for parent plan/design and M5

1. Plan §1's M4 “CS edges witnessed” gate is impossible under compile-only M4;
   M5 owns edge observation.
2. Plan §7's unqualified old-property zero conflicts with history and the M4
   specs; use §9's preserve list.
3. Design §4.2 orders assert before set-config; M4 reverses it to shorten selected
   idle time.
4. Design §5 understates ordinary path as 3+ (explicitly four) and calls delay a
   nanosecond wait; upstream collapses max setup/hold to one rounded microsecond
   value used at both edges.
5. Design §6's “sole writer” is only sole driver path, not ownership reservation;
   direct GPIO consumers can interfere. Require exclusive CS-pin ownership.
6. Plan R7's “no software reset” is stale; reset API exists at FFI lines 706–748.
7. M5 must use `SPI_HOLD_ON_CS | SPI_LOCK_ON` and explicit checked reset setup,
   logging reset count; keep reset out of ordinary init/callbacks.
8. Plan inventory omitted SPI Kconfig, same-change docs/changelog, and this spec.
9. Tester assurance grade: approximately **20% Class A genuinely proved**, **35%
   Class B source-shape only**, and **45% Class C zero assurance in M4**. The two
   executed RED baselines prove that the current tree accepts both foreign
   `gpio-emul` CS and cross-parent PDG GPIO CS; A-03 specifically proves the
   same-parent clause is independently load-bearing.
10. The four most-argued properties remain Class B or C: defanged unlock/no second
    edge, fault-latch refusal before I/O, RX commit barrier, and HOLD-requires-LOCK.
    In particular, the latch's M4 probe is only byte-offset ordering between source
    constructs: it catches deletion/misplacement, but not checking the wrong field,
    inverted sense, or returning 0. These properties must not be reported as
    behaviorally verified until M5 plus the instrumentation boundary in §5.5.

## 15. Open maintainer policy question

Whether shipped public samples should be board-specific is deferred outside M4.
M4 uses an unmistakable placeholder plus substitution instructions. Alternatives
for maintainer decision: separate private/fixture overlays versus public samples;
a documented user-overlay mechanism that injects serial; or revisiting the
compile-time identity policy. M4 does not weaken R11 while that decision is open.