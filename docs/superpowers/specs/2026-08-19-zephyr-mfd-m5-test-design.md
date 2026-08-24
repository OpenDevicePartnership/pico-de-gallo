# Zephyr MFD M5 — adversarial test design

Date: 2026-08-19
Branch: `zephyr`, baseline `cfb7f4245ed3`
Milestone: M5 — loopback integration and hardware acceptance
Contract: `docs/superpowers/specs/2026-08-19-zephyr-mfd-m5-acceptance.md` (amended)
Role: tester. This document designs acceptance; it implements nothing.

---

## 0. Scope, threat model, and what this pass did not do

### 0.1 Threat model

M5 is the first milestone on this branch that executes anything (plan §10.5,
§11.5). The properties under test have been **source-shape assertions for four
milestones** — the latch in particular is currently "tested" by asserting a byte
offset between two source constructs (plan §11.5). The adversary I am designing
against is therefore not a malicious user. It is:

1. **A test suite that passes against a broken driver.** Three of the four
   prior-milestone test defects on record (#104 plan §8.6, §8.11; this plan
   §10.5) were tests that could not fail. That is the dominant failure mode in
   this codebase and it is what most of this document is spent defeating.
2. **A fixture that is not what the document says it is.** CS-contract §8.11 and
   §8.13 caught a bad electrical setup **twice**, both times by a check that
   would otherwise have been reported as a pass.
3. **A measurement whose meaning is ambiguous.** A witness pin reading HIGH is
   consistent with at least four distinct physical situations (§5.4). A test that
   treats it as proof of one of them is fabricating evidence.
4. **A residue-carrying predecessor.** Process-local state (latch, lock, owner)
   dies with the process; firmware pin mode and level do not (spec §6.2). Any
   phase that consumes residue from an abnormal predecessor is measuring the
   previous run.

### 0.2 What I did, under the stated constraints

Read-only investigation only. No `gallo_*` MCP tool was invoked (R1). No
hardware was touched, no build run, no test run. No file was created or modified
other than this one.

Sources read directly and cited by line below: `zephyr/drivers/spi/pdg_spi.c`,
`zephyr/drivers/gpio/pdg_gpio.c`, `crates/pico-de-gallo-ffi/src/lib.rs`,
`~/zephyrproject/zephyr/tests/drivers/spi/spi_loopback/src/spi.c`, plus the M5
spec, the restructure plan (§2 R1–R12, §10.1–§10.5, §11.1–§11.5), the design
spec §7.6, and the CS-contract plan §8.11 / §8.13.

### 0.3 Nomenclature

- **Pin 2 / pin 3** are firmware GPIO indices, jumpered together. Pin 2 is chip
  select (`GPIO_ACTIVE_LOW`). Pin 3 is the witness.
- **Strong reading** — a reading only one physical situation can produce.
- **Weak reading** — a reading consistent with the intended situation and with
  at least one failure situation. Weak readings are recorded, never asserted as
  proof.
- **Gate** — a check whose failure voids every later measurement in the run.

---

## 1. Test index

Every row states what a broken implementation would have to do to still pass.
Where that answer is "nothing", the test is deleted rather than shipped (§9.1
records the two I deleted).

| ID | Name | Phase | What it proves | What a broken driver must do to still pass |
| --- | --- | --- | --- | --- |
| **T0** | Released-node baseline | jumper | The GPIO read path reports a real electrical level, not a constant | Read must return 1 for a pulled-up released node. A read stubbed to 0 fails here; a read stubbed to 1 fails T1a/T1b |
| **T1a** | Jumper proof, 3→2 | jumper | Pin 3's output drive reaches pin 2 across the fitted jumper | Actually drive pin 3 low **and** actually read pin 2. A no-op write leaves the node at pin 2's own pull-up → HIGH → fail. A stuck-1 read fails. A stuck-0 read already failed T0 |
| **T1b** | Jumper proof, 2→3 | jumper | The same, driven from the opposite end (design spec §7.6) | Same, with the roles swapped. Defeats a driver that only works for one pin index, or whose per-pin mask is wrong |
| **T1c** | Pull-down hold baseline | jumper | The RP2350 pre-charge/hold model (R2, CS §8.11) holds on this board today, so later electrical reasoning is sound | Nothing about the driver — this is a **fixture** assertion, deliberately. It is a gate on the *environment*, and is labelled as such rather than counted as driver coverage |
| **T2** | Empirical echo classification | acceptance | The MOSI↔MISO data path is byte-exact, and if it is not, *which* deterministic corruption it is | Return the exact TX bytes in the RX buffer. A driver that never issues the transfer returns the poison; one that shifts returns a named shift; one that stubs RX returns a constant. All six named modes are distinguishable (§4.3) |
| **T3a** | CS asserted under HOLD+LOCK | acceptance | The driver drives CS low for the transaction, and keeps it low after return | Drive pin 2 low. Witness LOW against pin 3's own pull-up is producible **only** by an active drive. A driver that never touches CS reads HIGH → fail. This is the negative control (§5.5) |
| **T3b** | `spi_release()` deasserts | acceptance | The release path issues the checked deassert | Perform the LOW→HIGH transition. Measured as a **transition** in one process, not as a standalone HIGH (§5.4) |
| **T3c** | Second release rejected | acceptance | A successful release clears `ctx->config`, so replay is `-EINVAL` (`pdg_spi.c:609-614`) | Return exactly `-EINVAL`, not 0 and not `-EHOSTDOWN` |
| **T3d** | Different-config transfer after release | acceptance | The lock was genuinely given back | Complete a transfer with a distinct `spi_config` address without blocking |
| **T3e** | HOLD without LOCK rejected | acceptance | `pdg_spi.c:413-419` fires **before** any I/O | Return `-ENOTSUP` **and** leave the witness HIGH. The witness clause is what distinguishes "rejected early" from "rejected after asserting CS" |
| **T4** | Fault injection: latch, `-EHOSTDOWN`, recovery | acceptance, **last** | D10 checked-deassert propagation, plan §11.1 latch entry, `-EHOSTDOWN` return, failed-release software unlock, successful-release latch clear | Return `-EBUSY` from release, then `-EHOSTDOWN` from the next transfer, then 0 from the retried release. Four distinct wrong outcomes are individually diagnosed in §6.5 — including the worst, `0`, which is exactly the bug the latch exists to prevent |
| **T5a** | 1013 accepted, TX-only | acceptance | The measured ceiling is inclusive; the boundary is `<=`, not `<` | Complete a 1013-byte TX-only transfer. `bufset_len_` uses `>` (`pdg_spi.c:293`), so an off-by-one to `>=` fails here |
| **T5b** | 1014 rejected locally | acceptance | Rejection happens at `pdg_spi.c:438-445`, **before** `k_malloc` (:457), `spi_context_lock` (:471), set-config (:494), and CS (:505) | Return `-EMSGSIZE` **and** leave the witness HIGH **and** emit no set-config. The witness clause is what makes "local" observable (§7.2) |
| **T5c** | 4096 rejected locally | acceptance | The specific regression named in spec §12.1: 4096 previously reached transport and failed `-ECOMM` | Return `-EMSGSIZE`, not `-ECOMM`. The errno *is* the discriminator |
| **T6** | Upstream `spi_loopback` | loopback | Broad data-path conformance across buffer topologies the bespoke tests do not cover | Pass the 13 cases in §8.2 and skip exactly the 8 named there. Any other disposition stops M5 |
| **T7** | Teardown state report | teardown | The fixture is left in a known state and the run's residue is recorded | Report `unknown` where no query exists, rather than guessing (spec §10) |

---

## 2. Phase ordering, and why

```text
reset → jumper → acceptance → loopback → teardown
                     └── T2 → T3 → T5 → T4 (last)
```

Two ordering decisions carry weight.

**T4 is last within acceptance.** Argued in full in §6.3.

**T5 (payload boundary) precedes T4.** T5's whole evidentiary value is the
*absence* of CS motion. If T4 ran first and left the controller latched, T5's
1014 call would return `-EHOSTDOWN` from `pdg_spi.c:479-490` instead of
`-EMSGSIZE` from `:438` — the ceiling check would never be reached and T5 would
be silently vacuous while still "erroring correctly". Ordering is the only thing
preventing that, so it is normative, not stylistic.

---

## 3. T0–T1 — the fixture gate

**Nothing downstream is trusted until this phase emits `M5_JUMPER_PASS`.** A
gate failure makes `fixture_validity` FAIL and every other verdict in the
aggregate JSON `INCONCLUSIVE` — not FAIL, because a failure whose fixture was
invalid is not evidence of a driver defect either.

The jumper image has SPI `disabled` (spec §2.2), so it is free to drive pin 2.
No later image may do so before SPI init (spec §6).

### 3.1 Sequence

| Step | Pin 2 | Pin 3 | Node driven to | Assertion | Strength |
| --- | --- | --- | --- | --- | --- |
| **T0** | `GPIO_INPUT \| GPIO_PULL_UP` | `GPIO_INPUT \| GPIO_PULL_UP` | nothing; both pulled up | both read **1** | strong against stuck-at-0 reads |
| **T1a.1** | `GPIO_INPUT \| GPIO_PULL_UP` | `GPIO_OUTPUT_LOW` | **pin 3 drives LOW** | pin 2 reads **0** | **strong** — only a driven output beats pin 2's own pull-up |
| **T1a.2** | unchanged | `GPIO_OUTPUT` set to 1 | **pin 3 drives HIGH** | pin 2 reads **1** | weak alone; the *transition* 0→1 is strong |
| **T1b.1** | `GPIO_OUTPUT_LOW` | `GPIO_INPUT \| GPIO_PULL_UP` | **pin 2 drives LOW** | pin 3 reads **0** | **strong**, opposite direction |
| **T1b.2** | `GPIO_OUTPUT` set to 1 | unchanged | **pin 2 drives HIGH** | pin 3 reads **1** | weak alone; transition strong |
| **T1c.1** | `GPIO_OUTPUT_LOW` | `GPIO_INPUT \| GPIO_PULL_DOWN` | **pin 2 drives LOW** | pin 3 reads **0** | pre-charge established |
| **T1c.2** | `GPIO_INPUT \| GPIO_PULL_DOWN` | unchanged | **released from LOW** | both read **0** | pull-down *holds* a low node |

Note the flag spelling: `pdg_gpio_pin_configure` rejects `GPIO_INPUT | GPIO_OUTPUT`
with `-ENOTSUP` (`pdg_gpio.c:226`) and rejects an init level without
`GPIO_OUTPUT` (`:236`). Every configuration above is inside the allow-list at
`pdg_gpio.c:108-109`.

### 3.2 Every GPIO read in this phase, with its precondition

This is the table the M5 spec §5.1 does not supply, and its absence is how R2
defects get shipped. **Every read below is preceded by a stated drive.**

| Read | Node driven to immediately before | Pull on the reading pin | Expected | Why R2 does not invalidate it |
| --- | --- | --- | --- | --- |
| T0 pin 2 | nothing (both released, both pulled **up**) | up | 1 | Pull-**up** works normally on RP2350 (CS §8.11 row 1). Only pull-**downs** are limited |
| T0 pin 3 | nothing | up | 1 | same |
| T1a.1 pin 2 | pin 3 drives LOW | up | 0 | An active drive overrides a pull-up. This is the one direction the silicon is unambiguous about |
| T1a.2 pin 2 | pin 3 drives HIGH | up | 1 | Drive and pull agree; weak, recorded as a transition only |
| T1b.1 pin 3 | pin 2 drives LOW | up | 0 | as T1a.1, opposite end |
| T1b.2 pin 3 | pin 2 drives HIGH | up | 1 | weak, transition only |
| T1c.1 pin 3 | pin 2 drives LOW | **down** | 0 | Node is **already low** when the pull-down is applied. This is the legal use of a pull-down per CS §8.11 row 3 |
| T1c.2 pin 2 | pin 2 released from a driven LOW | **down** | 0 | Pull-down *holds*; it is never asked to *pull down* |
| T1c.2 pin 3 | as above | **down** | 0 | same |

**No read in this design ever configures a pull-down and expects LOW without
the node having been driven low first.** That is the R2 / §8.11 rule, and it is
the rule both prior attempts violated.

### 3.3 Failure semantics

- Any failed assertion: log the pin, the configured flags, the expected and
  observed level, and **return nonzero immediately**. No `M5_JUMPER_PASS`.
- No rollback is attempted (spec §5.2). The image mutates pin modes and pulls
  and leaves them mutated; the next jumper attempt establishes its own initial
  modes explicitly, which is why every step above names both pins' full
  configuration rather than relying on inherited state.
- A gate failure **voids every subsequent measurement**. The executor must not
  run acceptance or loopback; it restarts at process 1 per spec §9.4.
- T0 failing while T1a passes is not possible in a coherent fixture and
  indicates a driver read-path defect rather than a wiring problem — report it
  as FAIL, not INCONCLUSIVE.

### 3.4 Entry-state interaction

Pin 3 arrives from #104 acceptance as an **output parked high** (plan R7).
T0 reconfigures it to `GPIO_INPUT | GPIO_PULL_UP` as its first act, so the
parked-high state is overwritten before any read. Pin 2 arrives **monitored**;
the reset image (which runs first) clears that subscription. If reset did not
run, T0's configure on pin 2 returns `-EBUSY` from `pdg_gpio_bottom` and the
phase fails loudly — which is the correct behaviour and a useful cross-check
that reset actually did something.

---

## 4. T2 — empirical echo semantics

### 4.1 The honest limitation, stated first

A MOSI↔MISO short is **mode-blind**. The same clock drives the master's shift-out
and shift-in, so a physical short echoes byte-exactly for any consistent
CPOL/CPHA. Sweeping modes 0–3 therefore proves **"all four modes are accepted
and the data path stays byte-exact"**. It does **not** prove CPOL/CPHA are mapped
correctly onto the wire, and no test on this fixture can. Recording that as
proved would be exactly the overstatement plan §10.5 warns about.

What T2 genuinely defends against is R3's real content: a *deterministic
corruption* of the data path — introduced by the driver's flatten/unflatten, by
the FFI, or by a firmware sampling-edge error — presenting as a one-bit shift
rather than as a clean failure.

### 4.2 Pattern and poison

**Pattern (5 bytes, from spec §5.2, adopted unchanged):**

```c
static const uint8_t m5_echo_tx[5] = { 0x96, 0x2D, 0xE1, 0x4B, 0x73 };
```

**Poison: `0x3C`, not `0xA5`.** This is a deliberate deviation from the spec's
suggested poison and it strengthens the test. Computing the predicted RX stream
for a **one-bit right shift** of this pattern gives byte 3 = `0xA5` — identical
to the spec's poison value. A right shift would therefore produce, at index 3, a
byte indistinguishable from "the RX buffer was never written". `0x3C` appears in
**none** of the seven predicted fault streams below, so every cell of the
diagnosis table is unambiguous.

Justification of the pattern against the six required discriminations:

| Requirement | Property of `96 2D E1 4B 73` |
| --- | --- |
| non-palindromic | reversed is `73 4B E1 2D 96` ≠ original |
| non-uniform | all five bytes distinct |
| distinguishes bit-reversal | no byte is its own bit-reversal (`0x96`→`0x69`, etc.) |
| distinguishes shifts | MSB and LSB both vary across bytes, so cross-byte carry propagates visibly |
| distinguishes stuck-at | contains neither `0x00` nor `0xFF`, and no byte is all-ones or all-zeros in either nibble |
| distinguishes lag | byte 0 is unique, so a lag exposes the poison at index 0 |

### 4.3 Diagnosis table

Derived by hand from the pattern. The classifier compares the received buffer
against each row and reports the **named** mode, not "mismatch".

| Received bytes | Diagnosis | M5 verdict |
| --- | --- | --- |
| `96 2D E1 4B 73` | **Exact echo** | PASS |
| `3C 96 2D E1 4B` | **Whole-byte lag** — RX sampled one frame late | INCONCLUSIVE (mode/fixture limitation, spec §5.2) |
| `2C 5B C2 96 E6` or `…E7` | **One-bit left shift** with cross-byte carry | INCONCLUSIVE |
| `4B 16 F0 A5 B9` or `CB 16 F0 A5 B9` | **One-bit right shift** with cross-byte carry | INCONCLUSIVE |
| `69 B4 87 D2 CE` | **Per-byte bit reversal** — LSB-first sampling | FAIL (`SPI_TRANSFER_LSB` is rejected at `pdg_spi.c:390`; seeing it means the rejection is not effective) |
| `00 00 00 00 00` | **Stuck-at-0** — MISO grounded or RX zero-filled | FAIL |
| `FF FF FF FF FF` | **Stuck-at-1** — MISO open with a pull-up, or RX fill | FAIL |
| `3C 3C 3C 3C 3C` | **RX never written** — poison intact; no transfer occurred | FAIL |
| anything else | **Unclassified mismatch** | FAIL |
| transport errno | not a data result | FAIL / INFRASTRUCTURE per §9.1 of the spec |

The left-shift and right-shift rows have two admissible last/first bytes because
the carry into the first bit and out of the last is not determined by the
pattern; both spellings are accepted for the same diagnosis.

### 4.4 Placement

T2 runs **first in the acceptance image**, before T3, T5 and T4. Everything
downstream that compares bytes — including the whole of T6 — is meaningless if
the echo is shifted. A non-exact classification records `spi_data_path` as
INCONCLUSIVE and **stops the sequence before loopback**; running the loopback
comparison suite against a known one-bit shift produces dozens of identical failures
that say nothing.

### 4.5 Anti-vacuity

A driver that never issues the transfer returns the poison → caught. One that
returns the TX buffer without a round trip would pass — this is the one
substitution T2 cannot detect, and it is covered instead by T3a: a genuine
transfer asserts CS, and the witness proves it electrically. **T2 and T3a are
jointly non-vacuous; neither is alone.**

---

## 5. T3 — chip-select witness under `SPI_HOLD_ON_CS | SPI_LOCK_ON`

### 5.1 Why the pair, not HOLD alone

`pdg_spi.c:413-419` rejects `SPI_HOLD_ON_CS` without `SPI_LOCK_ON` with
`-ENOTSUP`, deliberately (plan §11.1): HOLD alone returns success and then
releases the controller, letting a second caller select a second peripheral
while the first is still asserted — on a bus with MOSI and MISO shorted that is
real MISO contention, not a hypothetical. Design §7.2's HOLD-alone sequence and
plan §10.2's are superseded (spec §12.2). **The pair is mandatory.**

### 5.2 Configuration

Two `spi_config` objects at **distinct addresses**, because
`spi_context_configured()` compares by pointer:

- `cfg_hold`: `SPI_WORD_SET(8) | SPI_OP_MODE_MASTER | SPI_HOLD_ON_CS | SPI_LOCK_ON`
- `cfg_plain`: `SPI_WORD_SET(8) | SPI_OP_MODE_MASTER`

Both must be built with the upstream `SPI_CONFIG_DT()` initializer against an
SPI **device node** under the controller, never hand-populated. This section
originally specified only the operation flags, and the first implementation
hand-built `struct spi_config` with a hand-built `struct spi_cs_control` that
set `.gpio` and `.delay` but omitted `.cs_is_gpio` (`spi.h:243-276`), which
static initialization left false. `spi_cs_is_gpio()` (`spi.h:951-954`) reads
**only** that field, so `pdg_spi_cs_control_checked()` returned 0 at
`pdg_spi.c:244` without ever calling `gpio_pin_set_dt()`: the transfer echoed
byte-exactly, returned 0, and chip select was never asserted, so T3a measured
nothing. `SPI_CONFIG_DT()`/`SPI_CS_CONTROL_INIT()` (`spi.h:393-400`) set every
field of the upstream struct by construction, including fields added later.

Witness pin 3 is `GPIO_INPUT | GPIO_PULL_UP` for the whole of T3 and is not
reconfigured. Pin 2 is **not** touched by the application; SPI init parked it
`GPIO_OUTPUT_INACTIVE` = logically inactive = physically HIGH under
`GPIO_ACTIVE_LOW` (spec §6).

### 5.3 Sequence

| Step | Action | Witness expectation | Strength | Errno expectation |
| --- | --- | --- | --- | --- |
| 1 | read witness before anything | HIGH | weak (§5.4) | — |
| 2 | `spi_transceive(cfg_hold, m5_echo_tx, rx)` | — | — | **0** |
| 3 | **T3a**: read witness | **LOW** | **STRONG** | — |
| 3b | compare RX against §4.3 | exact echo | — | — |
| 4 | `spi_release(cfg_hold)` | — | — | **0** |
| 5 | **T3b**: read witness | **HIGH** | strong **as a transition from step 3** | — |
| 6 | **T3c**: `spi_release(cfg_hold)` again | HIGH unchanged | — | **`-EINVAL`** |
| 7 | **T3d**: `spi_transceive(cfg_plain, …)` | — | — | **0** |
| 8 | read witness | HIGH | weak | — |
| 9 | **T3e**: `spi_transceive(cfg_hold_nolock, …)` | **HIGH, unchanged** | **STRONG** | **`-ENOTSUP`** |

Step 3 is the load-bearing measurement of the entire milestone. Pin 3 holds the
shared node up through its own pull-up; the **only** thing that can bring it low
is pin 2 actively driving. Reading LOW there is direct electrical evidence that
the driver asserted chip select.

Step 9's witness clause is not decoration. `-ENOTSUP` alone is consistent with a
driver that asserted CS, discovered the flag problem, and returned an error
without deasserting. The witness staying HIGH is what proves the rejection at
`pdg_spi.c:413` happens before `pdg_spi_cs_control_checked` at `:505`.

### 5.4 The asymmetry of witness readings — stated explicitly

**A witness reading of LOW is strong. A witness reading of HIGH is weak.**

Pin 3 with a pull-up reads HIGH under all of:

1. pin 2 is an output driving high — the intended deasserted state;
2. pin 2 is a high-impedance input — pin 3's pull-up wins;
3. pin 2 is monitored by the firmware, hence an input — same;
4. the 2↔3 jumper has fallen off — pin 3's pull-up wins with nothing attached.

Case 4 is excluded by §3's gate, but only for the duration of the run. Cases 2
and 3 are **live during T4** (§6.4).

Consequence, and it is a correction to how spec §6.1 step 5 reads: a standalone
HIGH is never asserted as proof of deassert. What is asserted is the **LOW→HIGH
transition observed within a single process across the `spi_release()` call**
(steps 3 → 5). The transition is strong because case 1 is the only one of the
four that the preceding LOW is compatible with.

### 5.5 Negative control

**What would catch a driver that never touches CS at all?**

A loopback passes regardless of CS — the short is unconditional. Every case in
T6, and T2 itself, would be green. The single check that fails is **step 3**: a
driver that issues no CS assert leaves pin 2 parked high (or never configured),
pin 3's pull-up holds the node up, and the witness reads HIGH where LOW is
required.

Equivalently: **delete `pdg_spi_cs_control_checked` from
`pdg_spi_transceive` and step 3 is the only assertion in the entire M5 suite
that fails.** That is the definition of the negative control and it is why
`cs_lifecycle` cannot be satisfied by loopback evidence (spec §7.2 closing line,
§10 aggregate rule).

A weaker mutation — a driver that asserts CS but never deasserts — is caught by
step 5 rather than step 3, and by T3d blocking. Both mutations should be run as
a mutation control if the executor has budget; the spec does not require it and
I do not either, but the M4-era precedent (a test asserting a byte offset) makes
the case for at least reasoning about it.

---

## 6. T4 — fault injection: latch entry, `-EHOSTDOWN`, and recovery

The highest-value item in M5, and the only one that converts a four-milestone
source-shape claim into behavioural evidence.

### 6.1 FFI entry points — verified, not assumed

Read directly from `crates/pico-de-gallo-ffi/src/lib.rs` at the current HEAD.
All four exist with the signatures the test needs:

| Entry point | Line | Signature | Notes |
| --- | --- | --- | --- |
| `gallo_gpio_subscribe` | 2082 | `(*const PicoDeGallo, pin: u8, edge: u8) -> Status` | `edge`: 0=Rising, 1=Falling, **2=Any**; any other value is `Status::InvalidArgument` (`:2093-2101`) |
| `gallo_gpio_unsubscribe` | 2126 | `(*const PicoDeGallo, pin: u8) -> Status` | returns `Status::GpioPinNotMonitored` if not subscribed |
| `gallo_gpio_set_config` | 2023 | `(*const PicoDeGallo, pin: u8, direction: u8, pull: u8) -> Status` | `direction`: 0=Input, **1=Output**; `pull`: 0=None, 1=Up, 2=Down (`:2035-2053`) |
| `gallo_system_reset_subscriptions` | 726 | `(*const PicoDeGallo, out_reset: *mut u8) -> Status` | `out_reset` may be NULL |

The named-enum spellings `GalloGpioEdge_Any` (`:355-368`) and
`GalloGpioDirection_Output` (`:330-341`) exist and are ABI-stable; use them in
preference to bare integers. As with the reset image (spec §3), the success
enumerator must be read from the **generated** header, not guessed.

Every one of these returns `Status`, an `i32` where `Ok = 0` and all errors are
negative. Per CS-contract §13.17 (2026-08-17) the C consumer must switch on
`(enum Status)` with **no `default:` inside the switch** plus `-Werror=switch`,
and keep the unknown-value fallback *after* the switch — otherwise a future
status silently falls through.

### 6.2 Sequence, with the errno and witness expectation at every step

Preconditions: T2, T3 and T5 have all passed. Witness pin 3 is
`GPIO_INPUT | GPIO_PULL_UP`. `cfg_hold` is a live `spi_config` at a stable
address.

| # | Action | Expected result | Witness | Meaning |
| --- | --- | --- | --- | --- |
| 1 | `spi_transceive(cfg_hold, …)` | 0 | **LOW** (strong) | CS asserted and held |
| 2 | `gallo_gpio_subscribe(ctx, 2, Any)` | `Ok` | **not read** | firmware monitor takes pin 2 and sets it input |
| 3 | `spi_release(cfg_hold)` | **`-EBUSY`** | **not read** | `gpio_pin_set_dt` → `pdg_gpio_write_locked` (`pdg_gpio.c:150-186`) → `PinMonitored` → `-EBUSY`; `pdg_spi.c:627-638` latches, releases software ownership, retains `ctx->config` via `pdg_spi_unlock_defanged(ctx, true)` |
| 4 | `spi_transceive(cfg_plain, …)` | **`-EHOSTDOWN`** | **not read** | `pdg_spi.c:479-490`, taken after the lock and before set-config/CS/clocking |
| 5 | `gallo_gpio_unsubscribe(ctx, 2)` | `Ok` | **not read** | monitor releases the pin; pad remains a hardware input while firmware still tracks `ExplicitOutput` |
| 6 | `gallo_gpio_set_config(ctx, 2, Output, None)` | `Ok` | **not read** | restores the pad to output, reconciling pad and tracked mode |
| 7 | `spi_release(cfg_hold)` | **0** | **HIGH** | checked deassert succeeds; `pdg_spi.c:640-641` clears the latch |
| 8 | `spi_transceive(cfg_plain, …)` | **0** | HIGH | latch is genuinely clear, not merely reporting clear |
| 9 | `gallo_gpio_unsubscribe(ctx, 2)` | `GpioPinNotMonitored` | — | **belt-and-braces**: proves step 5 took effect and the run leaves no subscription |

Step 8 is not redundant with step 7. Step 7 returning 0 proves the release path
reported success; only step 8 proves `data->cs_fault` was actually cleared
rather than the release having taken a different branch.

### 6.3 Ordering decision, and the argument for it

**T4 runs last in the acceptance image. Acceptance runs before loopback.
Teardown runs last overall.**

The argument:

1. **T4 deliberately re-creates the exact orphaned pin-2 subscription that M5
   exists to clear** (plan R7 entry state). Any test placed after it inherits a
   fixture that may be in the entry-state hazard if T4 died partway. Placing it
   last means the set of tests that can be contaminated is empty.
2. **T4 is the only test that can leave the controller latched.** A latched
   controller returns `-EHOSTDOWN` from `pdg_spi.c:479` to *every* subsequent
   transfer, before reaching any other code path. Running T5 after T4 would
   silently convert T5's `-EMSGSIZE` assertion into an unreachable branch (§2).
   Running T2 after T4 would make the echo classification unreachable.
3. **T4 must not precede loopback under any circumstance.** Spec §6.2 already
   forbids loopback after an *abnormal* acceptance exit; I am strengthening that
   to: loopback runs only after acceptance exits zero **with** `M5_ACCEPTANCE_PASS`,
   which by construction means T4's step 7 succeeded and the witness read HIGH.
4. The counter-argument — run T4 first, so the fixture is clean for everything
   else — fails on point 2. A latch is not a state the other tests can be made
   robust against without weakening them.

### 6.4 The re-created pin-2 subscription hazard

**Statement of the hazard.** Between steps 2 and 5 the fixture is in exactly the
state M5's reset image exists to clean: pin 2 owned by a firmware monitor task,
with no software reset path. If the process dies in that window — crash, timeout
kill from the 420 s bounded runner (spec §9.2), operator Ctrl-C — the board is
left in the entry-state hazard and the next SPI init's
`gpio_pin_configure_dt(GPIO_OUTPUT_INACTIVE)` returns `-EBUSY`
(`pdg_spi.c:730-737`).

**Design response, four parts.**

1. **Minimise the window.** Steps 2 through 5 are three calls with no
   intervening logic, no sleeps, no retries and no I/O other than the three RPCs.
   The subscription is live for the shortest interval the test's semantics allow.
   Nothing may be inserted into that window "for diagnostics".
2. **Cleanup on every failure path.** Every assertion between steps 2 and 5 exits
   via a single `goto fault_cleanup:` label that issues
   `gallo_gpio_unsubscribe(ctx, 2)` and then `gallo_gpio_set_config(ctx, 2,
   Output, None)`, ignoring their return values, before returning nonzero. This
   does not help against process death, but it covers every *assertion* failure,
   which is the far more likely case.
3. **Teardown must assert, not assume.** The recovery/teardown image already
   calls `gallo_system_reset_subscriptions()` (spec §10 step 1) and reports the
   count. I require that the count be **asserted equal to 0** in a normal run and
   the value carried verbatim into `teardown.subscriptions_reset` in the
   aggregate JSON. A nonzero count after a supposedly-normal acceptance is direct
   evidence that T4's cleanup did not run, and must make `fault_latch` FAIL even
   if T4's own assertions all passed. **A reset count of 1 is a T4 defect
   report, not a housekeeping detail.**
4. **What the operator must check** after any abnormal acceptance exit, before
   anything else: run the reset image and read its count. Nonzero means T4 died
   in the window. Then run the recovery image; it requires SPI init ready (which
   proves pin 2 is no longer monitored) and witness HIGH. Only then restart from
   process 1 per spec §9.4. If reset itself does not answer within the process
   bound, power-cycle and record `power_cycle_occurred: true`.

**The jumper interaction.** Pin 2 is jumpered to pin 3. Subscribing sets pin 2 to
input, so from step 2 to step 6 the shared node is held solely by pin 3's
pull-up and sits **HIGH**. This is why steps 2–6 in §6.2 read the witness
**nowhere**: during that window a HIGH reading is case 2/3 of §5.4 and carries
no information about chip select. Asserting witness HIGH there would be a
test that passes against a driver which never deasserts — precisely the vacuity
class this document exists to eliminate. The first meaningful witness read after
step 1 is step 7, and it is meaningful only because step 6 restored pin 2 to an
output first.

**The `Any`-edge event stream.** An `Any`-edge subscription on a toggling node
produces a push stream on the `gpio/event` topic that nothing on the Zephyr side
consumes — D5 leaves interrupts as `-ENOSYS` and no bottom half drains the
topic. Assessment: **it cannot toggle.** Across the whole subscribed window the
node is held statically high by pin 3's pull-up; pin 3 is not reconfigured, and
the step-3 release attempt is *refused by the firmware* rather than executed, so
it produces no edge. Expected event count is zero or one (a single settling edge
at subscribe time). The design rule that makes this true is explicit and
normative: **pin 3 must not be reconfigured or driven at any point between
step 2 and step 5.** If a future revision needs to toggle pin 3 there, the
subscription must be changed to a single-edge subscription in the non-occurring
direction, or the assessment redone.

### 6.5 Distinguishing "correctly returned `-EHOSTDOWN`" from "something else errored"

`-EHOSTDOWN` is returned from **exactly one place** in the driver —
`pdg_spi.c:489`, in the latch branch. It appears nowhere else in `pdg_spi.c` or
`pdg_gpio.c`. There is therefore no ambiguity about *which* code path produced
it. The remaining question is whether the preconditions were what the test
believes, and that is settled by requiring step 3 to have returned exactly
`-EBUSY` first: the latch can only have been entered at `:628-631`.

Wrong outcomes at step 4 and what each means:

| Observed | Diagnosis | Severity |
| --- | --- | --- |
| **`0`** | The latch was never entered and the transfer proceeded with CS in an indeterminate state. **This is the exact defect plan §11.1's latch exists to prevent**: a failed deassert on one slave letting the next transfer to a different slave succeed and return success | **crash-class** — stop M5, report as a driver defect, do not weaken |
| **`-EBUSY`** | The latch was not entered; instead the CS *assert* failed inside this call. D10 propagation works but latch entry does not | spec-violation — stop M5 |
| **`-ENODEV`** | `data->ctx` is NULL — the device is not ready. Infrastructure, not a test result | INFRASTRUCTURE per spec §9.1 |
| **`-EINVAL`** | `config == NULL`, or a malformed buffer set. Test bug | test defect — fix the test, rerun |
| **`-EMSGSIZE`** | The test passed an oversized buffer. Test bug | test defect |
| **blocks forever** | The controller lock was never given back by the failed release, i.e. `pdg_spi_unlock_defanged` did not run at `:644`. This is the "if a caller never releases" hazard of plan §11.1 made live | the 420 s bounded runner fires → `INFRASTRUCTURE_TIMEOUT`. **Note this is the one case where a timeout is *not* purely infrastructure** — see §10, escalation E4 |

Wrong outcomes at step 3 (`spi_release`) and what each means:

| Observed | Diagnosis |
| --- | --- |
| **`0`** | The deassert reported success on a monitored pin. Either the subscription did not take (check step 2's status) or the GPIO bottom half is swallowing `PinMonitored`. Stop M5 |
| **`-EINVAL`** | `spi_context_configured()` did not match, i.e. the HOLD transfer did not retain `ctx->config`. That contradicts `retain_lock` at `pdg_spi.c:581`. Stop M5 |
| **`-EACCES`** | The status mapped was `GpioWrongDirection` rather than `PinMonitored`. Means the pin was left an input by something other than the monitor. Investigate before drawing any conclusion |
| **`-EHOSTDOWN`** | Impossible from `pdg_spi_release`, which has no latch branch. Indicates the test called `spi_transceive` where it meant `spi_release` |

### 6.6 What T4 does not prove

Carried verbatim from spec §6.2 and §8, and it must appear in
`explicitly_untested` in the aggregate JSON:

- that the `-EHOSTDOWN` branch issued **no invisible RPC** — no FFI-visible
  operation counter exists, and a witness cannot prove absence;
- **first-errno preservation** — subscribe induces only `-EBUSY`, and no FFI
  endpoint injects a chosen GPIO errno;
- **no second GPIO edge** — a duplicate deassert is electrically identical to the
  first and API-invisible;
- the **non-returning-RPC** row — deliberately not induced.

A driver fault shim or a `pdg_gpio_bottom_put` counter would cover the first
three. Both are prohibited (spec §8). I am not proposing them and I am not
softening the four rows to look covered.

---

## 7. T5 — payload boundary

### 7.1 What changed and why the test is now possible

`PDG_SPI_MAX_BUFFER` is `1013U` (`pdg_spi.c:226`), reached via `4096U` → `3072U`
→ `1013U`; the first two were guesses and both failed on hardware. Previously a
4096-byte transfer passed the local check — `4096 > 4096 - 0` is false — reached
the transport, and failed `-ECOMM` (`CommsFailed`) because the packet-buffer
budget covers payload **plus** postcard-rpc header and COBS framing. M4
acceptance C-18 called 4096 "a local boundary"; spec §12.1 records that as
factually wrong. T5 is the test that makes the corrected statement checkable.

### 7.2 The observable that distinguishes local rejection from transport failure

Three independent signals, all required together:

1. **The errno.** `-EMSGSIZE` is returned from **exactly one place** in the
   driver: `bufset_len_` at `pdg_spi.c:295`. A transport failure surfaces as
   `-ECOMM` via the status mapping in `common.c`. The two are never confusable.
2. **The witness stays HIGH throughout.** `bufset_len_` is called at `:438` and
   `:442`, before `k_malloc` (`:457`), before `spi_context_lock` (`:471`),
   before `pdg_spi_bottom_set_config` (`:494`) and before
   `pdg_spi_cs_control_checked(ctx, true, …)` (`:505`). A local rejection
   therefore issues **no chip-select edge**. Reading the witness immediately
   before and immediately after the rejected call, and requiring HIGH both
   times, is direct electrical evidence that no bus transaction began. This is
   the observable the spec asks for and it is stronger than the errno alone,
   because an errno can be produced by a driver that rejects *after* asserting.
3. **The log line.** `bufset_len_` emits `LOG_WRN("SPI %s buffers exceed maximum
   transfer size of %u bytes. …", direction, PDG_SPI_MAX_BUFFER)` at `:294`.
   Requiring the ceiling substring in that line pins the *constant actually
   compiled in*, not the one the source claims. The runner derives that number
   from the app's own `M5_T5A_LENGTH` rather than hardcoding it. A driver whose header said 1013
   while the TU compiled 4096 is caught here and nowhere else.

### 7.3 Cases

**Governing principle, added after execution falsified 3072.** Every T5 case is
either **(a)** a length *measured* to work, or **(b)** a length expected to be
rejected **locally** with `-EMSGSIZE` before any bus traffic. No T5 case puts an
unmeasured length on the wire. That makes the 1015-byte firmware hang window
unreachable by construction rather than by care. There is deliberately **no**
case at 1015, 1016 or 3072.

| ID | TX length | RX length | Expected | Witness before/after | Also required |
| --- | --- | --- | --- | --- | --- |
| T5a | 1013 | **none (TX-only)** | **0**, transfer completes | — | proves the boundary is inclusive (`>` at `:293`, not `>=`); the measured ceiling |
| T5b | 1014 | 0 | **`-EMSGSIZE`** | **HIGH / HIGH** | `LOG_WRN` containing `1013`; first length over the line |
| T5c | 4096 | 0 | **`-EMSGSIZE`** | **HIGH / HIGH** | not `-ECOMM`; the **after** arm of the controlled experiment whose **before** arm measured `-70` from the transport |
| T5d | 512 + 502 in two `spi_buf`s | 0 | **`-EMSGSIZE`** | HIGH / HIGH | proves the check is on the **accumulated** total (`*total_len` at `:293`), not per-buffer. Both fragments individually pass; their sum is 1014 |
| T5e | 512 | 512 | **0**, RX byte-exact | — | duplex **shape** check only — see below |

**T5a is TX-only by deliberate choice. Do not "improve" it into a full-duplex
test.** TX-only is the shape that was actually measured on hardware. The
**duplex ceiling has never been measured at any working length**: the only
duplex data point on record is 3072, which fails `-ECOMM`. Making T5a duplex
would put an unmeasured length on the wire and walk straight back into the
failure that produced this revision.

**T5e is a shape check, not a ceiling check.** It establishes that full duplex
works *at all*, at a length chosen for margin. It says nothing about where the
duplex ceiling lies, and must not be read as if it did. `spi_full_duplex_payload_ceiling`
stays in `explicitly_untested` alongside
`payload_ceiling_boundary_between_1013_and_1015` and
`firmware_dispatcher_hang_at_1015_byte_transfer`.

T5d is not in the spec. I am adding it because `bufset_len_`'s check is
`buf->len > PDG_SPI_MAX_BUFFER - *total_len`, an accumulation guard, and a
plausible refactor to a per-buffer check would pass T5a–T5c and fail only here.
It costs one extra call.

### 7.4 Anti-vacuity

- A driver that rejects **everything** with `-EMSGSIZE` passes T5b, T5c and T5d
  and fails T5a. T5a is load-bearing.
- A driver that accepts everything passes T5a and fails T5b–T5d.
- A driver that rejects at the right size but *after* asserting CS passes all
  four errno checks and fails the witness clause.
- A driver whose compiled constant is larger than the real transport limit
  passes T5a but fails T5b (the oversized call is not rejected locally and
  returns `-ECOMM` from the transport instead of `-EMSGSIZE`) — caught. This is
  exactly what happened at 4096 and again at 3072.

No single mutation passes all of T5.

---

## 8. T6 — upstream `spi_loopback`

### 8.1 Configuration and the two non-negotiable log markers

- `CONFIG_SPI_ASYNC=n` — mandatory. `pdg_spi.c:58-60` is a `BUILD_ASSERT`; with
  async enabled the image does not compile. The async cases at `spi.c:936`,
  `:971` and `:1019` are inside `#if (CONFIG_SPI_ASYNC)` and are **not built**.
- `CONFIG_SPI_LARGE_BUFFER_SIZE=512`. `spi.c:95` sets `BUF3_SIZE` from it and
  `spi.c:605-611` transfers exactly that many bytes **full duplex**. It must sit
  strictly below the ceiling, with real margin: the measured 1013 is a TX-only
  result and the duplex ceiling has never been measured. 512 is roughly half the
  measured TX-only ceiling and far from the 1015-byte length that hangs the
  firmware dispatcher. The upstream default of 8192 would fail `-EMSGSIZE` and be
  misreported as a data-path defect.
- **Both** `Testing loopback spec: SLOW` and `Testing loopback spec: FAST` must
  appear in the log. The string is printed by `spi_loopback_setup` at
  `spi.c:1159` from `spec_names[spec_idx]`; `spec_idx` is advanced by
  `run_after_suite` at `:1166-1169`. A malformed spec array — one child, or two
  children that resolve to the same node — yields a run that executes half the
  cases while still reporting all-green. **Requiring both markers is the only
  thing standing between a vacuous run and a reported pass.**

### 8.2 Expected case ledger — OBSERVED, superseding the source-derived one

**Observed on hardware: 41 PASS / 12 SKIP / 1 FAIL / 2 NOT BUILT.**

The 26/12/2 ledger below this note was derived by reading `spi.c` and is
**wrong**. Recorded so the next person does not re-derive it the same way, three
mistakes compounded:

1. It counted distinct test **functions in the source** rather than the
   **results ztest reports**. The suite reports 26 distinct tests in
   `spi_loopback` (19 pass + 6 skip + 1 fail) plus 2 in
   `spi_extra_api_features`, and both suites iterate twice.
2. It did not account for **ztest re-attempting a failing test**, which adds
   results that no reading of the source predicts.
3. It mis-attributed the split between the two suites.

The lesson is general: a ledger derived from source counts *what exists*, not
*what the harness reports*. Only an observed run settles it.

**The anti-vacuity property held exactly, and its check is retained.**
SKIP was **12 on the nose**, with **no pass in any expected-SKIP row** — all five
word sizes skipped on the driver's `-ENOTSUP`, `test_spi_deinit` on the missing
`miso-gpios`/`mosi-gpios`, `test_spi_hold_on_cs` on the unsupported
HOLD-without-LOCK pair, times two iterations. That was the disposition most
likely to hide a defect, and the runner asserts the count exactly.

**One expected FAIL: `test_spi_complete_multiple_timed`.** See §8.6.

The table below is retained for its per-case reasoning, which remains correct;
only its totals are superseded.

**Both suites run twice.** `test_main` calls
`ztest_run_all(NULL, false, ARRAY_SIZE(loopback_specs), 1)` at `spi.c:1210` with
`suite_iter = 2`. `ZTEST_SUITE(spi_loopback, …)` and
`ZTEST_SUITE(spi_extra_api_features, …)` at `:1178-1179` are both ordinary
suites, so both are iterated twice. `spi_extra_api_features` does **not** consult
`spec_idx` — it hard-codes `&spi_slow` / `&spi_fast` (`:1054-1055`, `:1078`) —
so its second iteration is a byte-identical repeat against SLOW. This
contradicts spec §7.2, which marks three rows "×1"; see escalation E1.

| Case | Line | Verdict | Why, from the source |
| --- | --- | --- | --- |
| `test_spi_complete_multiple` | 322 | **PASS ×2** | 18+36 bytes, two bufs each way; supported 8-bit synchronous path |
| `test_spi_complete_multiple_timed` | 341 | **PASS ×2** | `Z_TEST_SKIP_IFDEF(CONFIG_COVERAGE)` at `:343` does not fire; uses the measured multiplier from spec §7.1 |
| `test_spi_complete_loop_mode_0..3` | 455, 464, 473, 482 | **PASS ×2** each | modes accepted; see §4.1 on what a short can and cannot prove about mode |
| `test_spi_null_tx_buf` | 491 | **PASS ×2** | `flatten_tx_` zero-fills (`pdg_spi.c:308`) |
| `test_spi_rx_half_start` | 508 | **PASS ×2** | RX shorter than TX; `unflatten_rx_` copies the prefix |
| `test_spi_rx_half_end` | 524 | **PASS ×2** | skip guard at `:525` is `CONFIG_SPI_STM32_DMA \|\| CONFIG_DMA_SILABS_SIWX91X_GPDMA` — neither is set |
| `test_spi_rx_every_4` | 546 | **PASS ×2** | skip guard at `:547-551` names three DMA symbols, none set |
| `test_spi_rx_bigger_than_tx` | 573 | **PASS ×2** | skip guard at `:575` names two DMA symbols, neither set. Requires the RX tail beyond TX to read all-zero — satisfied because `flatten_tx_` memsets the flat buffer to 0 first (`pdg_spi.c:308`) and the short echoes it |
| `test_spi_complete_large_transfers` | 605 | **PASS ×2** | 512 bytes full duplex, deliberately well below the measured ceiling. **This is the case most likely to fail for a non-driver reason** — see escalation E2 |
| `test_spi_null_tx_buf_set` | 619 | **PASS ×2** | `bufset_len_` returns 0 for a NULL set (`pdg_spi.c:284-286`) |
| `test_spi_null_rx_buf_set` | 634 | **PASS ×2** | as above |
| `test_spi_null_tx_rx_buf_set` | 643 | **PASS ×2** | both NULL → `clock_len == 0` → early return 0 (`:453-455`) |
| `test_nop_nil_bufs` | 650 | **PASS ×2** | same early-return path |
| `test_spi_write_back` | 662 | **PASS ×2** | same buffer as TX and RX; flatten happens before the transfer, so aliasing is safe |
| `test_spi_same_buf_cmd` | 678 | **PASS ×2** | skip guard at `:679-684` names four DMA/nRF symbols, none set |
| `test_spi_word_size_7` | 737 | **SKIP ×2** | `spi_loopback_test_word_size` (`:710-736`) sets `SPI_WORD_SET(n)` and calls `spi_loopback_transceive`; `pdg_spi.c:385-388` returns `-ENOTSUP` for any word size ≠ 8; the wrapper converts `-EINVAL`/`-ENOTSUP` to `ztest_test_skip()` at `spi.c:259-263` |
| `test_spi_word_size_9` | 746 | **SKIP ×2** | same |
| `test_spi_word_size_16` | 761 | **SKIP ×2** | same |
| `test_spi_word_size_24` | 770 | **SKIP ×2** | same |
| `test_spi_word_size_32` | 785 | **SKIP ×2** | same |
| `test_spi_concurrent_transfer_same_spec` | 880 | **PASS ×2** | three threads; `spi_context_lock` serializes. Slow — three concurrent callers each doing four USB round trips per transfer |
| `test_spi_concurrent_transfer_different_spec` | 893 | **PASS ×2** | as above with distinct spec copies |
| `test_spi_deinit` | 907 | **SKIP ×2** | `:912-916` skips immediately because `zephyr,user` declares neither `miso-gpios` nor `mosi-gpios`. `device_deinit()` is **never reached**. It is in the `spi_loopback` suite, so it skips **twice** |
| `test_spi_async_call` | 971 | **NOT BUILT** | inside `#if (CONFIG_SPI_ASYNC)` at `:936` |
| `test_spi_transceive_cb` | 1019 | **NOT BUILT** | same conditional block |
| `test_spi_lock_release` | 1047 | **PASS ×2** | `SPI_LOCK_ON` on SLOW, transfer, `spi_release_dt`, then a FAST transfer. `pdg_spi.c:581` retains the lock on success and `pdg_spi_release` gives it back. `run_after_lock` (`:1171-1176`) clears the flags between iterations |
| `test_spi_hold_on_cs` | 1074 | **SKIP ×2** | `:1083` sets `SPI_HOLD_ON_CS` **without** `SPI_LOCK_ON`; `pdg_spi.c:413-419` returns `-ENOTSUP`; `:1087-1090` converts that to `ret = 0; goto early_exit`, and `:1129-1133` calls `ztest_test_skip()`. The supported pair is tested by T3, not here |

**SUPERSEDED BY EXECUTION.** These totals were source-derived and are wrong; the
observed run is **41 PASS / 12 SKIP / 1 FAIL / 2 NOT BUILT** (see the
observation-backed ledger at the head of this section). The SKIP total of 12 is
the one figure the source derivation got right, and it is the anti-vacuity
check the runner asserts exactly.

### 8.3 Real defect versus expected environmental skip

The rule, and it is the whole point of agreeing the ledger in advance:

- **A skip in a row marked PASS is a defect.** The only skip route into those
  rows is `spi.c:259-263` converting `-EINVAL`/`-ENOTSUP` from the driver — which
  for an 8-bit synchronous master-mode transfer means one of `pdg_spi.c`'s
  `-ENOTSUP` guards fired when it should not have.
- **A pass in a row marked SKIP is a defect** — most sharply for the five word
  sizes: passing means `pdg_spi.c:385` did not reject a non-8-bit word, and the
  bytes compared equal only because a short echoes anything.
- **A skip in a row marked SKIP is expected and carries no information.** It must
  not be reported as coverage.
- **`ztest_verify_all_test_suites_ran()`** at `spi.c:1216` catches a suite that
  never ran at all; its failure is an infrastructure or overlay defect, not a
  driver defect.
- Any disposition not in the table **stops M5** (spec §7.2 closing line). It is
  not reclassified, not retried, and not annotated away.

### 8.4 Anti-vacuity

T6's honest weakness: **a loopback passes regardless of what chip select does**
(plan §10.1). Twenty-six green cases are compatible with a driver that never
touches CS. T6 therefore proves *data-path* conformance only, and the aggregate
rule in spec §10 — that a missing or contradictory CS witness makes overall
INCONCLUSIVE even if every loopback byte matches — is the correct treatment.

Its genuine value is coverage T3/T5 cannot reach: scatter-gather buffer
topologies (`rx_half_end`, `rx_every_4`, `rx_bigger_than_tx`), aliased TX/RX
buffers (`write_back`, `same_buf_cmd`), zero-length and NULL sets, concurrency
under the controller lock, and word-size rejection. Eight of the thirteen passing
cases exercise `flatten_tx_`/`unflatten_rx_` offset arithmetic that nothing else
in M5 touches, and an off-by-one there fails them and nothing else.

### 7.5 The ceiling sweep cannot converge as written — recorded, not fixed

Executed on hardware. Results, from two byte-identical consecutive runs:

- largest known-good **TX-only** transfer: **1013 bytes**;
- smallest clean failure: **1017 bytes** → `-ECOMM`;
- every failure was `-ECOMM`, **never** `-EMSGSIZE`, so `LIMITED_BY = transport`
  unambiguously and the compiled constant was never the limiter;
- **1014 and 1016 were never probed. DUPLEX was never measured at all.**

**The binary search cannot finish.** Probe 11 lands inside a hang window: a
1015-byte TX-only transfer never returns and wedges the firmware dispatcher
device-wide. Any search that narrows on the boundary between 1013 and 1017 must
step into it, so the sweep terminates the run rather than converging. This is a
property of the search, not a flake.

Two prerequisites before the sweep can be finished, in order:

1. **Investigate the hang** (root cause is in `crates/`, out of M5 scope). Until
   it is understood, no probe strategy is safe near the boundary.
2. **Give the sweep per-probe containment** — a bounded per-probe timeout, or a
   descending scan that starts from the known-good 1013 and never probes above
   the last clean failure, so it cannot re-enter the window.

Until both are done, the ceiling is a measured lower bound (1013) rather than a
located boundary, and **full duplex remains entirely unmeasured** — the only
duplex data point is 3072, which fails. That gap must be stated wherever the
ceiling is quoted; it must not be glossed as "1013 works".

### 8.6 Expected failure: `test_spi_complete_multiple_timed`

```
START - test_spi_complete_multiple_timed
Transfer took 0 us vs theoretical minimum 432 us
    Assertion failed at .../spi_loopback/src/spi.c:406:
    (time_spent_us >= minimum_transfer_time_us is false)
```

**Known-unrunnable on this target. Not a driver defect. Kept visible.**

Upstream `spi.c` measures with the Zephyr clock, which does not advance on
`native_sim` while the host thread is blocked inside a USB call. Our acceptance
app moved to `clock_gettime(CLOCK_MONOTONIC)`; `spi.c` did not, and patching
upstream is out of scope.

Two facts make the classification certain rather than convenient:

1. `spi.c:406` asserts `time_spent_us >= minimum_transfer_time_us` — a **lower**
   bound. `CONFIG_SPI_IDEAL_TRANSFER_DURATION_SCALING` bounds only the **upper**
   limit, so **no value of the multiplier can affect this assertion**. Raising it
   would be both useless and a softening.
2. It fails on SLOW and passes on FAST purely because SLOW's theoretical minimum
   (432 µs) is larger. That is **structural, not flaky** — ztest's `FLAKY` label
   is misleading here.

The runner carries it on an explicit expected-failure list: it is reported by
name on every run with this reasoning, and **any failure not on that list makes
the phase FAIL**. It must never be silenced — an expected failure that stops
being printed is how a real regression hides. Recorded in
`explicitly_untested` as `upstream_timed_transfer_bounds_on_native_sim`.

### 8.5 Timing multiplier — a constraint the spec leaves open

**RESOLVED, and the host-clock path is validated on hardware.** Observed:
SLOW p50 **2322 µs**, FAST p50 **1729 µs**, selected multiplier **47** — inside
the required 1–256 band, with FAST binding exactly as predicted. Timing is
therefore a real measurement on this target, not a `NOT_MEASURABLE` fallback.
The paragraph below records why the Zephyr clock could not produce it.

**Correction from execution: the Zephyr clock cannot measure this at all on
`native_sim`.** Simulated time does not advance while the host thread blocks
inside a USB call, so `k_cycle_get_32()` around a transfer reported
p50 = p95 = p99 = max = 0 µs across 25 real, correctly-echoing transfers at each
frequency. The formula below is unsatisfiable with that clock, and the derived
multiplier of 0 aborted the run before loopback. Elapsed time is now taken from
the host monotonic clock; timing is non-gating and reports `NOT_MEASURABLE`
rather than failing when a multiplier cannot be derived. Note the corollary:
upstream's own `test_spi_complete_multiple_timed` uses the Zephyr clock and
therefore **passes vacuously** on this target — it belongs in
`explicitly_untested`, not in the PASS column of §8.2.

Spec §7.1's formula is `ceil((1.25 * observed_max_us) / theoretical_minimum_us)`
measured over ≥20 healthy 54-byte transfers at SLOW and FAST. Two things must be
pinned that it does not pin:

1. **The measured frequencies must be exactly the `spi-max-frequency` values
   declared for `slow@0` and `fast@0` in `spi_loopback.overlay`.** Measuring at
   any other operating point derives the multiplier from the wrong ratio. The
   acceptance image uses two `spi_config`s whose `.frequency` fields are those
   two literals.
2. **The multiplier is USB-latency-dominated and therefore scales inversely with
   frequency.** Transfer wall time here is four USB round trips regardless of
   clock rate, while `theoretical_minimum_us = 54 * 8 / f`. Doubling the FAST
   frequency roughly doubles the required multiplier. FAST is what sets the
   value; SLOW will not bind. If the chosen FAST frequency drives the required
   multiplier above 256, spec §7.1 says stop for reliability review — the correct
   remedy is a lower `fast@0` frequency in the overlay, **not** a larger
   multiplier. Recommend `fast@0` at or below 8 MHz, which puts the requirement
   in the spec's expected 64–128 band.

Never raise the multiplier to make a failed run green (spec §7.1). If the
measured value and the run disagree, the measurement is stale — re-measure.

---

## 9. Anti-vacuity audit and self-grade

### 9.1 Tests I considered and deleted

Recorded so the next pass does not re-propose them.

- **"Assert `PDG_SPI_MAX_BUFFER == <ceiling>` in the test source."** A broken driver
  passes trivially: the constant would be re-read from the same header the driver
  uses, proving only that a macro equals itself. This is the same class as the
  M4-era latch test that asserted a byte offset between two source constructs
  (plan §11.5). Replaced by T5's `LOG_WRN` substring check, which pins the value
  actually compiled into the translation unit.
- **"Assert `spi_release()` returns 0 in the normal path."** Passes against a
  `pdg_spi_release` that is `return 0;`. Only meaningful when paired with the
  witness transition (T3b) and the second-release rejection (T3c), so it is not a
  separate test.
- **"Read the witness after T3e's `-ENOTSUP` and expect HIGH."** Retained, but
  only because the *preceding* state is a known HIGH and the assertion is that it
  is **unchanged** across a call that must issue no edge. As a standalone HIGH
  assertion it would be weak (§5.4).

### 9.2 Coverage grade

Graded on the same three-way scale M3's tester used (plan §10.5: ~20% proved,
~35% source-shape, ~45% zero). Denominator is the 17 test IDs in §1, weighted by
the property each is meant to establish rather than by line count.

| Class | Fraction | Contents |
| --- | --- | --- |
| **Genuinely proved by execution** | **~60%** | T0, T1a, T1b (electrical, strong readings only); T2 exact-echo classification; T3a (the strong LOW), T3b transition, T3c, T3d, T3e; T4 steps 1–8 in full; T5a–T5e; T6's 41 pass results |
| **Weak — catches deletion, blind to present-but-wrong** | **~25%** | Every witness **HIGH** assertion (§5.4) — necessary, not sufficient, and only meaningful as part of a transition; T1a.2/T1b.2; T3 step 1 and step 8; the mode-0..3 sweep, which proves acceptance-and-echo but is **mode-blind** (§4.1); T6's 12 skip results, which confirm rejection happened but not that the rejection was for the right reason; T7's `acknowledged` fields, which report what was commanded rather than what is |
| **Zero** | **~15%** | The four rows of spec §8's ledger — first-errno preservation, no-second-GPIO-edge, `-EHOSTDOWN`-before-any-I/O, and the non-returning-RPC row — all of which need instrumentation that is prohibited; T1c, which is a fixture assertion about silicon rather than driver coverage and must not be counted toward the driver; the CPOL/CPHA wire mapping, which no test on a MOSI↔MISO short can reach |

**Reasoning for the jump from M3's ~20% to ~60%.** It is not that this suite is
three times better designed. It is that M5 is the first milestone that *runs*,
and execution converts a large block of previously source-shape assertions —
latch entry, `-EHOSTDOWN`, checked-deassert propagation, the payload ceiling, the
CS edges themselves — into behavioural evidence in a single step. The residual
25% weak and 15% zero are stubborn precisely because they are absence proofs and
mode proofs, and neither the fixture nor the permitted instrumentation can reach
them. I do not expect a later milestone to move them without the two prohibited
capabilities named in spec §8.

I am explicitly **not** claiming plan §11.5's four properties become behavioural
in M5. Spec §8 already corrects that, and this grade agrees with the spec's
correction rather than the plan's original expectation.

---

## 10. Escalations — things I believe are untestable as written, or wrong

Raised rather than quietly worked around, per the brief.

### E1 — Spec §7.2's "×1" verdicts are wrong; both suites run twice

**Severity: spec-violation (documentation), with a live risk of a false FAIL.**

Spec §7.2 marks three rows "once": `deinit` SKIP once, `lock/release` PASS once,
`HOLD generic` SKIP once. `test_main` calls
`ztest_run_all(NULL, false, ARRAY_SIZE(loopback_specs), 1)` at `spi.c:1210`,
where `ARRAY_SIZE(loopback_specs)` is 2 (`spi.c:43`) and is the `suite_iter`
argument. Both `ZTEST_SUITE`s at `spi.c:1178-1179` are iterated twice.
`test_spi_deinit` is a member of `spi_loopback` (`spi.c:907`), so it skips twice.
`spi_extra_api_features` does not consult `spec_idx` and hard-codes the specs, so
its two iterations are identical repeats against SLOW.

**Consequence if not corrected:** an executor holding the spec's ledger sees
`lock/release` pass twice where the contract said once and must, per §7.2's
"any deviation stops M5", halt a healthy run. The correct dispositions are
`deinit` SKIP ×2, `lock/release` PASS ×2, `HOLD generic` SKIP ×2, and the
per-iteration totals in §8.2. I have written §8.2 to the source, not to the spec.

**Requested amendment:** correct the three "once" cells in spec §7.2 to "×2" and
add a sentence recording that `spi_extra_api_features` repeats against SLOW
because it does not consult `spec_idx`.

### E2 — `CONFIG_HEAP_MEM_POOL_SIZE` additive default of 6144 is very likely insufficient

**Severity: wrong-result — a real allocator failure misreported as a data-path defect.**

Spec §2.1 and §7.1 set the production SPI Kconfig additive heap default to
**RESOLVED — no value change was needed.** The module's Kconfig already
defaulted to 8192, not the 6144 this escalation assumed; E2 read the upstream
test's default. Only the help text was wrong. The original reasoning is kept
below for the record. It assumed **exactly 6144** because full duplex was then
believed to allocate two 3072-byte buffers
(`pdg_spi.c:457`, `:464`). Zephyr's `k_malloc` heap carries per-chunk metadata
and alignment padding, and the heap itself has a header. Two allocations of
exactly half the heap will, on most `sys_heap` configurations, fail the second.

The observable failure is `-ENOMEM` logged at `pdg_spi.c:466` and returned to
`spi_transceive`. `spi_loopback`'s wrapper at `spi.c:259-263` converts only
`-EINVAL` and `-ENOTSUP` to a skip, so `-ENOMEM` falls through to
`zassert_ok(ret, "SPI transceive failed, code %d", ret)` at `:264` and
`test_spi_complete_large_transfers` **fails** — presenting as a data-path defect
in the single most conspicuous case in the suite.

I cannot confirm the exact overhead without building, which I am forbidden from
doing. But the margin as specified is zero, which is not a defensible design
point for an allocator.

**Requested amendment:** raise the additive default to **8192** and adjust the
help text accordingly. If the
maintainer prefers to keep 6144, then T6's large-transfer case must be run first
and an `-ENOMEM` there explicitly classified as a **configuration** failure
rather than a data-path failure, so it is not misreported. I recommend the former;
it is one digit and removes the ambiguity entirely.

### E3 — Spec §6.1 step 5's standalone witness-HIGH assertion is weak as written

**Severity: degradation — the criterion as written can be satisfied by a driver that never deasserts.**

Spec §6.1 step 5 reads "witness returns HIGH". Pin 3 with a pull-up reads HIGH
under four distinct physical situations (§5.4), only one of which is the intended
deassert. In particular a driver that leaves pin 2 as a high-impedance input, or
one whose deassert silently no-ops, produces HIGH.

I have **not** weakened this. §5.3 keeps the assertion and strengthens it: the
load-bearing observation is the **LOW→HIGH transition across `spi_release()`
within a single process** (step 3 → step 5), which the preceding strong LOW makes
sound. I am recording the ambiguity because spec §10's aggregate rule treats CS
witness evidence as decisive, and a reader implementing §6.1 literally would ship
the weak form.

**Requested amendment:** restate §6.1 step 5 as "witness transitions from LOW to
HIGH across the release, both readings taken in the same process", and add a
sentence noting that a standalone HIGH is necessary but not sufficient.

### E4 — The bounded runner classifies a real driver defect as infrastructure

**Severity: wrong-result (classification).**

Spec §9.2 states "Timeout is **infrastructure failure**, never a test result",
and §1.1 repeats it. That is right for board-absent, board-busy and permission
failures. It is **wrong** for one specific case that T4 can produce.

Plan §11.1 documents that if a caller never releases, "the line stays asserted,
`ctx->config`/`ctx->owner` stay set, and any transceive with a different config
**blocks forever**. No timeout, no watchdog, not detectable across process
death." T4 step 4 is exactly a transceive with a different config while a HOLD is
outstanding. If `pdg_spi_release` failed to reach `pdg_spi_unlock_defanged` at
`pdg_spi.c:644`, step 4 blocks on the controller semaphore and the 420 s runner
kills it. The spec then records `INFRASTRUCTURE_TIMEOUT` and the run is retried —
**hiding a genuine software-lock leak behind an infrastructure verdict**, and
retrying it forever.

**Requested amendment:** add a carve-out. A timeout **in the acceptance phase,
at or after T4 step 3**, is classified `FAIL` with the note "possible controller
lock leak; see plan §11.1", not `INFRASTRUCTURE_TIMEOUT`, and is **not retried**.
Timeouts in every other phase keep the existing classification. The distinguisher
is available to the runner: the acceptance log will contain T4's step-3
`-EBUSY` line before the timeout, and will not contain a step-4 result line.

### E5 — Spec §7.1's measured frequencies are unbound

**Severity: degradation.**

§7.1 says acceptance measures "at SLOW and FAST" without tying those to the
`spi-max-frequency` values in `spi_loopback.overlay`. A multiplier derived at a
different operating point does not apply to the suite that consumes it, and the
error is silent — it manifests only as an intermittent timing failure much later.
§8.5 pins it; the spec should too, together with the observation that the
multiplier is USB-latency-dominated and that the remedy for exceeding 256 is a
lower FAST frequency rather than a larger multiplier.

### E6 — `explicitly_untested` should gain a fifth entry

**Severity: completeness.**

The aggregate JSON's `explicitly_untested` list has four entries. §4.1 identifies
a fifth property that M5 cannot reach and that a reader would otherwise assume
the mode-0..3 sweep covers: **a MOSI↔MISO short is mode-blind, so CPOL/CPHA wire
mapping is not verified.** Recommend adding `cpol_cpha_wire_mapping` so the sweep
is not read as more than it is. This is an addition to an explicitly-untested
list, so it cannot weaken any criterion.

---

## 11. Surface NOT attacked

Stated explicitly so the next pass knows what remains untested.

**Out of scope by construction:**

- **I2C child, MFD parent init, GPIO child in isolation.** M5's images exercise
  the parent only as a context provider and the GPIO child only as the CS and
  witness path. `pdg_i2c.c` is not loaded by any M5 image.
- **The `gpio/event` topic and any push path.** D5 leaves interrupts `-ENOSYS`;
  §6.4 argues the T4 subscription window is quiescent, but the *drain* behaviour
  of a topic nothing consumes is not exercised and would need a deliberately
  toggling node.
- **All four rows of spec §8's ledger.** First-errno preservation, no-second-edge,
  `-EHOSTDOWN`-before-any-I/O, non-returning-RPC. Prohibited instrumentation.
- **CPOL/CPHA wire correctness** (§4.1, E6).
- **`device_deinit()`.** `spi.c:912-916` skips before reaching it; `pdg_spi.c`
  has no deinit entry in `pdg_spi_driver_api` (`:649-652`).
- **Async and RTIO paths.** `BUILD_ASSERT`ed off at `pdg_spi.c:58-63`; the
  assertions themselves are compile-time and are proved only by the fact that the
  image builds with `CONFIG_SPI_ASYNC=n`. A build with it **on**, expected to
  fail with the named message, would prove the assert fires — not run here.

**In scope but deliberately not attacked this pass:**

- **Concurrency beyond the three threads upstream spawns.** No test drives a
  HOLD+LOCK from one thread while a second thread transceives with a different
  config — the deadlock plan §11.1 documents. It is documented, unfixed, and
  deliberately not induced, because the failure mode is an unbounded block
  indistinguishable from infrastructure loss (the same reasoning spec §8 gives
  for the non-returning-RPC row).
- **Multiple chip selects.** Every M5 overlay declares one `cs-gpios` entry.
  `pdg_spi_init`'s loop at `:712-750` and the partial-failure residue path at
  `:738-746` are never exercised with `idx > 0`.
- **Duplicate CS indices**, permitted deliberately per CS-contract §8.17.
- **`-ENOMEM` on the `k_malloc` paths** (`:457-468`) — not induced; see E2, where
  it may be induced accidentally.
- **`bufset_len_`'s `count != 0 && buffers == NULL` guard** (`:287-290`).
  A one-line addition to the acceptance app would cover it; not required by the
  spec and not added.
- **Init-time priority inversion** (`pdg_spi.c:715-726`) — requires a deliberately
  wrong `CONFIG_SPI_PICO_DE_GALLO_INIT_PRIORITY`, which is a build variant M5's
  file set does not authorise.
- **Two boards attached** — the R11 / 2026-07-29 ambiguous-target class. One
  board is attached; spec §9.1 requires exactly one `045e:067d`, which turns the
  two-board case into an infrastructure abort rather than a test.
- **Mutation controls.** §5.5 reasons about two mutations of
  `pdg_spi_cs_control_checked` but does not require running them. The M2 precedent
  (CS-contract §8.13, where reintroducing "first match" failed 3 of 7 tests) shows
  the value; the M5 file set does not authorise a driver edit, so a mutation
  control would have to be run and reverted outside the committed set. Flagged for
  a maintainer decision, not assumed.
