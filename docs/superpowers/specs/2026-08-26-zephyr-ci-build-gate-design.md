# Zephyr CI build gate — design

- **Date:** 2026-08-26
- **Issue:** [#130](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/130) — *ci: No CI job builds or lints the Zephyr module*
- **Related:** #98 (upstreaming tracker), #109 (twister metadata and hardware-free
  unit tests), #131 (the human-review half, landed in #144 as `bf06fc9`)
- **Status:** approved, pending implementation

---

## 1. Problem

Nothing in CI builds, compiles or lints `zephyr/`. `rg -ln zephyr .github/workflows/`
returns nothing. `check.yml` covers the seven host crates, `nostd.yml` covers the
firmware for both hardware revisions, `gh-pages.yml` builds the book. The Zephyr
module — five drivers, four samples, four test applications, four devicetree
bindings and a shield — has no gate at all.

This matters more than an ordinary coverage gap because the Zephyr drivers are a
**consumer of `pico-de-gallo-ffi`**. They call `gallo_*` through the cbindgen-generated
`pico_de_gallo.h`, linked by Corrosion at Zephyr build time. An FFI change can
therefore break `zephyr/` while every existing gate stays green:

- `check.yml` builds the FFI crate, which still compiles.
- cbindgen regenerates the header without complaint.
- The Zephyr translation unit that consumed the removed or renamed symbol is
  never compiled by anyone.

AGENTS.md §15.1 already states the consequence outright: the module's two
compile-time safety mechanisms "only fire when a human runs a Zephyr build by
hand."

Those two mechanisms are real and currently dormant:

- `zephyr/drivers/common/common.c` maps every FFI `Status` to an `errno` in a
  switch with no `default:` inside it, compiled under `-Werror=switch`
  (`zephyr/drivers/CMakeLists.txt:34`). A new `Status` variant is a build failure.
- `zephyr/tests/pdg_mfd_m5/common/m5_bottom.c:37-51` carries eight
  `_Static_assert`s pinning `GalloGpioEdge`, `GalloGpioDirection` and
  `GalloGpioPull` to the wire-enum variant order, which is itself ABI (§6.1). A
  reordered wire enum is a build failure.

Neither has ever fired in CI, because CI has never compiled them.

## 2. Scope

### 2.1 In scope

A single GitHub Actions workflow that **builds** the Zephyr module against a
pinned upstream Zephyr revision on `native_sim/native/64`, and asserts per-target
outcomes including non-vacuity.

### 2.2 Explicitly out of scope — this gate is BUILD ONLY

The workflow never executes a produced binary. No `west build -t run`, no direct
launch of `zephyr.exe`, no invocation of `zephyr/tests/pdg_mfd_m5/run-m5.sh`, no
twister run. GitHub-hosted runners have no USB and no attached board.

This is safe, and the safety is a property of the code rather than of the
workflow's good manners. Per
`docs/superpowers/specs/2026-08-17-zephyr-mfd-m1-parent.md:236-240`:

> No build path reaches `gallo_init_strict()`. Configure invokes only
> `rustc --version --verbose`; Corrosion compiles and cbindgen generates; linking
> merely resolves the call compiled into `gallo_registry.c`. USB opens only when
> the native_sim process starts. Builds are safe; produced binaries must never run
> (no direct launch, `west build -t run`, test runner, or hardware command).

`gallo_init_strict()` is called from `zephyr/drivers/common/gallo_registry.c:174`,
reached only at runtime. Linking resolves the symbol; it never calls it.

### 2.3 What the gate does and does not catch

| Regression class | Caught | Mechanism |
|---|---|---|
| `gallo_*` renamed or removed | yes | undefined reference at the native_simulator runner link |
| `gallo_*` signature changed | yes | compile error at the call site |
| New FFI `Status` variant | yes | `-Werror=switch` in `common.c` |
| Reordered wire enum | yes | `_Static_assert` in `m5_bottom.c` (M5 targets only) |
| DT binding, Kconfig or shield breakage | yes | configure or devicetree error |
| `Cargo.lock` out of sync with `Cargo.toml` | yes | `corrosion_import_crate(... LOCKED)` |
| Wire or firmware **behaviour** change | **no** | needs attached hardware |
| Anything `run-m5.sh` verifies | **no** | needs attached hardware and physical jumpers |

Issue #130's claim that "compile coverage alone would have caught both prior
regressions" is not adopted here. The AGENTS.md §13.17 rows dated 2026-08-17 and
2026-08-19 record wire and firmware **behaviour** defects found on hardware; a
compile gate would not have caught either. This spec claims the narrower thing
that is true: the gate closes the FFI/link-surface hole, and hardware coverage
remains a separate, manual procedure.

## 3. Decisions

### D1 — Pin Zephyr to `26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0`

`zephyr/README.md:26` records the measured build environment as `main` at
`v4.4.0-6123-g26f811ee9d0`. The `git describe` form in
`docs/superpowers/specs/2026-08-17-zephyr-mfd-m1-parent.md:242` is
`v4.4.0-6123-g26f811ee9d0d`; the README truncates the abbreviated SHA by one
character. The full commit is
`26f811ee9d0dc8f67e8e596f6aef9e6e79a55db0`, committed 2026-06-24, verified present
upstream.

Pinning to that exact revision is chosen over any newer commit because it is the
only revision this module has ever been measured against. Selecting a newer one
would assert an untested compatibility claim. The README's own framing — "treat
`main` as the verified baseline rather than an asserted minimum" — is honestly
encoded by pinning the measured commit rather than by tracking a moving branch.

Pin rot is the accepted cost. A non-blocking scheduled job tracking Zephyr `main`
was considered and deferred (§8).

### D2 — One job driving a checked-in helper script

Rejected: a matrix over the eight build targets. It matches house style more
closely (`check.yml` and `nostd.yml` both matrix heavily) and yields eight
independent checks, but every leg re-restores a multi-gigabyte west workspace
cache and re-runs Corrosion's `PROFILE release` build of `pico-de-gallo-ffi`.

Chosen: one `ubuntu-latest` job that performs workspace setup once and then runs
`zephyr/scripts/ci-build.sh`, which builds all eight targets sequentially and
writes a per-target result table to `$GITHUB_STEP_SUMMARY`. Per-target attribution
is recovered from the summary table and `::group::` log folding.

The script is checked in rather than inlined in YAML for three reasons: the
assertion logic in §5 is intricate enough that it needs to be readable and
greppable; a maintainer with a Zephyr environment can run the whole gate by hand
with one command; and the repository has no local Zephyr environment, so a
hand-runnable artifact is the only practical escape hatch for reproducing a CI
failure.

### D3 — Eight build targets; the fifth M5 image is excluded

The four samples and the four M5 test applications are built. The fifth M5 image,
`spi_loopback`, is deliberately excluded.

`spi_loopback` builds the **upstream** suite at
`$ZEPHYR_BASE/tests/drivers/spi/spi_loopback` with this repository's overlay and
config fragments, and additionally requires `/tmp/m5-measured.conf`, which
`zephyr/tests/pdg_mfd_m5/run-m5.sh:537` synthesises from a live hardware timing
measurement. A build-only gate would have to fabricate that value.

Its marginal value for this issue is nil: it links the same
`pdg_spi.c`, `pdg_spi_bottom.c`, `common.c` and `gallo_registry.c` as the
`m5_acceptance` target, so it adds no `gallo_*` call site that target 7 does not
already cover. What it does add is upstream Zephyr API-surface coverage, which is
not what #130 is about, plus a dependency on an upstream source path and a
fabricated config value.

### D4 — Every pass target asserts non-vacuity

A green build proves nothing if the shield overlay silently left every
`CONFIG_*_PICO_DE_GALLO` disabled: the sample would compile with zero of this
module's translation units and pass forever, including under mutation.
`2026-08-17-zephyr-mfd-m1-parent.md` §6.2 flags the same hazard, requiring an
enabling overlay to "prove any otherwise-disabled translation unit is compiled."

Each pass target therefore additionally asserts the artefacts listed in §5.1.
Without this, the mutation check in §7.2 could pass for the wrong reason.

### D5 — Baseline failures are attributed by node, not by ordinal

`2026-08-17-zephyr-mfd-m1-parent.md` §6.2 is explicit:

> Baseline symbols were `__device_dts_ord_43` and `__device_dts_ord_44`, but the
> new shield node may renumber ordinals. Compare per sample by the same single
> undefined symbol's attribution, never by literal integer or cross-sample
> identity.

The assertion in §5.2 resolves the ordinal from the generated devicetree header at
build time and compares against the undefined symbol, so it is correct under
renumbering.

### D6 — The workflow and README SHA agreement is enforced, not promised

Issue #130's acceptance criterion 2 requires that the pinned revision be recorded
in both the workflow and `zephyr/README.md`, and that the two agree. A step greps
`zephyr/README.md` for the literal pinned SHA held in the workflow's `env:` and
fails if it is absent. The criterion becomes a test.

### D7 — Path-filtered triggers, with a required-check caveat

Triggers are `push` to `main` and `pull_request`, both filtered on `zephyr/**`,
`crates/pico-de-gallo-ffi/**`, `crates/pico-de-gallo-internal/**`, `Cargo.lock`,
`Cargo.toml` and `.github/workflows/zephyr.yml`.

`Cargo.lock` is in the list because `zephyr/CMakeLists.txt` imports the crate with
`corrosion_import_crate(... LOCKED)`, so a manifest/lock split breaks the Zephyr
build as well as `check.yml`'s `lockfile` job.

**Caveat, recorded deliberately:** a path-filtered workflow reports as *skipped* on
unrelated pull requests. If this job is later configured as a required status
check, GitHub will block merges on the skipped result unless a companion no-op job
is added. That configuration choice belongs to the maintainer and is not made here.

## 4. Target inventory

All builds use `-p always`, board `native_sim/native/64`, `-DSHIELD=pico_de_gallo`,
an absolute `-DEXTRA_ZEPHYR_MODULES=<repo root>`, and a build directory under
`/tmp`. Repository-root `build/` is not gitignored (only `zephyr/build/` is), so
build directories must never land in the tree.

The M5 applications use named overlays rather than `app.overlay`, so west does not
apply them automatically; each is passed explicitly via `-DDTC_OVERLAY_FILE`. The
command forms below follow
`docs/superpowers/specs/2026-08-19-zephyr-mfd-m5-acceptance.md:562-566` verbatim.

| # | Target | Source directory | Overlay | Expected |
|---|---|---|---|---|
| 1 | `i2c_bridge` | `zephyr/samples/i2c_bridge` | automatic (`app.overlay`) | pass |
| 2 | `spi_nor_id` | `zephyr/samples/spi_nor_id` | automatic | pass |
| 3 | `spi_bridge` | `zephyr/samples/spi_bridge` | automatic | baseline-fail |
| 4 | `combined_i2c_spi_bridge` | `zephyr/samples/combined_i2c_spi_bridge` | automatic | baseline-fail |
| 5 | `m5_reset` | `zephyr/tests/pdg_mfd_m5/reset_subscriptions` | `reset.overlay` | pass |
| 6 | `m5_jumper` | `zephyr/tests/pdg_mfd_m5/jumper_preflight` | `jumper.overlay` | pass |
| 7 | `m5_acceptance` | `zephyr/tests/pdg_mfd_m5/acceptance` | `acceptance.overlay` | pass |
| 8 | `m5_teardown` | `zephyr/tests/pdg_mfd_m5/recovery_teardown` | `recovery.overlay` | pass |

Targets 3 and 4 fail because they instantiate an `is31fl3743b@0` node whose
`issi,is31fl3743b` driver is not in Zephyr `main`; only `is31fl319x`,
`is31fl3216a` and `is31fl3733` are upstream. `zephyr/README.md` documents this.
That compatible string appears in exactly two files in the repository, both
`app.overlay` files of targets 3 and 4.

### 4.1 Expected translation units per target

Derived from each target's overlay, which decides the `DT_HAS_*_ENABLED` Kconfig
defaults. Used by the §5.1 non-vacuity assertion.

| Target | Zephyr-side objects | native_simulator-side objects |
|---|---|---|
| 1 `i2c_bridge` | `pdg_mfd`, `pdg_i2c` | `common`, `gallo_registry`, `pdg_i2c_bottom` |
| 2 `spi_nor_id` | `pdg_mfd`, `pdg_gpio`, `pdg_spi` | `common`, `gallo_registry`, `pdg_gpio_bottom`, `pdg_spi_bottom` |
| 5 `m5_reset` | `pdg_mfd` | `common`, `gallo_registry`, `m5_bottom` |
| 6 `m5_jumper` | `pdg_mfd`, `pdg_gpio` | `common`, `gallo_registry`, `pdg_gpio_bottom` |
| 7 `m5_acceptance` | `pdg_mfd`, `pdg_gpio`, `pdg_spi` | `common`, `gallo_registry`, `pdg_gpio_bottom`, `pdg_spi_bottom`, `m5_bottom` |
| 8 `m5_teardown` | `pdg_mfd`, `pdg_gpio`, `pdg_spi` | `common`, `gallo_registry`, `pdg_gpio_bottom`, `pdg_spi_bottom`, `m5_bottom` |

Target 6 deliberately does not link `m5_bottom.c`; its `CMakeLists.txt` says so
explicitly ("This image uses only the Zephyr GPIO API. It needs no host-context
shim"). Target 5 sets `CONFIG_GPIO=n` and `CONFIG_SPI=n` in `prj.conf`.

The two halves are detected by different means, following M4's measured
practice. Zephyr-side translation units appear in the build's
`compile_commands.json`; native_simulator-side units do not, because the runner
is a separate sub-build, so those are located as object files. Object-file
suffix is matched tolerantly (`.o` or `.obj`) because it varies by generator and
platform.

## 5. Assertion contracts

All four mechanisms below are taken from
`docs/superpowers/specs/2026-08-19-zephyr-mfd-m4-acceptance.md` (checks A-01,
A-08 and A-11) and `2026-08-17-zephyr-mfd-m3-gpio-tests.md` §"Ordinal
resolution". They are reused verbatim rather than reinvented, because they were
executed against real builds of this module and this design cannot be executed
locally before merge.

### 5.1 Pass targets (1, 2, 5, 6, 7, 8)

1. `west build` exits zero.
2. `libpico_de_gallo_ffi.a` exists under the build directory — proves Corrosion
   built the crate and cbindgen generated the header.
3. `grep -o 'pdg_[a-z0-9_]*\.c' <build>/compile_commands.json | sort -u` contains
   every Zephyr-side translation unit in the target's §4.1 row, and contains
   none of the other three driver translation units. This is M4 A-01's
   mechanism.
4. Every native_simulator-side object in the §4.1 row is found under the build
   directory by name — this is the half that actually calls `gallo_*`. M4 A-08
   locates the same objects this way.
5. `<build>/zephyr/.config` contains `CONFIG_MFD_PICO_DE_GALLO=y` plus the
   `CONFIG_{GPIO,I2C,SPI}_PICO_DE_GALLO=y` entries implied by the target's §4.1
   row. A Kconfig symbol silently dropping to `n` is the precise mechanism by
   which a vacuous pass would occur, so it is asserted directly rather than
   inferred from the compiled set.

Assertion 3 is deliberately two-sided rather than a plain containment check: a
target that compiles *more* drivers than its overlay enables signals a broken
overlay just as much as one that compiles fewer.

It is scoped to the four driver translation units — `pdg_mfd.c`, `pdg_gpio.c`,
`pdg_i2c.c`, `pdg_spi.c` — rather than asserted as whole-file set equality,
because M4 A-01 recorded only that `compile_commands.json` *contains* those
names. It did not establish whether the native_simulator sub-build contributes
its own `pdg_*_bottom.c` entries to the same file. Asserting equality over every
`pdg_*.c` match would therefore encode an unverified assumption about the
sub-build's layout, and would fail for a reason unrelated to the property under
test. Assertion 4 covers the bottom half independently and by a different
mechanism.

### 5.2 Baseline-failure targets (3, 4)

1. `west build` exits **non-zero**. A zero exit is a failure of this gate: it means
   the IS31FL3743B driver reached upstream Zephyr and this branch of the script,
   plus `zephyr/README.md`, must be revisited.
2. `<build>/zephyr/zephyr.elf` exists — proves the failure is at the
   native_simulator runner link producing `zephyr.exe`, not earlier.
3. The build log contains exactly one distinct undefined `__device_dts_ord_N`
   symbol.
4. `N` equals the ordinal resolved for the `is31fl3743b@0` node from
   `<build>/zephyr/include/generated/zephyr/devicetree_generated.h`.

Step 4 reuses M3's measured resolution idiom, which maps ordinal to node rather
than the reverse:

```bash
grep -o '__device_dts_ord_[0-9]*' "$log" | sort -u | while read -r s; do
  n=${s##*_}
  grep -n "DT_N_.*ORD $n\$" "$build"/zephyr/include/generated/zephyr/devicetree_generated.h
done
```

The trailing `$` anchor is load-bearing: it pins the match to the `_ORD` define
whose value is the bare ordinal, excluding the sibling `_ORD_STR_SORTABLE`
define, whose value is a quoted string. The resolved define name embeds the
node's full devicetree path, which the shield can change, so the resulting name
is then required to contain `is31fl3743b`. M4 A-11 recorded the resolved path as
`/pico-de-gallo/spi/is31fl3743b@0` for both targets, and recorded distinct
ordinals for them (49 and 50), which is why cross-sample ordinal identity must
never be asserted.

Steps 3 and 4 together are the per-sample attribution D5 requires, expressed
without reference to any literal ordinal.

## 6. Workflow structure

`.github/workflows/zephyr.yml`, one job, `runs-on: ubuntu-latest`, no container.
House style is followed: a top-of-file comment block explaining why the workflow
exists, `permissions: contents: read`, the standard `concurrency` block from
`check.yml`, and a `name:` following the `platform / toolchain / purpose` idiom
(`ubuntu / host / zephyr`).

No Zephyr SDK is installed. `ZEPHYR_TOOLCHAIN_VARIANT=host` builds `native_sim`
with the host compiler, and `native/64` avoids `gcc-multilib`. This removes the
single most expensive component of a conventional Zephyr CI setup and is why a
plain runner suffices where the repository otherwise uses containers only for
`docker://rhysd/actionlint`.

Steps, in order:

1. `actions/checkout@v7` with `submodules: true`, matching house style.
2. Assert `zephyr/README.md` contains the pinned SHA (D6).
3. Install apt build dependencies with `--no-install-recommends`.
4. Install `west` via pip.
5. `actions/cache` over the west workspace, keyed on the pinned SHA. The key is
   stable because the revision is pinned, so steady-state runs restore rather than
   clone.
6. On cache miss only: `west init -m <zephyr> --mr <SHA>` followed by
   `west update --narrow -o=--filter=blob:none`, the fast path documented in
   `zephyr/README.md`.
7. `dtolnay/rust-toolchain@stable`.
8. Run `zephyr/scripts/ci-build.sh`.

`actions/cache` is a deliberate deviation from house style, which uses no caching
anywhere. The justification is that a Zephyr west workspace is multi-gigabyte and
the cache key is exactly stable under D1.

The script is invoked from the west workspace root with absolute source paths into
this repository, rather than relying on west discovering its topdir by walking up
from the repository directory. This sidesteps west workspace resolution entirely.

## 7. Verification

### 7.1 Static

- `actionlint` on the new workflow. The repository baseline is clean (exit 0
  across all ten existing workflows), so any finding is attributable to this
  change. `check.yml` also runs `actionlint` in CI.
- `shellcheck -S warning` on the new script. The repository baseline is clean at
  that severity for the existing 600-line `run-m5.sh`, so the new script is held
  to the same bar.
- `dos2unix` on both new files, per AGENTS.md §3. CRLF in a workflow `run:` block
  breaks `actionlint` with `unexpected character $'\r'`.

### 7.2 Mutation check

Issue #130's acceptance criterion 3 requires demonstrating that a deliberate FFI
symbol rename fails the job. No Zephyr environment is available locally, so the
demonstration is performed by CI, using this protocol agreed with the maintainer:

1. Push the clean branch and confirm the workflow passes.
2. Add a commit renaming a `gallo_*` symbol in `pico-de-gallo-ffi`.
3. Push; confirm the workflow fails at the native_simulator runner link.
4. `git reset --hard` back to the clean commit and `git push --force-with-lease`.
5. Link the failing run in the pull request body as evidence.

The mutated symbol is `gallo_gpio_unsubscribe`, chosen because it is called only
from `zephyr/tests/pdg_mfd_m5/common/m5_bottom.c` and not from any sample.
A green result under that mutation would therefore also prove targets 5 through 8
are vacuous, making the mutation a simultaneous check of D4.

The mutation is never merged.

### 7.3 Not verifiable before first CI run

Everything else. The first CI run is the first execution of both new files.

Reusing M3's and M4's measured idioms (§5) removes the largest category of
guesswork, since those greps ran against real builds of this module. What remains
unverified is the surrounding scaffolding rather than the assertion logic. In
descending order of expected probability:

1. A missing or misnamed apt package in the runner setup.
2. `west` workspace or `ZEPHYR_BASE` resolution, mitigated by invoking builds
   from the workspace root with absolute source paths.
3. The native_simulator object-file path or suffix in §5.1 assertion 4, which is
   the one place a `find` by name could come up empty for a benign reason.
4. Cache key or restore behaviour on the first populated run.

None of these are silent-pass failures: each fails loudly and visibly. The
failure mode this design most needs to avoid — a green run that proves nothing —
is guarded by §5.1 assertions 3 and 5, and independently by the §7.2 mutation
check.

## 8. Follow-ups, deliberately not in this change

- **Weekly non-blocking `main` tracker.** Pin rot is D1's accepted cost. A
  `schedule:` job building against Zephyr `main` with `continue-on-error: true`
  would surface upstream drift without failing pull requests. Deferred because
  #130's acceptance criteria say nothing about upstream drift, and adding a
  scheduled job that has never been observed to pass is poor practice.
- **Twister metadata and hardware-free unit tests.** Issue #109 covers
  `sample.yaml` and `testcase.yaml` files, the dangling `tests:` and
  `snippet_root:` entries in `zephyr/module.yml`, and unit tests for the pure
  helper functions. This change deliberately does not add twister metadata; it
  drives `west build` directly.

  > **Partly overtaken 2026-08-27.** Two of those premises were already stale
  > when this was written, and a third was wrong. `zephyr/tests/` exists, so
  > only `snippet_root:` dangled. And upstream has retired *both* `sample.yaml`
  > and `testcase.yaml` in favour of `tests.yaml`
  > (`filename:sample.yaml repo:zephyrproject-rtos/zephyr` → 0 results;
  > `filename:testcase.yaml path:tests/drivers` → 0, against 268 `tests.yaml`).
  > Twister metadata and a `twister` CI job landed under #109; the unit tests
  > for the helpers remain outstanding.
- **Hardware runs.** `run-m5.sh` remains a manual, board-attached procedure.

## 9. Documentation obligations

Per AGENTS.md §15.1, and noting the §15.1 carve-out that `zephyr/` has no book
chapter:

- `zephyr/README.md` — record the pinned revision including the full SHA required
  by D6, and add a short section pointing at the workflow and the script, stating
  plainly that the gate is build-only and confers no runtime coverage.
- `zephyr/CHANGELOG.md` — a Keep a Changelog entry.
- `AGENTS.md` — four sites become false or incomplete once this lands, verified
  against `main` @ `2089a4f`:

  | Site | Current text | Required change |
  |---|---|---|
  | §5.3 table, line 231 | ends at `nostd.yml` | add a `zephyr.yml` row |
  | §5.4 catalog, lines 237-245 | no Zephyr entry | add a `zephyr.yml` row: trigger, and that it is build-only |
  | §15.1, lines 907-913 | "**Neither gate is automatic.** No CI job builds, compiles, or lints the Zephyr module (#130) … Until #130 lands … Do not treat a green CI run as evidence that `zephyr/` still builds." | rewrite: both gates now fire in CI on the filtered paths; a green run **is** evidence the module still builds, and is **not** evidence it still works on hardware |
  | §15.1 checklist item 7, lines 957-960 | "Do not accept 'CI is green' as evidence — no CI job builds the Zephyr module (#130)." | rewrite: a green `zephyr.yml` run is now acceptable evidence for compile and link breakage; behavioural claims still require a hardware run |

  The §15.1 rewrite must not overcorrect. The distinction the paragraph is
  protecting — that compiling is not the same as working — remains true and is
  the whole point of §2.3. Only the "nothing builds it at all" claim becomes
  false.

No `book/src/**` change is required. The §15.1 carve-out is explicit that
`zephyr/README.md` and `zephyr/CHANGELOG.md` satisfy the parity rule for a
Zephyr-only change, and this change alters nothing the book describes.
