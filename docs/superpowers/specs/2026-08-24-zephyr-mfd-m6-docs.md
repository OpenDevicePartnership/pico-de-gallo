# Zephyr MFD restructure M6 — documentation consolidation specification

Date: 2026-08-24
Branch baseline: `zephyr` at `80b9139134e8`
Milestone: M6 — reconcile the SP1 record and consolidate user documentation
Status: ready for documentation implementation

---

## 1. Context and authoritative ruling

M1–M5 shipped the MFD parent, nested I2C/SPI children, the GPIO controller,
standard `cs-gpios`, checked chip-select handling, HOLD/LOCK support, and the M5
hardware suite. M6 changes documentation only. It must preserve the dated M1–M5
record while making its early summaries point at the later corrections.

### 1.1 Transfer-ceiling contradiction — ruling

The M5 acceptance artifact is authoritative. At the start of M6, its 512-byte
duplex success evidence disagrees with the comments under
`PDG_SPI_MAX_BUFFER` and the SPI book chapter, both of which incorrectly say no
duplex length was measured working. M6 must bring the source comment, Zephyr
CHANGELOG, SPI binding/Kconfig, README, and book into agreement with this
contract:

- `1013` is the largest **TX-only** length observed to work, twice.
- the exact TX-only boundary is unresolved between 1013 and 1015; 1014 was not
  probed and 1015 hangs;
- full duplex is known to work at 512 and fail at 3072, but its ceiling was not
  measured and duplex at 1013 is unverified;
- `1013` is a local containment limit, not a derived protocol limit or a fix.

Plan §12.1, beginning “Every estimate”, is the loose account. Its statement that a binary sweep
measured the “true ceiling” at approximately 1013 must be superseded. No M6 text
may call 1013 a measured duplex ceiling, a true boundary, or generally usable
payload. This same claim must appear in the plan correction, AGENTS regression
row, Zephyr CHANGELOG, binding, README, and book.

## 2. Scope and prohibitions

Authorized edits are documentation only:

- `docs/superpowers/plans/2026-08-17-zephyr-mfd-restructure.md`
- this specification
- `AGENTS.md`
- `zephyr/CHANGELOG.md`
- `zephyr/README.md`
- `book/src/**`
- `zephyr/dts/bindings/**/*.yaml`, restricted to `description:` prose
- `zephyr/drivers/**/Kconfig`, restricted to `help` prose
- the `PDG_SPI_MAX_BUFFER` comment block in `zephyr/drivers/spi/pdg_spi.c`

Hard prohibitions:

- **No code changes.** Correct a doc when code is authoritative. If a doc is
  right and code is wrong, escalate to the maintainer; do not fix code.
- Nothing under `crates/`; no package version or `Cargo.lock` change.
- Do not touch hardware: no `gallo_*` MCP tool, `probe-rs`, built binary, M5
  acceptance run, or `--ceiling-sweep`.
- No tree-wide checkout/restore/reset/clean, rebase, unrelated amend, repo-wide
  format, push, or force-push.
- Do not edit ignored `book/book/`; `book/.gitignore:1` confirms it is output.
- Normalize every edited text file to LF with `dos2unix`.

`git diff --stat e7087bdc4eee~1..HEAD -- crates/` produced no output. Therefore
SP1 changed no crate and no per-crate CHANGELOG needs an entry. M6 must not
propose one.

## 3. Plan reconciliation

Preserve §§8–12 verbatim in substance. Use additive forward pointers matching
§4's existing “Corrected after M1” precedent.

### 3.1 §1 milestone table (the table beginning “| # | Milestone”)

Insert immediately after the table (after current line 39):

> **Corrected after M1–M5 — this table is the original sequencing summary, not
> the final acceptance record.** M1 could not make all four samples build and
> its parent was disabled by default; the achieved gate was two clean builds,
> two failures matching the measured baseline, and an enabled-parent probe
> (§8.1). M2 was not a pure refactor: it changed initialization failure coupling,
> failure location, and worst-case boot latency (§9.1). M4 was compile-only, so
> chip-select edges could only be witnessed in M5 (§11.3). M5's data-path and
> chip-select acceptance passed, but its overall verdict was **FAIL** because it
> found three defects, including a crash-class firmware-dispatcher hang; the
> upstream loopback result was 41 PASS / 12 SKIP / 1 structurally unrunnable
> FAIL / 2 NOT BUILT, not a clean suite pass (§12 and `zephyr/CHANGELOG.md`).

This note resolves the false M1 gate at `:34`, “pure refactor” at `:35`, M4 edge
witness at `:37`, and clean M5 loopback implication at `:38` without rewriting
the historical table.

### 3.2 §2 R5 sample table (the table beginning “| Sample | Status”)

Insert after current line 105:

> **Corrected after M5 — this table remains a compile-time sample inventory.**
> `i2c_bridge` was not exercised on hardware by SP1, so its target state remains
> unverified. The two IS31 samples still do not link, and `spi_nor_id` still has
> no attached NOR. Runtime evidence instead came from the dedicated M5 fixtures;
> it must not be summarized as “all four samples pass.”

The table itself is otherwise accurate; do not invent runtime results.

### 3.3 §2 R7 board state (the paragraph beginning “Dirty from #104 acceptance”)

Insert after current line 125:

> **Corrected after M3 and M5 — the original recovery claim and residue premise
> were stale.** `gallo_system_reset_subscriptions()` exists and is the software
> cleanup path for orphaned subscriptions (§10.4, §11.3). M5 measured
> `M5_RESET_COUNT=0`, so the expected orphaned pin-2 subscription was not present
> (§12.5). Separately, after a different 1015-byte SPI dispatcher wedge the
> device was observed to resume after USB re-enumeration (`usbipd detach`
> followed by attach on Windows/WSL). This observation does not establish the
> recovery mechanism or generalize to the earlier GPIO-wait trigger (§12.2).
> Keep resets explicit in acceptance setup because their necessity is
> conditional on actual board state.

Do not delete the original board-state report.

### 3.4 §3 file inventory (`:129-150`)

Insert after current line 132, before the table:

> **Corrected after M1–M5 — this inventory was an indicative planning baseline,
> not an exhaustive allow-list.** Every implementation milestone discovered
> justified parity, build, test, or specification files omitted here; see
> §§8.1, 9.3, 10.4, 11.3, and 12.5. The milestone specifications and shipped
> diffs are authoritative for the files actually changed.

This expressly demotes “Anything not listed is out of scope” without erasing it.

### 3.5 §4 verification (`:154-206`)

The TU grep at current line 189 is already corrected to
`pdg_[a-z0-9_]*\.c`; no further grep edit is needed. Insert after current line
199 (“All four samples must build”):

> **Corrected after M1 — “all four” means reproduce the measured baseline, not
> four successful links.** Build the two viable samples successfully and verify
> the two IS31 samples fail identically to baseline; use an enabling overlay to
> prove any otherwise-disabled translation unit is compiled (§8.1). Do not turn
> a pre-existing missing upstream device driver into an SP1 regression.

The existing non-vacuity block and corrected board target remain accurate.

### 3.6 §7 definition of done (the original branch-level checklist)

Preserve the original checklist verbatim and append a “Final evidence (after
M4–M6)” checklist beneath it:

- [ ] M1–M6 committed on `zephyr`, nothing pushed *(M6 pending until these docs land)*
- [ ] The original all-four-samples criterion needs a maintainer ruling; the
  measured result is two clean builds and two failures identical to baseline
- [ ] The original hardware criterion needs a maintainer ruling; M5 observed CS
  hold/release and exact loopback data, but its overall verdict remains **FAIL**
- [x] `cs-gpio-indices` has zero hits under `zephyr/`, `book/`, and `crates/`
- [ ] `cargo test --workspace --locked` remains green *(not rerun by documentation-only M6)*
- [ ] `mdbook build book` clean *(M6 gate)*
- [x] No `[package].version`, `Cargo.lock`, wire-protocol, firmware, or `crates/` change

Insert immediately after the list:

> **Corrected after M4–M5 — repository-wide absence was never a valid gate.**
> Historical plans, specifications, and AGENTS.md legitimately name the deleted
> property. The scoped zero-hit gate above is the one M4 verified (§11.3).
> Likewise, “the loopback suite passes” is not the outcome: see §12 and the
> Zephyr CHANGELOG for the mixed upstream result and the crash-class finding.

Leave R8 where it is. Moving R8 or R9–R12 would create a noisy historical rewrite;
the forward notes make navigation coherent without risking changed substance.

### 3.7 §12.1 transfer account (the paragraph beginning “Every estimate”)

Insert after current line 740:

> **Corrected during M6 — “true ceiling” and “binary sweep” overstate the
> evidence.** The shipped source comment and M5 acceptance specification are
> authoritative: 1013 is the largest **TX-only** length observed to work, not a
> measured duplex ceiling. The exact TX-only boundary remains unresolved between
> 1013 and 1015 because 1014 was not probed and 1015 hangs. Full duplex is known
> to work at 512 and fail at 3072; duplex at 1013 is unverified. Therefore 1013
> is a conservative local containment limit, not a derived or generally usable
> protocol payload limit. Do not run the non-converging `--ceiling-sweep`.

This note is mandatory; it resolves the principal contradiction rather than
propagating §12.1's loose wording.

## 4. AGENTS.md §13.17

### 4.1 Additive clarification to the 2026-06-03 GPIO-wait row

In `AGENTS.md` §13.17, the 2026-06-03 GPIO-wait row, retain all existing text
and append:

> A later, different dispatcher wedge (2026-08-19, SPI transfer framing) was
> observed to recover after USB re-enumeration rather than a power cycle; that
> observation was never tested against this row's GPIO-wait trigger, so the
> power-cycle assumption here stands unrefuted rather than confirmed.

Do not rewrite the historical diagnosis or fix.

### 4.2 New 2026-08-19 row

Append this one-line four-column row after the 2026-08-17 row:

The row must be dated 2026-08-19 and distinguish three outcomes: the MFD/
`cs-gpios` restructure is fixed; `PDG_SPI_MAX_BUFFER = 1013` contains the Zephyr
path only; the underlying framing/dispatcher defect remains open. It must name
the directionality trap and report USB re-enumeration only as the observed SPI
recovery procedure, without generalizing it.

Keep the row dense and specific like its neighbours. Do not say a duplex sweep found
1013.

## 5. Zephyr CHANGELOG and README

### 5.1 `zephyr/CHANGELOG.md`

The file already has a valid `[Unreleased]` section and substantive coverage of
all four required outcomes:

- topology/selector break: `:100-156`;
- GPIO controller: `:213-256`;
- HOLD/LOCK: `:175-180`;
- 1013 containment and unresolved ceiling: `:34-98`.

Do **not** duplicate these as a new SP1 umbrella entry or add an `SP1 summary`
heading. Keep a Changelog categories win: unique content belongs under
`Added`, `Changed`, `Removed`, `Fixed`, or `Security`, while redundant summary
content must be deleted. This is the maintainer's ruling after M6 review. Make
only these genuine corrections:

1. At `:87-92`, preserve the authoritative caveat exactly in substance. If prose
   is normalized, use: “1013 is the largest TX-only length measured to work;
   full duplex is proven at 512 only, fails at 3072, and is unverified at 1013.”
2. At `:267-273`, the old #104 Added bullet ends by saying full `cs-gpios` was
   rejected. Append this sentence rather than deleting history:

   > **Superseded in the same Unreleased development cycle:** SP1 later added a
   > real GPIO controller and replaced that temporary mapping with required,
   > same-parent standard `cs-gpios`; the Zephyr path now uses checked GPIO
   > edges around `spi/transfer`, while non-Zephyr `spi/batch` remains supported.

No root `CHANGELOG.md` or per-crate CHANGELOG edit is warranted: SP1 is a Zephyr
module change and the crate diff is empty.

### 5.2 `zephyr/README.md`

The 4096 row at current `:564` is I2C (`PDG_I2C_MAX_BUFFER=4096U`); the 1013 row
at `:582` is SPI (`PDG_SPI_MAX_BUFFER=1013U`). They are not contradictory and
must remain distinct.

Correct recovery language only:

- `:417-421`: after the sentence about host death, state that ordinary asserted
  CS can be reclaimed by a fresh session that deasserts it; a wedged dispatcher
  was observed to resume after USB re-enumeration. Do not imply a proven
  cancellation mechanism or that every condition requires power-cycle.
- `:450-453` and `:659-662`: keep
  `gallo_system_reset_subscriptions()` as the primary orphaned-subscription path;
  say power-cycle is only a fallback if USB re-enumeration/software cleanup is
  unavailable, not required.
- `:469-476` and `:647-652`: retain the process-local CS fault-latch recovery;
  explicitly distinguish it from the device-wide non-returning RPC wedge.

The README parity pass also covers prerequisites, initialization latency,
stderr logging, GPIO read side effects/errors, HOLD/LOCK failure, measured SPI
limits, current log format, controller count, pin naming, and stale cross-links.

## 6. Book parity sweep

### 6.1 Checklist execution mapped to AGENTS.md §15.1

`@docs` must record these six checks in its handoff:

1. Compare every SP1 code path in `git diff e7087bdc4eee~1..HEAD` with its paired
   book text. The relevant user-visible changes are MFD topology, GPIO, standard
   CS, HOLD/LOCK, non-atomic Zephyr transfers, and the 1013 containment.
2. Re-check CLI snippets only by source/help snapshots already in the repository;
   do not execute a binary. SP1 changed no CLI surface, so no CLI text change is
   expected.
3. Re-derive endpoint/status/wire/capability tables from source statically.
   SP1 added no endpoint, status, wire variant, or capability bit; catalog files
   should not gain such entries.
4. Confirm no new endpoint exists. `appendix/endpoints.md` needs no endpoint row;
   it may continue to document `spi/batch` because that endpoint still exists.
5. Confirm no wire shape changed in SP1. No schema/version/release-page edit is
   required by M6; the pre-existing branch schema-freeze warning remains.
6. Run `mdbook build book` only after edits. It is the M6 executable gate; do not
   touch ignored `book/book/`. Report success/failure, and do not tick the plan
   checkbox before success.

### 6.2 Required book edits

#### `book/src/appendix/troubleshooting.md`, `BufferTooLong` (`:98-119`)

Replace “usable payload ... 1013 bytes, established on hardware” with:

> The Zephyr SPI driver uses a **1013-byte local containment limit**. 1013 is
> the largest TX-only length observed to work; 4096 TX-only and 3072 full duplex
> reached the transport and failed `-ECOMM`. The exact TX-only boundary is
> unresolved between 1013 and 1015, and the full-duplex ceiling is unknown:
> duplex is proven at 512, fails at 3072, and is unverified at 1013.

Retain the split-transfer advice, but do not claim batch can make an otherwise
undeliverable aggregate payload fit one frame; each encoded request/response
still has framing limits.

#### `book/src/interfaces/batching.md`, Limits (`:263-272`)

Replace the two 4096 “payload/response” rows with:

| Parameter | Value |
|-----------|-------|
| Maximum operations per batch | 64 (`MAX_BATCH_OPS`) |
| Protocol packet-buffer budget | 4096 bytes (`MAX_TRANSFER_SIZE`), including framing |
| Usable request/response payload | Shape-dependent and strictly below 4096; not a published general ceiling |

Follow with:

> `MAX_TRANSFER_SIZE` is a packet-buffer budget, not a promise that 4096 bytes
> of application data are deliverable. Request shape, postcard length encoding,
> RPC headers, COBS framing, and response size all matter. Split operations well
> below the budget; the Zephyr SPI driver's separate 1013-byte containment limit
> does not establish a generic batch or duplex ceiling.

#### `book/src/crates/ffi.md`, Limits (`:63-78`)

Keep the exported literal because it matches the generated header, but replace
“applies per direction ... may carry that many bytes each way” with:

> `GALLO_MAX_TRANSFER_SIZE` mirrors the protocol's 4096-byte packet-buffer
> budget and local argument bound; it is **not** a guarantee that 4096 bytes of
> application payload can traverse the framed transport. Deliverable size is
> operation-shape dependent. In particular, Zephyr contains `spi/transfer` at
> 1013 bytes based on TX-only evidence, while the full-duplex ceiling remains
> unknown. Exceeding an API's local bound yields `Status::BufferTooLong`; a
> smaller framed request can still fail at transport.

This is a documentation correction to a false public claim, not a crate change.

#### `book/src/interfaces/spi.md`

- `:13-21` and `:118-227` are coherent: the first block describes firmware and
  host `spi/batch`; the later block explicitly says Zephyr does not use it. Keep
  both, but add “Non-Zephyr host APIs” to the first block's opening sentence so
  readers do not mistake it for the Zephyr path.
- Preserve `:212-218` with the authoritative TX-only/duplex caveat.
- At `:174-177`, use the same recovery distinction as the Zephyr README: a fresh
  session can deassert ordinary residue; after the reproduced dispatcher wedge,
  the device was observed to resume following USB re-enumeration.
- In the Error Handling row `:359`, change “firmware buffer limit” to “local
  operation limit or framed transport budget; usable payload is shape-dependent.”

#### `book/src/internals/firmware.md`

The watchdog claim at `:24-38` is false after M5. Replace the claim that a future
unbounded handler await “will trip the watchdog” with:

> The dedicated feeder proves executor liveness, not dispatcher progress. M5
> demonstrated that a request handler can block the serial postcard-rpc
> dispatcher while the feeder continues to run, so the 2 s watchdog does not
> reset that failure mode. A 1015-byte TX-only `spi/transfer` reproduced this
> device-wide wedge. In those reproduced tests, the device resumed after USB
> re-enumeration (`usbipd detach`/attach on Windows/WSL); that is an observed
> procedure, not proof that detach cancels the handler. Treat the watchdog as
> defense against executor-wide stalls, not as bounded RPC cancellation or a
> guarantee against handler deadlock.

At `:179-180`, clarify that 4096 is a protocol/internal buffer bound and usable
end-to-end payload is lower and shape-dependent.

### 6.3 Files reviewed; no edit required

- `book/src/interfaces/gpio.md`: coherent MFD child, parent selector, GPIO
  limitations, and CS role. No old flat topology or bespoke property remains.
- `book/src/appendix/endpoints.md`: endpoint catalog remains correct;
  `spi/batch` still exists for non-Zephyr users.
- `book/src/internals/wire-protocol.md`: no SP1 wire change; existing
  schema-freeze warning remains relevant.
- `book/src/hardware/pinout.md`: physical GPIO mapping and host batch CS remain
  correct; it does not claim GPIO5 is a Zephyr CS.
- `book/src/SUMMARY.md`: the Zephyr module is already linked from the SPI/GPIO
  pages, while `zephyr/README.md` explicitly remains its WIP detailed guide.
  Adding a duplicate book chapter is not earned in M6.
- `book/src/crates/{app,python,ffi,lib,mcp}.md`: each reachable host surface
  needs a short warning that the Zephyr containment does not protect it from
  the 1015-byte crash-class request, linking to the canonical SPI explanation.

Final static greps must find zero `cs-gpio-indices` under `book/`, `zephyr/`, and
`crates/`; historical docs are deliberately excluded. Search book text for old
flat node paths and for any statement that Zephyr uses `spi/batch`.

## 7. Draft issue briefs — report only, do not file

The following are **not-filed handoff drafts**. `@docs` must provide paste-ready
issue prose to the maintainer but must not file issues or add separate issue
artifacts to the repository.

### 7.1 Crash-class serial-dispatcher hang

- **Framing:** high-severity firmware reliability defect; deterministic denial
  of service, not a Zephyr bug. Labels: `bug`, `firmware`, `SPI`, `reliability`;
  avoid “security” unless the maintainer applies that policy.
- **Evidence:** 1015-byte TX-only `spi/transfer` never returns, twice with
  byte-identical logs; fresh-process RPCs including
  `system/reset-subscriptions` then hang; watchdog feeder continues; source
  the `PDG_SPI_MAX_BUFFER` comment beginning “SAFETY FIRST, SIZE SECOND”, plan
  §12.2, and Zephyr CHANGELOG “Known Issues”.
- **Reachability:** CLI, Python, FFI, library, and any consumer that can send the
  request; `PDG_SPI_MAX_BUFFER=1013` protects only the Zephyr driver.
- **Recovery observation:** the reproduced SPI condition resumed after USB
  re-enumeration (`usbipd detach`/attach on Windows/WSL). Do not claim a direct
  cancellation mechanism or generalize it to other dispatcher wedges.
- **Acceptance:** bounded request cancellation or dispatcher progress recovery;
  fresh RPCs work after the offending host dies; regression test cannot wedge
  the suite indefinitely. Link `AGENTS.md` §13.17's 2026-06-03 GPIO-wait row as
  a related serial-dispatch risk, not the same reproduced trigger.

### 7.2 Undeliverable advertised transfer ceiling

- **Framing:** high-priority protocol-contract bug/design debt. Labels: `bug`,
  `protocol`, `firmware`, `breaking-change` (or project equivalents).
- **Evidence:** advertised `MAX_TRANSFER_SIZE=4096`; 4096 TX-only fails transport;
  guessed 3072 duplex also fails; 1013 is only a TX-only lower bound and duplex
  works at 512. Cite the `PDG_SPI_MAX_BUFFER` comment beginning “MODEL”, plan
  §§12.1 and M6 correction,
  troubleshooting, and Zephyr CHANGELOG.
- **Required design:** derive safe operation-specific payload ceilings from
  worst-case request and response encoding/framing; expose one shared/generated
  contract instead of consumer-local constants; add limit and limit+1 tests for
  TX-only, RX-only, and duplex.
- **Release impact:** wire change requiring schema bump and lockstep release per
  AGENTS.md §§6.2 and 6.5. Do not propose a magic-number increase.

### 7.3 Silent success when `cs_is_gpio` is false

- **Framing:** medium-severity diagnostic/invariant bug in the Zephyr driver;
  upstream-shaped and not an SP1 regression. Labels: `bug`, `zephyr`, `SPI`,
  `diagnostics`.
- **Evidence:** `pdg_spi_cs_control_checked()` returns 0 for a non-GPIO CS; manual
  malformed `spi_config` can then complete without asserting any CS even though
  the binding makes `cs-gpios` structurally mandatory. Cite the README paragraph
  beginning “If you would rather not declare a child node”.
- **Recommendation:** add `LOG_WRN` when a live transfer config has no GPIO CS;
  decide separately whether rejecting it would exceed upstream compatibility.
  Acceptance must prove a diagnostic and no change to valid DT-generated paths.

### 7.4 Watchdog monitors executor liveness, not dispatcher progress

- **Framing:** high-priority firmware reliability design gap, related to but
  separable from the concrete SPI trigger. Labels: `enhancement` or `bug`,
  `firmware`, `watchdog`, `reliability`.
- **Evidence:** feeder task runs independently every 800 ms, so a serial handler
  can block all RPC dispatch forever while the watchdog remains fed; M5's 1015
  trigger proves it. Cite the firmware chapter's “Watchdog” section before M6,
  plan §12.2, and the `PDG_SPI_MAX_BUFFER` comment's “KNOWN FIRMWARE HANG”.
- **Desired outcome:** feed only on a health invariant that includes dispatcher
  progress, or add per-request deadlines/cancellation; preserve long legitimate
  operations. Define a bounded test that distinguishes executor activity from
  RPC progress. Link issue 7.1 rather than duplicating its reproduction.

## 8. Invariants and failure modes

- Historical findings in plan §§8–12 remain recognizable and substantively
  intact; corrections are additive forward/supersede notes.
- Every transfer-limit statement distinguishes packet budget, TX-only evidence,
  duplex evidence, and the local containment constant.
- `spi/batch` remains documented for non-Zephyr consumers; only Zephyr stopped
  using it.
- No text promises that the watchdog recovers a blocked serial handler.
- No text presents USB re-enumeration as the proven mechanism or generalizes the
  reproduced SPI recovery observation to another dispatcher-wedge trigger.
- If static source evidence conflicts with a proposed doc correction, stop and
  ask the maintainer; do not alter code.
- `mdbook build book` failure blocks the book commit. It does not authorize an
  edit to generated `book/book/`.

## 9. Alternatives considered

1. **Rewrite the plan's early sections.** Rejected because it erases the
   chronological record and violates the explicit historical-preservation rule.
2. **Treat 1013 as the duplex ceiling because plan §12.1 says so.** Rejected:
   the M5 acceptance test proves duplex only at 512; the source comment and book
   initially disagreed and are corrected by M6.
3. **Add a dedicated Zephyr book chapter.** Rejected as duplication while the
   module remains WIP and `zephyr/README.md` is intentionally authoritative.
4. **One giant documentation commit.** Rejected because plan reconciliation,
   regression policy, release notes, and user book parity are independently
   reviewable logical changes.

## 10. Verification

Documentation implementer must run only non-hardware checks:

```text
# static searches
cs-gpio-indices under zephyr/, book/, crates/ => zero hits
old flat /pdg-i2c or /pdg-spi claims in book/ => zero current-contract hits
“Zephyr” near “spi/batch” => only explicit “does not use” wording

# generated documentation check
mdbook build book

# scope check
git diff --stat e7087bdc4eee~1..HEAD -- crates/ => empty
git status --short => only authorized M6 documentation files
```

Do not run Cargo binaries, Zephyr samples, M5, hardware tools, or a ceiling sweep.

## 11. Commit split

Use four implementation commits plus a prior spec commit. This agrees with the
maintainer's candidate split: each has a distinct source of truth and review
surface.

1. `docs(repo): Specify M6 documentation consolidation`
   - this specification only; committed first by `@integrator`.
2. `docs(repo): Reconcile the Zephyr MFD plan with M5 evidence`
   - plan forward pointers and final definition-of-done only.
3. `docs(repo): Record the Zephyr MFD transport regressions`
   - AGENTS.md existing-row correction and new row together; separating them
     would temporarily retain the contradiction.
4. `docs(zephyr): Consolidate the SP1 changelog and recovery guidance`
   - `zephyr/CHANGELOG.md`, `zephyr/README.md`, binding descriptions, Kconfig
     help, and the `PDG_SPI_MAX_BUFFER` comment.
5. `docs(repo): Align the book with measured SPI transport limits`
   - all `book/src/**` edits; run `mdbook build book` before this commit.

All subjects are under 72 characters, imperative, capitalized, and have no
trailing period. Add the repository-required AI trailers using the actual agent
and model; never add `Signed-off-by`. Do not squash.

## 12. Open questions and maintainer escalations

No ruling is needed for the duplex contradiction: the committed M5 acceptance
test proves 512-byte full duplex, while 1013 remains TX-only evidence. M6 fixes
the shipped comments that disagreed with that artifact.

The following remain code/product decisions and must not be fixed in M6:

- the crash-class 1015-byte firmware dispatcher hang;
- the undeliverable/general transfer-ceiling contract and required wire change;
- silent success for a non-GPIO CS config in `pdg_spi_cs_control_checked()`;
- a watchdog architecture that proves executor liveness but not dispatcher
  progress.

One documentation-policy choice may be revisited later: whether the Zephyr WIP
module deserves its own mdBook chapter after upstreaming. It is not required for
SP1 parity and has no bearing on M6 completion.
