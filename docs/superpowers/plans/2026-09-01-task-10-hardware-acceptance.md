# Task 10 — hardware acceptance for issue #159

Manual, board-attached. Not runnable in CI. This is the issue's stated
acceptance criterion, so #159 should not be closed until it passes.

Branch: `issue-159`. Nothing pushed. Tree must stay clean between steps
except where a step says otherwise.

---

## Before you start — three things to know

1. **BOOTSEL is physical.** No debug probe is attached (`probe-rs list` finds
   none), and the firmware exposes only `binary_info` metadata, not a picotool
   reset interface (`picotool info` reports no device in BOOTSEL). So the board
   cannot be rebooted into BOOTSEL from software. You must hold the button.

2. **Do not use `elf2uf2-rs`.** The locally installed `elf2uf2-rs v2.2.0` is the
   stale crates.io build with no `--family` flag — the exact AGENTS.md §13.9
   trap. Use `picotool uf2 convert`, which is what
   `.github/workflows/release-firmware.yml:78` uses. Local `picotool v2.3.0`
   supports it.

3. **Once flashed, a *released* `gallo` cannot read `device/info` from this
   board.** Appending `build_id` changed the endpoint key, so a mismatched host
   drops the reply and the call times out. Throughout this procedure use the
   branch build — `cargo run -p gallo --locked -- ...` from the repo root — not
   an installed `gallo`. Step 6 restores the board.

Board under test: serial `49742081C885AC69`, currently running released
firmware 0.11.0 / schema 0.7 (no `build_id`).

---

## Pre-built artifacts (hw-rev2)

All three images are built and their embedded identity verified. `hw-rev2` is
the default feature, so these are rev2 images.

| File | Embedded identity | SHA-256 |
|---|---|---|
| `/tmp/pdg-159/clean.uf2` | `firmware-v0.11.0-41-ga73a8130f9fe` | `8eb5d1f7…4ebe67d` |
| `/tmp/pdg-159/dirty.uf2` | `firmware-v0.11.0-41-ga73a8130f9fe-dirty` | `c1c8bab2…828239fa` |
| `/tmp/pdg-159/clean2.uf2` | `firmware-v0.11.0-41-ga73a8130f9fe` | `8eb5d1f7…4ebe67d` |

`clean2.uf2` was produced by reverting the edit and rebuilding **without**
`cargo clean`. It is **byte-identical** to `clean.uf2`, so for Step 5 you can
simply re-flash `clean.uf2`.

### What is already proven, without a board

Inspecting the identity embedded in each image establishes, at build level:

- **PASS** — clean and dirty identities differ, from a behaviour-only edit that
  changed no wire type. This is the acceptance criterion's mechanism.
- **PASS** — `-dirty` cleared after revert on an **incremental** rebuild with no
  `cargo clean`, so the always-rerun trigger in `build.rs` works and the
  identity does not go stale.
- **PASS** — the reverted build reproduced `clean.uf2` bit-for-bit.

### RESULT — executed 2026-09-01, PASS

Run on board `49742081C885AC69` via `flash.sh`, which runs `picotool verify`
against the file before rebooting. Verify reported `OK` on all three.

| Image flashed | `gallo version` reported `Build` |
|---|---|
| `clean.uf2` | `firmware-v0.11.0-41-ga73a8130f9fe` |
| `dirty.uf2` | `firmware-v0.11.0-41-ga73a8130f9fe-dirty` |
| `clean2.uf2` | `firmware-v0.11.0-41-ga73a8130f9fe` |

- **PASS** — clean and dirty are distinguishable over the wire, while
  `Firmware v0.11.0` and `Schema v0.7.0` stayed identical across both. That is
  the acceptance criterion: two images differing only in handler behaviour now
  produce two different `device/info` responses, which `validate()` alone could
  never do.
- **PASS** — `-dirty` cleared on the reverted image, confirming end to end that
  the identity does not go stale.

**Always use `flash.sh`, or run `picotool verify` by hand.** A first attempt at
this procedure produced three identical readings because the second and third
writes did not land, and nothing in the flow said so — the board simply kept
running the first image. `picotool verify` is what makes that impossible to
miss, and a silent no-op here would have looked exactly like a firmware bug.

### What the board proved that a build inspection could not

The above only shows the right bytes are *in the image*. It does not show the
firmware *reports* them over the wire. Flashing is what verifies the
`device/info` path end to end: that the handler populates `build_id`, that it
survives postcard encoding, and that `gallo version` renders it. That is the
part still outstanding.

---

## Flashing, once (referenced below as "flash `<file>`")

```bash
# 1. Unplug the board.
# 2. Hold BOOTSEL, plug it in, release BOOTSEL.
#    It enumerates as a mass-storage device named RPI-RP2.
# 3. Then either drag the .uf2 onto RPI-RP2, or:
picotool load -x <file>
```

`-x` executes after loading, so the board re-enumerates as a Pico de Gallo.
Confirm with:

```bash
cd /home/balbi/workspace/pico-de-gallo
cargo run -p gallo --locked -- list
```

---

## Step 1 — flash the clean build (already built for you)

Flash `/tmp/pdg-159/clean.uf2` (see the artifact table above).

## Step 2 — record the clean identity

```bash
cd /home/balbi/workspace/pico-de-gallo
cargo run -p gallo --locked -- version
```

**Expect** a two-table output whose `Build` row reads
`firmware-v0.11.0-41-ga73a8130f9fe`, with **no** `-dirty` suffix, and
`Firmware v0.11.0` / `Schema v0.7.0` / `HW revision 2` / `GPIOs 4`.

Record the `Build` value. Call it **A**.

> If `Build` is absent, or the command falls back to
> `(legacy firmware — no schema/hw/capabilities info)`, the flash did not take —
> the board is still on released firmware. Re-flash before continuing.

## Step 3 — make a behaviour-only change and rebuild

Already done for you. `dirty.uf2` was built from the tree with one extra
`info!` line added to `ping_handler` in
`crates/pico-de-gallo-firmware/src/handlers/info.rs` — behaviour changed, no
wire type touched. The edit was reverted afterwards, so the repo is clean.

Flash `/tmp/pdg-159/dirty.uf2`.

## Step 4 — THE ACCEPTANCE CHECK

```bash
cd /home/balbi/workspace/pico-de-gallo
cargo run -p gallo --locked -- version
```

**Expect:**

- `Build` now ends in `-dirty` and therefore **differs from A**;
- `Firmware` is still `v0.11.0` and `Schema` still `v0.7.0` — **unchanged**.

That is the acceptance criterion from the issue: *two images differing only in
handler behaviour produce two distinguishable `device/info` responses.* Before
this change, both images reported identical firmware and schema versions with
nothing to tell them apart — which is how a previous hardware verification
session reached a wrong conclusion.

Record this value as **B**. **A != B is the pass condition.**

## Step 5 — the staleness regression check

This proves the always-rerun trigger in `build.rs` works. Without it, the
identity would go stale across incremental builds and keep claiming a clean
tree.

The build half is **already verified**: `clean2.uf2` was produced by reverting
the edit and rebuilding with **no `cargo clean`**, and it came out
byte-identical to `clean.uf2`. So re-flash `/tmp/pdg-159/clean.uf2`, then:

```bash
cd /home/balbi/workspace/pico-de-gallo
cargo run -p gallo --locked -- version
```

**Expect** the `-dirty` suffix to be **gone**, and `Build` to equal **A** again.

If `-dirty` is still there, `build.rs` did not re-run and the always-rerun
trigger is broken — that is a real failure, report it.

## Step 5b — optional: the MCP surface

With the board still on the branch firmware, check the MCP `status` tool
reports `build_id`, and that the server log carries a `connected` line with it.
The default filter was changed to `error,gallo_mcp=info` precisely so that line
is emitted; if you see no `connected` line, that regression is back.

## Step 6 — restore the board (recommended)

The branch firmware cannot talk to a released `gallo`. To put the board back:

Download `firmware-rev2.uf2` from the `firmware-v0.11.0` GitHub release and
flash it. Then a released `gallo` works again, and
`cargo run -p gallo --locked -- version` from this branch will show the legacy
fallback — which is itself a correct demonstration of the mismatch behaviour.

---

## What to report back

| | Value |
|---|---|
| A — clean build identity (Step 2) | |
| B — dirty build identity (Step 4) | |
| A != B ? | pass / fail |
| `Firmware`/`Schema` unchanged between A and B ? | pass / fail |
| Step 5: `-dirty` disappeared without `cargo clean` ? | pass / fail |
| Step 5b: MCP `status.build_id` + connect log present ? | pass / fail / skipped |

Any deviation, however small — especially a `Build` value that does not change
when it should, or one that keeps saying `-dirty` after Step 5 — is a genuine
failure and should be reported rather than retried until it looks right.
