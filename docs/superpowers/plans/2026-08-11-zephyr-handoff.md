# Zephyr Module — Session Handoff (2026-08-11)

Continuation notes for a fresh session. The preceding work is on branch
`zephyr`, open as **draft PR
[#112](https://github.com/OpenDevicePartnership/pico-de-gallo/pull/112)**
against `OpenDevicePartnership/pico-de-gallo` `main`.

Read `AGENTS.md` §4 and §13 first. This file only covers what is *not*
already in the repo.

---

## 1. State

| Item | Value |
|---|---|
| Branch | `zephyr`, based on `upstream/main` @ `f92dd100` |
| Pushed to | `origin` (`felipebalbi/pico-de-gallo`). **Not** on `upstream`. |
| PR | #112, **draft**, CI green (`deploy` skipped, main-only) |
| Worktree | clean |

Check the live state rather than trusting a number written here:

```bash
git rev-list --count upstream/main..HEAD
gh pr checks 112 --repo OpenDevicePartnership/pico-de-gallo
```

Closed by the PR: **#103, #105, #106, #107, #108, #111**. Refs #98.

Do not force-push without asking (§4 rule 8). The PR is a draft
deliberately — per §11, request review only once it is intended to land.

---

## 2. Environment (this is the part that is not in the repo)

There is a Zephyr workspace at `~/zephyrproject` (7.0 GB, Zephyr
`main` @ `v4.4.0-11199-g3a6406439c5a`) and a virtualenv at
`~/zephyr-venv` holding `west` 1.5.0. **The system `west` at
`~/.local/bin/west` is broken** — it imports from
`/usr/lib/python3.14/site-packages` and fails. Always use the venv.

Every Zephyr command needs all three of these:

```bash
export ZEPHYR_BASE=/home/balbi/zephyrproject/zephyr
export ZEPHYR_TOOLCHAIN_VARIANT=host
export PATH=/home/balbi/zephyr-venv/bin:$PATH
```

`ZEPHYR_TOOLCHAIN_VARIANT=host` is not optional. Without it the build dies
in `FindZephyr-sdk.cmake` because the installed SDK is 0.16.0 and Zephyr
4.4 wants ≥1.0 — an error that names neither `native_sim` nor the SDK
version as the problem.

Build a sample (note `-b` and `-DSHIELD=` are required on a clean tree; the
sample's `set(... CACHE ...)` defaults are inert because west resolves
`BOARD` before CMake):

```bash
cd zephyr/samples/spi_nor_id
west build -p always -b native_sim/native/64 -- -DSHIELD=pico_de_gallo
west build -t run       # works only after the tree is configured
```

Toolchain versions in play: CMake **4.4.2**, gcc **16.1.1**, rustc
**1.97.1**. The repo lives at `/home/balbi/workspace/pico-de-gallo`,
entirely outside the Zephyr workspace, and that is fine — the samples add
the module via `EXTRA_ZEPHYR_MODULES`.

---

## 3. Hardware

One board attached during the session: serial `49742081C885AC69`,
hw rev **2**, firmware 0.10.1, schema 0.6.1. A **GigaDevice GD25Q16**
SPI NOR (2 MiB) is wired to chip-select index **0 = GPIO 8**, and it
holds an **iCE40 FPGA bitstream**. Treat it as precious: the owner
would have to reflash it.

No I2C peripheral was attached, so the I2C path was only ever verified at
build time and against a no-board failure. See §5.

### Chip-select is an index, not a pin

This costs everyone an hour. `cs` / `reg` / `.slave` is an index 0–3 that
selects a *user* GPIO:

| index | GPIO |
|---|---|
| 0 | 8 |
| 1 | 9 |
| 2 | 10 |
| 3 | 11 |

The dedicated `SPI_CS` on **GPIO 5** in the hardware pinout is **not
configured by the firmware at all** — `main.rs` wires only PIN_6 (SCK),
PIN_7 (MOSI), PIN_4 (MISO). That is the substance of open issue **#99**.

---

## 4. Remaining work

Nothing below was attempted, except where a row says otherwise. All were
explicitly out of scope.

**Line numbers quoted inside issues #99–#110 are stale.** They were written
against `f92dd10` and the files have moved since — e.g. #106 cites
`pdg_spi.c:179` for the `config->slave > 3U` bound, which is now line 206.
Grep for the code, do not trust the citation.

| Issue | Notes for whoever picks it up |
|---|---|
| **#101** I2C probing broken (NULL / zero-length msgs) | Needs an I2C peripheral attached. `pdg_i2c.c:120-127` only accepts specific message groupings and returns `-ENOTSUP` otherwise. |
| **#102** `i2c_burst_write()` → `-ENOTSUP` | Same root area as #101: the message-grouping matcher at `pdg_i2c.c:112-127`. |
| **#104** SPI silently reconfigures GPIO0 as CS | Related to the index/pin confusion in §3. Consider fixing alongside #99. The hard-coded `config->slave > 3U` can now be bounded by `GALLO_NUM_GPIOS` from the generated header — see §4.1. |
| **#99** Can't use `SPI_CS` pin as CS | Firmware-side: GPIO 5 is never claimed. Wire-protocol change likely — read §6 of AGENTS.md before touching it. |
| **#109** twister / CI | Investigated and **deliberately deferred** by the maintainer, not blocked. See §4.2 for the constraint and what was already proven. |
| **#110** book ↔ AGENTS.md parity | `zephyr/README.md` was rewritten and is accurate, but the module is still absent from `book/` and from the §15.1 mapping table. |

### 4.1 #106 is closed

`GALLO_NUM_GPIOS` shipped in `feat(internal,ffi,firmware): Export the GPIO
count as GALLO_NUM_GPIOS`. `NUM_GPIOS` was `pub(crate)` in the firmware —
invisible to every host crate — so it was hoisted into
`pico-de-gallo-internal`, re-exported through `pico-de-gallo-lib`, and
mirrored into the C header. The firmware now reaches it through a
`pub(crate) use`, so every `crate::context::NUM_GPIOS` call site is
unchanged.

Both cbindgen traps were re-confirmed rather than assumed: the constant is
written as a literal (a path expression emits *nothing*, silently) and a
`const` assertion ties it back to the wire crate — mutating it to 5 fails
the build. Running cbindgen confirmed `#define GALLO_NUM_GPIOS 4` really
does reach the header.

This removes the last magic number from the Zephyr module's list in #106
that had no exported source of truth.

### 4.2 #109 — what is settled and what is not

**Hard constraint from the maintainer: no `#include` of a `.c` file.** That
rules out the whitebox approach, which is otherwise the cheapest way in.

The obstacle is unchanged: all six functions #109 wants tested are `static`,
so a test binary cannot reach them —

    speed_to_code_   pdg_i2c.c:47      bufset_len_     pdg_spi.c:71
    freq_to_speed_   pdg_i2c.c:88      flatten_tx_     pdg_spi.c:95
    validate_group_  pdg_i2c.c:110     unflatten_rx_   pdg_spi.c:116

Remaining options are to extract them into a real `.c`/`.h` pair with
external linkage, or to test only through the driver's public API.

Three findings from the investigation are worth keeping, because they were
verified by building and are not obvious:

1. **Hardware-free CI is genuinely cheap.** A ztest suite that instantiates
   no devicetree node leaves `CONFIG_PICO_DE_GALLO=n`, because
   `zephyr/Kconfig` only defaults it `y` when a `odp,pico-de-gallo-*` node
   is enabled. That skips the whole `if(CONFIG_PICO_DE_GALLO)` block in
   `zephyr/CMakeLists.txt` — **no cargo, no rustc, no Corrosion tarball
   fetch**. Confirmed by grepping the build tree. The board dependency only
   bites for sample builds and hardware-in-the-loop.
2. **twister builds with `-Werror`.** `CONFIG_COMPILER_WARNINGS_AS_ERRORS=y`
   is set by twister's `runner.py`, so any `static` function reachable from
   nothing is a hard build failure, not a warning.
3. **A recording fake for the `pdg_*_bottom_*` layer is what makes #101
   answerable at all.** #101's real question — what does a zero-length
   write actually put on the wire? — cannot be answered by inspection, and
   otherwise needs a bench peripheral and a logic analyser. The fake is
   independent of how the `static` problem is solved.

Also still true: `samples/i2c_bridge` and `samples/spi_nor_id` both build
without a board, so a build-only CI tier is available whenever it is wanted.

### Not an issue yet

- **`samples/spi_bridge` and `samples/combined_i2c_spi_bridge` cannot be
  built.** They need an `issi,is31fl3743b` LED driver that is not in Zephyr
  `main` (only `is31fl319x`, `is31fl3216a`, `is31fl3733` are). Either
  upstream that driver or rework the samples.
- The `i2c_bridge` "with board attached" output in `zephyr/README.md` is
  **derived from `src/main.c`'s printk format, not observed.** Attach a
  TMP117 and correct it if it differs. This is the only unverified claim
  left in that file.

---

## 5. What is actually verified, and how

Do not re-litigate these; do not assume anything else was checked.

- **SPI, on hardware, end to end.** `samples/spi_nor_id` drives the real
  flash through Zephyr's stock `jedec,spi-nor` driver and the generic flash
  API, with geometry discovered at runtime over SFDP. Transcript is in the
  commit message of `feat(zephyr): Identify SPI NOR via the jedec,spi-nor
  driver`.
- **The flash was not written.** Verified through an independent path (the
  `gallo` MCP server, not the Zephyr driver) before and after: both status
  registers `0x00`, first 16 bytes byte-identical, two further sectors
  `0x00` rather than the `0xFF` an erase leaves.
- **#103 guard**, via a four-state matrix. This matters: the shield ships
  `pdg_spi0` **disabled**, so the stock sample cannot reach the SPI Kconfig
  and a naive check passes on an empty diff. Enable the node with an
  overlay, then confirm baseline+RTIO builds with
  `CONFIG_SPI_PICO_DE_GALLO=y`, guarded+RTIO and guarded+ASYNC each fail,
  and guarded with neither still builds.
- **#107**, by mutation rather than inspection: changing
  `GALLO_MAX_BATCH_OPS` in the working tree fails the Zephyr build on the
  const assertion at `crates/pico-de-gallo-ffi/src/lib.rs:268`.
- **I2C: build only.** `samples/i2c_bridge` compiles and runs; with no board
  it reports `-ENODEV` as expected. No I2C traffic has ever been observed.

---

## 6. Traps found the hard way

Each of these cost real time. They are recorded in commit messages too.

1. **Kconfig `depends on !SPI_ASYNC` is a dependency loop.** `SPI_ASYNC`
   and `SPI_RTIO` both depend on `SPI`, which the driver `select`s. Kconfig
   refuses to parse. `BUILD_ASSERT` is the working spelling, and diagnoses
   better — `depends on` would silently drop the driver and surface as an
   unresolved `__device_dts_ord_N` at link.
2. **`$<TARGET_FILE:pico_de_gallo_ffi>` does not work**, for two
   independent reasons: Corrosion's target is not an executable/library
   target, *and* generator expressions are not recursive, so the
   `$<TARGET_PROPERTY:>` read in `natsim_config.cmake` emits the genex
   verbatim into `nsi_config`. Seeing `$<JOIN:` there is not evidence that a
   nested genex expands. Use `CMAKE_*_LIBRARY_PREFIX/SUFFIX`.
3. **`CONFIG_FLASH=y` silently enables `CONFIG_FLASH_SIMULATOR=y`** on
   `native_sim`, adding a second flash device and dropping a 2 MiB
   `flash.bin` in the source tree. `samples/spi_nor_id` disables it
   explicitly so results cannot be mistaken for simulated ones.
4. **Zephyr's `spi_nor` driver can write during init**, but every such path
   is gated on a devicetree property: `has-lock` (WREN+WRSR),
   `requires-ulbpr`, `enter-4byte-addr`, `has-dpd`,
   `mxicy-mx25r-power-mode`, `use-flag-status-register`. Omit them all and
   init issues only RDID and RDSFDP. **Adding `has-lock` will write to the
   part** — correct behaviour for a part that powers up protected, but not
   what you want against an FPGA bitstream.
5. **Zephyr `main` is mandatory**, not a release. `pdg_spi.c` reads
   `config->cs.setup_ns` / `cs.hold_ns`, which exist only on `main`.
   Check with
   `grep -c setup_ns $ZEPHYR_BASE/include/zephyr/drivers/spi.h`.
6. **Always `native_sim/native/64`.** `zephyr/Kconfig` has
   `depends on 64BIT` because `corrosion_set_hostbuild()` forces the rustc
   host triple. Plain `native_sim` is 32-bit.

---

## 7. Policy note

Commit subject limit was raised from 50 to **72 characters** at the
maintainer's request, in `AGENTS.md` §10 and in
`.opencode/agent/{coder,integrator}.md`. That change is carried on this
branch as `docs(repo): Raise the commit subject limit to 72 characters` and
is unrelated to the Zephyr work — it can be split out if a reviewer
objects.

The two plan documents under `docs/superpowers/plans/` still quote the old
50-char rule. That is deliberate: they are dated records of what was
believed while executing them.
