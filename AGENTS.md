# AGENTS.md — Pico de Gallo

This file is for AI coding agents (Claude, Codex, Cursor, Cline,
Aider, Continue, GitHub Copilot, etc.) working in this repository.
It exists so an agent can come in cold and avoid the same dozen
mistakes humans have already made.

If you only have time to read one section, read **§4 Hard rules** and
**§13 Common gotchas**.

---

## 1. What Pico de Gallo is

Pico de Gallo turns a [Raspberry Pi Pico 2](https://www.raspberrypi.com/products/raspberry-pi-pico-2/)
(RP2350) into a USB-attached protocol bridge: **I²C, SPI, UART, GPIO,
PWM, ADC, 1-Wire**. The firmware speaks
[postcard-rpc](https://docs.rs/postcard-rpc) over USB; host code
(Rust, C, Python, or the `gallo` CLI) calls strongly-typed RPCs and
gets responses back.

The point is to let you develop and test embedded drivers
**on your laptop** without cross-compiling and flashing every change.
That makes the wire protocol the single most important contract in
the project — see §6.

---

## 2. Repository layout

```text
.
├── AGENTS.md                        # ← you are here
├── README.md, ROADMAP.md
├── CONTRIBUTING.md, SECURITY.md, CODE_OF_CONDUCT.md, CODEOWNERS
├── LICENSE                          # MIT
├── .gitattributes                   # LF EOL enforcement (see §3)
├── deny.toml                        # cargo-deny policy
├── .github/
│   ├── workflows/                   # CI (check, nostd, gh-pages, release-*)
│   ├── ISSUE_TEMPLATE/              # Issue forms
│   ├── DISCUSSION_TEMPLATE/         # Discussion forms
│   ├── pull_request_template.md
│   ├── copilot-instructions.md      # Detailed agent reference (read it)
│   └── RELEASE.md                   # manual release playbook
├── book/                            # mdBook → balbi.sh/pico-de-gallo/
├── docs/                            # design specs, plans, agent guides (NOT the book)
├── hardware/                        # KiCad landing-board PCB
├── case/                            # FreeCAD enclosure
├── zephyr/                          # Zephyr module — documented in its own README (§15.1)
├── Cargo.toml                       # HOST workspace (7 members)
├── Cargo.lock                       # COMMITTED — keep it in sync
└── crates/
    ├── pico-de-gallo-internal/      # Wire protocol types (postcard-rpc)
    ├── pico-de-gallo-lib/           # Async host library (nusb + tokio)
    ├── pico-de-gallo-hal/           # embedded-hal trait impls
    ├── pico-de-gallo-ffi/           # C FFI (cdylib + cbindgen → pico_de_gallo.h)
    ├── pico-de-gallo-app/           # CLI — binary name is `gallo`
    ├── pico-de-gallo-mcp/           # MCP server — binary name is `gallo-mcp`
    ├── pyco-de-gallo/               # Python bindings (PyO3 + maturin)
    └── pico-de-gallo-firmware/      # SEPARATE workspace, no_std, RP2350
        └── Cargo.lock               # ALSO committed — separate from host lock
```

There are **two** Cargo workspaces. The host workspace manifest
lives at the repository root (`./Cargo.toml`) and includes the seven
host crates under `crates/`. The firmware workspace is deliberately
separate because it targets `thumbv8m.main-none-eabihf` and pulls
in no_std-only deps — its own `Cargo.toml` is at
`crates/pico-de-gallo-firmware/Cargo.toml`. Do not try to add the
firmware to the host workspace — it will break.

Run `cargo` commands for the host workspace from the repository
root. Firmware commands run from `crates/pico-de-gallo-firmware/`.

---

## 3. File and EOL conventions

**All text files use LF line endings.** `.gitattributes` has
`* text=auto eol=lf` plus explicit overrides for `.rs`, `.toml`,
`.md`, `.yml`, `.yaml`, `.json`, `.sh`, `.py`, `.h`, `.c`, `.lock`.

Why this matters for agents:

- CRLF in `run:` blocks of GitHub Actions workflows silently breaks
  `actionlint` and `shellcheck` (`unexpected character $'\r'`).
- CRLF in source files produces noisy whole-file diffs that drown
  out the actual change.
- Git will renormalize on commit, but the working tree may still show
  CRLF, which trips other tooling.

**What to do whenever you create a file on Windows:**

```powershell
dos2unix path/to/your/new-file.yml
```

(`dos2unix` is installed; it's on `PATH` via Strawberry Perl.) On
Linux/macOS the editor will usually do the right thing, but it costs
nothing to run `dos2unix` anyway.

`.FCStd`, `.uf2`, `.elf`, `.so`, `.dll`, `.dylib`, `.png`, `.pdf`,
etc. are marked **binary** — never line-end them. `.kicad_*` files
are S-expressions / JSON and are tracked as **text with forced LF**
so `git diff`, `git blame`, and three-way merge work on hardware
changes; run `dos2unix` on any KiCad file you somehow ended up
editing on Windows with CRLF.

---

## 4. Hard rules (don't break these)

1. **LF endings on every text file.** Run `dos2unix` if you're not
   sure. See §3.
2. **Never reorder enum variants in `pico-de-gallo-internal`.**
   postcard serializes enums by *variant index*, not discriminant.
   Reordering is a silent wire-protocol break. See §6.
3. **Commit `Cargo.lock` alongside any `Cargo.toml` change.** Both
   workspaces have a committed lock file. CI's `lockfile` job will
   fail any PR that splits them apart.
4. **Always pass `--locked` when validating dependency changes.** A
   bare `cargo build` happily resolves new transitive versions and
   hides regressions until release day (see §13, embassy-usb-driver
   0.2.1).
5. **Firmware logs with `defmt` only.** No `log`, no `println!`, no
   `eprintln!` — that crate is `no_std`.
6. **Conventional Commits with a crate scope.** Used for readable
   history, scoping, and CHANGELOG authoring. See §10.
7. **AI-assisted commits include `Co-authored-by: Copilot` and
   `Assisted-by:` trailers; NEVER `Signed-off-by:`.** Only humans
   may certify the DCO.
8. **Don't push or force-push without explicit user permission.** If
   you amend a commit, use `git push --force-with-lease` and only
   after the user asks for it.
9. **Don't squash-merge.** Clean history is project policy. Each
   commit must build cleanly on its own.
10. **Canonical repository is `OpenDevicePartnership/pico-de-gallo`**
    (the `upstream` git remote). The `origin` remote on this checkout
    points at the maintainer's personal fork. All docs, templates,
    and links should use `OpenDevicePartnership/...`.
11. **Book and code must stay in sync.** Every PR — human or
    AI-authored — has to land both the code change *and* the
    matching `book/` update in the same logical change. See §15.1
    for the parity rule, the per-area mapping, and the reviewer
    checklist.
12. **Version bumps are a deliberate, manual release step — never a
    drive-by edit.** There is no release automation anymore
    (release-please was removed; see §12 and `.github/RELEASE.md`).
    A crate's `[package].version` in its own `Cargo.toml` is the sole
    source of truth. Do **not** bump `[package].version` inside an
    ordinary feature/fix PR — land the change with a Conventional
    Commit and let a maintainer cut the release separately. When you
    *are* deliberately cutting a release, a version bump is never
    isolated: in the **same commit** you must also (a) bump the
    matching `version = "..."` dep specs in every dependent crate
    (`lib`→`internal`; `hal`/`ffi`/`application`/`mcp`/`pyco`→`lib`;
    `firmware`→`internal`), (b) hand-write each released crate's
    `crates/<crate>/CHANGELOG.md` entry, (c) regenerate **both**
    `Cargo.lock`s, and (d) after merge, push the per-component tags
    that fire the publish workflows.
    Bumping a version without its dep specs ships a crate to crates.io
    that resolves a stale sibling; forgetting the firmware lock fails
    CI's `lockfile` job. Follow the checklist in §12 / `.github/RELEASE.md`.

---

## 5. Build, lint, test (mirror CI exactly)

CI in `.github/workflows/check.yml` runs each job **per crate** with
`working-directory: crates/<crate>`. The workspace-level shortcuts
work locally, but per-crate failures are what CI gates on, so when
something fails reproduce it per crate.

### 5.1 Host crates

The host matrix is `pico-de-gallo-{app,internal,ffi,hal,lib,mcp}` and
`pyco-de-gallo`.

```bash
# Per-crate (matches CI):
cd crates/<crate>
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo hack --feature-powerset check
cargo +1.90 check                    # MSRV
RUSTDOCFLAGS=--cfg docsrs cargo +nightly doc --no-deps --all-features
```

```bash
# Workspace shortcuts (local convenience, not CI — run from repo root):
cargo fmt --all
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo check --workspace --locked        # lockfile drift guard
cargo deny --manifest-path Cargo.toml check
```

### 5.2 Firmware (separate workspace, no_std)

Two mutually exclusive hardware-revision features: **`hw-rev2`**
(default) and **`hw-rev1`** (deprecated — removal no earlier than
2031-09-01; `build.rs` prints a `cargo:warning` and `main()` logs a
`defmt::warn!` at boot when it is enabled). `nostd.yml` builds and
lints both. If you touch firmware, do the same locally:

```bash
cd crates/pico-de-gallo-firmware

# hw-rev2 (default)
cargo fmt --check
cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf

# hw-rev1 (deprecated — must opt in explicitly)
cargo clippy --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1 -- -D warnings
cargo build --release --locked --target thumbv8m.main-none-eabihf \
    --no-default-features --features hw-rev1
```

The release-mode firmware binary is named `pico-de-gallo-firmware`.

> **Trap:** `nostd.yml` and `release-firmware.yml` each carry a
> `[rev1, rev2]` matrix in which exactly one entry rides the default
> features. Swapping those `feature-flags` values makes
> `firmware-rev1.uf2` silently contain a **rev2** image, which flashes
> onto a v1.0 board with the wrong pinout and no error. If you change
> the default feature, change both matrices and verify by inspecting
> the produced binaries, not by reading the diff.

### 5.3 Other CI jobs to be aware of

| Job                | Purpose                                                                                                                                        |
|--------------------|------------------------------------------------------------------------------------------------------------------------------------------------|
| `lockfile`         | `cargo check --locked` in both workspaces — fails if `Cargo.toml` and `Cargo.lock` disagree.                                                   |
| `semver`           | `cargo-semver-checks` on `pico-de-gallo-internal` (the published wire crate).                                                                  |
| `deny`             | `cargo-deny check bans licenses sources advisories` in both workspaces.                                                                        |
| `actionlint`       | Lints every `.github/workflows/*.yml`. CRLF kills it, so does bad matrix syntax.                                                               |
| `nostd.yml`        | Builds firmware for both `hw-rev1` and `hw-rev2`.                                                                                              |
| `zephyr.yml`       | Builds the Zephyr module on `native_sim/native/64` against a pinned Zephyr revision. Build-only except the hardware-free `pdg_fake` suite, which twister runs. |

### 5.4 Full CI workflow catalog

| Workflow                  | Trigger                            | What it does                                                                                                |
|---------------------------|------------------------------------|-------------------------------------------------------------------------------------------------------------|
| `check.yml`               | Push to `main`, PRs                | fmt, clippy, doc, hack (feature powerset), test, msrv, **lockfile drift**, **actionlint**, **cargo-deny**, **cargo-semver-checks** |
| `nostd.yml`               | Push to `main`, PRs                | Firmware compiles + clippy for `thumbv8m.main-none-eabihf`, both `hw-rev1` and `hw-rev2`                    |
| `zephyr.yml`              | Push to `main`, PRs (path-filtered)| Builds `zephyr/` for `native_sim/native/64` at a pinned Zephyr revision: 2 samples pass, 2 IS31 samples asserted to fail at baseline, 4 M5 apps, 1 I2C gather-write test. **Build-only**, except `tests/pdg_fake/i2c`, which omits `build_only` and is executed by twister against a recording fake |
| `gh-pages.yml`            | Push to `main`                     | Builds and deploys the mdBook docs to GitHub Pages                                                          |
| `release-application.yml` | `application-v*` tags              | Builds `gallo` for Linux/Windows/macOS                                                                      |
| `release-ffi.yml`         | `ffi-v*` tags                      | Builds `.so` / `.dll` / `.dylib` + C header                                                                 |
| `release-firmware.yml`    | `firmware-v*` tags **and PRs**     | Builds `.uf2` and `.elf`. PR runs are **build-only** (skip-upload) so tooling breakage is caught at PR time |
| `release-hardware.yml`    | `hardware-v*` tags                 | KiCad ERC/DRC, gerbers, schematic PDF                                                                       |
| `release-pyco.yml`        | `pyco-v*` tags                     | Builds Python wheels (CPython 3.8–3.14, Linux/Win/macOS), attaches to GitHub Release                        |
| `release-crates.yml`      | `internal-v*`, `library-v*`, `hal-v*`, `ffi-v*`, `application-v*`, `mcp-v*` tags | Publishes the matching crate to crates.io                     |

### 5.5 Test baseline

About **613 unit tests + 7 doctests** across the host workspace,
measured 2026-09-01:

| Crate                    | Passing | `#[ignore]`d |
|--------------------------|---------|--------------|
| `pico-de-gallo-internal` | 168     | 0            |
| `pico-de-gallo-ffi`      | 128     | 0            |
| `gallo-mcp`              | 114     | 7            |
| `pico-de-gallo-lib`      | 77      | 4            |
| `gallo`                  | 74      | 0            |
| `pico-de-gallo-hal`      | 43      | 0            |
| `pyco-de-gallo`          | 9       | 0            |

Counts are measured from the workspace root (`cargo test --locked`),
which unifies features and therefore enables `pico-de-gallo-internal`'s
`use-std` tests. Running `cargo test -p pico-de-gallo-internal` alone
reports 141, because some tests are gated on that feature — see §13.14.

Doctests: `pico-de-gallo-lib` 4, `pico-de-gallo-hal` 2,
`pico-de-gallo-internal` 1.

All 11 ignored tests are board-attached and therefore never run in CI:

- The 7 in `gallo-mcp` need **two** boards, because they cover
  per-call serial-number target selection. Run them with
  `cargo test -p gallo-mcp -- --ignored`.
- The 4 in `pico-de-gallo-lib` are the #135 zero-length-write
  regression tests. They need one board, and
  `empty_batch_write_never_reaches_the_bus` additionally needs a
  TMP102-like target on the I2C bus. Run them with
  `cargo test -p pico-de-gallo-lib -- --ignored`.

If you add code, add tests next to it; round-trip serialization tests
are the norm for wire types.

> **Trap:** `pico-de-gallo-internal` without the `use-std` feature
> fails on the `vec!` macro. Test it via the workspace or with
> `--features use-std`.

---

## 6. Wire protocol — CRITICAL

All protocol types live in `pico-de-gallo-internal`. The firmware
and every host crate depend on it. Get this wrong and devices in the
field stop talking to your new release.

### 6.1 Enum ordering is ABI

postcard serializes enums by **variant index** (0, 1, 2, …), **not**
by discriminant value. Therefore:

- Never reorder variants in any `#[derive(Serialize, Deserialize)]`
  enum in `pico-de-gallo-internal`.
- Adding a new variant is safe **only at the end**.
- Removing or renaming an existing variant is a breaking change.
- The relevant enums have `// WARNING: Do not reorder...` comments
  on them — **preserve those comments**.

### 6.2 Schema version

`pico-de-gallo-internal/build.rs` derives `SCHEMA_VERSION_MAJOR`,
`SCHEMA_VERSION_MINOR`, `SCHEMA_VERSION_PATCH` from the crate's
`[package].version`. **Do not edit these constants directly** — bump
the crate version and let `build.rs` regenerate them.

`PicoDeGallo::validate()` (host side) compares the firmware-reported
schema version to the host's compiled-in version. Pre-1.0, the
**minor** version is the breaking-change axis. Bump it whenever:

- you add or remove an endpoint or topic,
- you change a request/response type,
- you append a variant to a wire enum (even though append-only is
  technically non-breaking on the wire, host validation is strict).

### 6.3 Endpoint catalog

If you add, remove, or rename an endpoint, **update this table in the
same commit**.

| Path                     | Description                                             |
|--------------------------|---------------------------------------------------------|
| `"ping"`                 | Echo a u32 (testing)                                    |
| `"version"`              | Get firmware version                                    |
| `"device/info"`          | Get firmware version, schema version, capabilities, runtime GPIO count, build identity |
| `"i2c/read"`             | I²C read                                                |
| `"i2c/write"`            | I²C write                                               |
| `"i2c/write-read"`       | I²C write-then-read                                     |
| `"i2c/scan"`             | Scan I²C bus for responding addresses                   |
| `"i2c/batch"`            | Execute a batch of I²C operations                       |
| `"i2c/set-config"`       | Configure I²C (`I2cFrequency` enum)                     |
| `"i2c/get-config"`       | Query current I²C frequency                             |
| `"spi/read"`             | SPI read                                                |
| `"spi/write"`            | SPI write                                               |
| `"spi/flush"`            | SPI flush                                               |
| `"spi/transfer"`         | SPI full-duplex transfer                                |
| `"spi/batch"`            | SPI batch under chip-select (read/write/transfer/delay) |
| `"spi/set-config"`       | Configure SPI (frequency, phase, polarity)              |
| `"spi/get-config"`       | Query current SPI configuration                         |
| `"uart/read"`            | UART read with timeout                                  |
| `"uart/write"`           | UART write                                              |
| `"uart/flush"`           | Flush UART TX buffer                                    |
| `"uart/set-config"`      | Configure UART (baud rate)                              |
| `"uart/get-config"`      | Query current UART configuration                        |
| `"gpio/get"`             | Read GPIO pin                                           |
| `"gpio/put"`             | Set GPIO pin                                            |
| `"gpio/wait-high"`       | Wait for GPIO high                                      |
| `"gpio/wait-low"`        | Wait for GPIO low                                       |
| `"gpio/wait-rising"`     | Wait for rising edge                                    |
| `"gpio/wait-falling"`    | Wait for falling edge                                   |
| `"gpio/wait-any"`        | Wait for any edge                                       |
| `"gpio/set-config"`      | Configure GPIO direction and pull                       |
| `"gpio/subscribe"`       | Subscribe to push-based GPIO edge events                |
| `"gpio/unsubscribe"`     | Unsubscribe from GPIO edge events                       |
| `"pwm/set-duty-cycle"`   | Set raw PWM compare value                               |
| `"pwm/get-duty-cycle"`   | Query current duty cycle and max                        |
| `"pwm/enable"`           | Enable PWM slice owning the channel                     |
| `"pwm/disable"`          | Disable PWM slice owning the channel                    |
| `"pwm/set-config"`       | Configure PWM frequency / phase-correct                 |
| `"pwm/get-config"`       | Query PWM configuration                                 |
| `"adc/read"`             | Single-shot ADC read                                    |
| `"adc/get-config"`       | Query ADC capabilities                                  |
| `"onewire/reset"`        | 1-Wire reset + presence detection                       |
| `"onewire/read"`         | 1-Wire read                                             |
| `"onewire/write"`        | 1-Wire write                                            |
| `"onewire/write-pullup"` | 1-Wire write + strong pullup (parasitic power)          |
| `"onewire/search"`       | Start 1-Wire ROM search                                 |
| `"onewire/search-next"`  | Continue 1-Wire ROM search                              |
| `"system/reset-subscriptions"` | Tear down all GPIO subscriptions (host calls on connect) |

### 6.4 Topics (server → client push)

| Path           | Direction       | Message     | Description                  |
|----------------|-----------------|-------------|------------------------------|
| `"gpio/event"` | server → client | `GpioEvent` | Push stream of GPIO edges    |

Endpoints use the `endpoints!` macro with path strings. Response
types use `#[cfg(feature = "use-std")]` to switch between `Vec<u8>`
(host) and `&[u8]` (firmware).

### 6.5 Lockstep release rule

A wire-protocol change requires bumping in the **same release cycle**:

1. `pico-de-gallo-internal` (with `feat!` / `BREAKING CHANGE:`),
2. `pico-de-gallo-firmware` (encodes the new schema version),
3. `pico-de-gallo-lib`, `pico-de-gallo-hal`, `pico-de-gallo-ffi`,
   `pico-de-gallo-app`, `pico-de-gallo-mcp`, `pyco-de-gallo` (so every
   host surface sees the new types).

**Version bumps are manual.** There is no release automation
(release-please was removed — see §12 and `.github/RELEASE.md`). When
the wire protocol changes, a maintainer cuts a release that, in one
commit, bumps `[package].version` on **all eight** released crates
(internal, library, hal, ffi, application, mcp, pyco, firmware) to the
same new version (pre-1.0: a minor bump for any wire change), because
they are wire-coupled and must never drift in version space.

That single release commit must also:

- **Rewrite every cross-crate dep spec** by hand — nothing rewrites
  them for you now. Each dependent's `pico-de-gallo-X = { version =
  "Y.Z", path = "..." }` spec must point at the new version:
  - `lib` → `internal`
  - `hal`, `ffi`, `application`, `mcp`, `pyco` → `lib`
  - `firmware` → `internal` (separate workspace — easy to forget)
- **Hand-write each released crate's `crates/<crate>/CHANGELOG.md`**
  entry (Keep a Changelog). There is no root `CHANGELOG.md`; every
  crate owns its own, and `zephyr/CHANGELOG.md` covers the Zephyr
  module.
- **Regenerate both `Cargo.lock`s** (`cargo update --workspace` at
  the repo root; `cargo update -p pico-de-gallo-internal` in
  `crates/pico-de-gallo-firmware`) and verify with `cargo check
  --locked` in both workspaces. CI's `lockfile` matrix fails if a
  lock is out of sync.

After the release commit merges, push the per-component tags
(`internal-v*`, `library-v*`, `hal-v*`, `ffi-v*`, `application-v*`,
`mcp-v*`, `pyco-v*`, `firmware-v*`) to fire the publish workflows. See §12 and
`.github/RELEASE.md` for the full checklist.

Nothing enforces wire coupling for you: bumping the version numbers
does not change the firmware's schema encoding. The firmware source
must carry the matching change, and `SCHEMA_VERSION_*` regenerates
from `internal`'s version via `build.rs` (§6.2).

#### Don't forget the firmware (separate workspace)

`crates/pico-de-gallo-firmware` is **excluded** from the host
workspace (it targets `thumbv8m.main-none-eabihf` and is no_std), so
its `pico-de-gallo-internal = { version = "X", path = "..." }` dep
spec and its own `Cargo.lock` are entirely separate from the host
workspace. In the same release commit:

1. Edit `crates/pico-de-gallo-firmware/Cargo.toml` so the
   `pico-de-gallo-internal` dep spec's `version = "..."` matches the
   new internal target (e.g. `0.6.0` → `0.7.0`), and bump the
   firmware's own `[package].version`.
2. Refresh the firmware lockfile:
   ```bash
   cd crates/pico-de-gallo-firmware
   cargo update -p pico-de-gallo-internal
   cargo check --locked --target thumbv8m.main-none-eabihf
   ```
3. Stage both edits into the release commit. CI's `lockfile` matrix
   will fail if either is missing.

---

## 7. Dependency discipline

### 7.1 The ritual

Whenever you change a `Cargo.toml` (add/remove dep, bump version,
add/remove a pin):

```bash
cd <workspace>                       # repo root (host) or crates/pico-de-gallo-firmware/ (firmware)
cargo check                          # updates Cargo.lock in place, minimally
cargo check --locked                 # confirm the lock is now consistent
git add Cargo.toml Cargo.lock
```

Commit `Cargo.toml` and `Cargo.lock` together. CI fails PRs that
split them.

**Do not delete the lockfile to regenerate it.** `rm -f Cargo.lock &&
cargo generate-lockfile` re-resolves the *entire* graph, so adding one
dependency silently bumps every unrelated transitive crate that has
published a newer semver-compatible version since the lock was written.
Observed on the #157 branch: adding a single already-present dep moved
30 unrelated crates, where a plain `cargo check` produced a one-line
delta. That inflates review surface, and — as the 2026-05-04
`embassy-usb-driver` row in §13.17 shows — an unnoticed transitive bump
is exactly how this project has broken the firmware build before.

`cargo check` performs the minimal edit that satisfies the new manifest
and leaves everything else pinned. Reach for a full regeneration only
when you actually intend a wholesale dependency refresh, and then say so
in the commit message.

### 7.2 Pinned dependency rationale

Every `=X.Y.Z` exact pin in any `Cargo.toml` is listed here with the
upstream issue/commit and a removal criterion. **If you add a new
exact pin, add a row here in the same commit.**

| Crate                    | Pin                              | Reason                                                                                                                                                                                                                                | Remove when                                                                                           |
|--------------------------|----------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------------------|
| `pico-de-gallo-firmware` | `embassy-usb-driver = "=0.2.0"`  | `0.2.1` bumped `embedded-io-async` 0.6 → 0.7, which breaks `embassy-usb 0.5.1`'s CDC-ACM `ErrorType` impl (creates two incompatible copies of `embedded-io-async` in the dep graph). `cargo-deny`'s `bans.multiple-versions` will warn. | embassy-usb 0.6 is reachable through postcard-rpc (currently it only ships `embassy-usb-0_5-server`). |

### 7.3 Hard constraints

- `embassy-usb` = **0.5** (postcard-rpc 0.12 requires it; do not bump
  to 0.6).
- `embassy-sync` = **0.7.2** (compat lock).
- `nusb` on **0.1.x** (postcard-rpc dep).
- `pyo3` on **0.28.x** (via maturin).
- `embassy-usb-driver` = **=0.2.0** in firmware (see §13).
- `embedded-io` / `embedded-io-async` in `pico-de-gallo-hal` are
  **multi-major by design**, not a single pinned version. The deps are
  renamed and optional (`embedded-io-06`, `embedded-io-async-06`,
  `embedded-io-07`, `embedded-io-async-07`) behind two additive
  features. `embedded-io-06` remains enabled by default for compatibility;
  `embedded-io-07` is opt-in. When a new major lands upstream, **add a
  feature and a cfg-gated impl set**
  for `Uart` — do not migrate the existing one, or you silently strand
  every driver written against the older traits. All impls delegate to
  the shared private helpers on `Uart`, and the whole feature powerset
  (including *neither* feature) must keep compiling warning-free; that
  is why `Uart` carries a `cfg_attr(not(any(...)), allow(dead_code))`.

### 7.4 cargo-deny

`deny.toml` has `bans.multiple-versions = "warn"`. If you introduce
a duplicate-major in the dep graph (as the embassy-usb-driver 0.2.1
break did), `deny` will flag it. Take that warning seriously — it
usually means an inner dep silently bumped a semver-incompatible
version.

---

## 8. FFI conventions

- **Opaque pointer:** `PicoDeGallo` is opaque. `gallo_init` creates,
  `gallo_free` destroys.
- Every function takes `*const PicoDeGallo` as first arg — **null
  check first.**
- Status codes are `#[repr(i32)]`. `Ok = 0`; all errors are
  **negative**.
- **Status code values are stable C ABI.** Never renumber existing
  codes; only append new ones.
- `I2cFrequency` is passed as `u8` (`0 = Standard`, `1 = Fast`,
  `2 = FastPlus`) with range validation. Same for `GpioDirection`,
  `GpioPull`, `GpioEdge`, and the two batch-op `tag` fields. The
  header also names these values via `GalloI2cFrequency`,
  `GalloGpioDirection`, `GalloGpioPull`, `GalloGpioEdge`,
  `GalloI2cBatchOpTag`, and `GalloSpiBatchOpTag` — the signatures
  still take `uint8_t`, so the enums are a convenience, not an ABI
  change.
- **Those enum values are stable C ABI too**, and must match the
  discriminants of the `pico-de-gallo-internal` wire enums they
  mirror (variant order there is itself ABI — see §6.1). The
  `config_enums_match_wire_enums` test enforces the correspondence.
- `GALLO_MAX_TRANSFER_SIZE` / `GALLO_MAX_BATCH_OPS` / `GALLO_NUM_GPIOS`
  mirror the `pico-de-gallo-internal` constants. **They must be written
  as literals** — cbindgen folds const initializers syntactically and
  silently emits nothing for `= lib::MAX_TRANSFER_SIZE`. `const`
  assertions next to them turn drift into a build failure.
- **cbindgen prunes types no exported signature references.** Anything
  not reachable from a `gallo_*` prototype must be listed in
  `[export] include` in `cbindgen.toml`, or it silently disappears
  from the header.
- `pico_de_gallo.h` is generated by cbindgen during build — don't
  hand-edit it.

---

## 9. Python (pyco-de-gallo) conventions

- Built with **PyO3 + maturin**. `pyproject.toml` declares
  `requires-python = ">=3.8"`.
- Module name in Python is `pyco_de_gallo`. Public types are exposed
  without a `Py` prefix (e.g. `I2cFrequency`, `DeviceInfo`). The
  internal lib types are imported with a `Lib` prefix
  (`LibI2cFrequency`, etc.) to avoid collisions.
- The `PycoDeGallo` class owns a Tokio `Runtime` and `block_on`s the
  underlying async — Python methods are synchronous.
- For `#[pyclass]` enums used in `Vec<T>` arguments, derive `Clone`
  and use `#[pyclass(from_py_object)]`. Without `Clone` PyO3 can't
  extract them.
- Every `#[pyfunction]`, `#[pymethods]`, `#[pyclass]` item needs a
  rustdoc comment — it becomes the Python `__doc__`. Prefer Google
  style (`Args:`/`Returns:`/`Raises:`) so Sphinx napoleon and
  Pyright render it well.
- Errors are converted via `PyRuntimeError::new_err(format!("{e}"))`.
- `pyco-de-gallo` is `publish = false` on crates.io. Wheels are
  published to PyPI via the release workflow.

---

## 10. Commit conventions

We use [Conventional Commits](https://www.conventionalcommits.org/)
with a crate scope. Format:

```text
<type>(<scope>)<!>: <subject>

<body wrapped at 72 chars, explaining what and why>

<trailers>
```

- **type:** `feat`, `fix`, `chore`, `docs`, `refactor`, `perf`,
  `test`, `build`, `ci`, `revert`. Use `!` (or `BREAKING CHANGE:`
  footer) for breaking changes.
- **scope:** `internal`, `lib`, `hal`, `ffi`, `application`, `mcp`,
  `pyco`, `firmware`, or `repo`. Non-crate subprojects have their own
  scope: `zephyr` for the Zephyr module under `zephyr/`. Multiple
  scopes are comma-separated: `feat(internal,firmware): ...`.
- **subject:** ≤72 chars, capitalized, imperative mood, no trailing
  period. 72 is the limit, not a target — prefer shorter when it
  costs no clarity, but do not truncate a subject into vagueness to
  chase 50.

Required trailers for AI-assisted commits:

```text
Assisted-by: GitHub Copilot:claude-opus-4.7
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
```

- The `Assisted-by:` value is `AGENT_NAME:MODEL_VERSION [TOOL …]`.
  Use the actual model you're running as (verify before composing —
  don't assume from a previous session).
- **Never add `Signed-off-by:` on AI-assisted commits.** DCO is for
  humans.
- The `Co-authored-by: Copilot <…>` line is required by repo policy
  for all AI-written commits, even from non-Copilot agents.

Wire-protocol commits **must** be marked breaking (see §6.4).

---

## 11. PR etiquette

- Open a **draft PR first**. Let CI run on it.
- Don't request review until **all checks are green**, especially
  `lockfile`, `deny`, `semver`, and `actionlint` — these catch the
  exact regressions that have bitten this project before.
- For dep bumps, mention which `Cargo.toml(s)` and `Cargo.lock(s)`
  are touched. Reviewers expect them in the same commit.
- Use the PR template (`.github/pull_request_template.md`). It bakes
  in the wire-protocol and Cargo.lock checklists.
- Don't squash-merge. Rebase or merge-commit only.
- `Co-authored-by: Copilot` trailer on every AI-written commit; no
  `Signed-off-by` on AI commits.

---

## 12. Release process (manual — read `.github/RELEASE.md`)

Releases are performed **by hand**. There is no release automation —
release-please was removed (issue #83) because it caused more
problems than it solved (version/manifest drift that silently
disabled releases, seven-PR fan-out, plugin interaction bugs). A
maintainer bumps versions, writes the per-crate CHANGELOGs, merges,
and pushes tags. The full checklist lives in
[`.github/RELEASE.md`](.github/RELEASE.md); the summary:

1. Bump `[package].version` in each crate being released, and the
   matching `version = "..."` dep specs in every dependent
   (`lib`→`internal`; `hal`/`ffi`/`application`/`mcp`/`pyco`→`lib`;
   `firmware`→`internal`).
2. Hand-write each released crate's `crates/<crate>/CHANGELOG.md`
   entry (Keep a Changelog). There is no root `CHANGELOG.md`.
3. Regenerate both `Cargo.lock`s and verify `cargo check --locked`
   in the host workspace and firmware workspace.
4. Commit (`chore(release): ...`), open a PR, get CI green, merge to
   `main` (no squash).
5. Tag the merged commit — one tag per component — and push the
   tags. The tag-triggered `release-*.yml` workflows then publish to
   crates.io / PyPI and build the binaries.

**Tag prefix glossary** (common typos hurt):

| Crate                    | Tag prefix       | Publishes to         |
|--------------------------|------------------|----------------------|
| `pico-de-gallo-internal` | `internal-v*`    | crates.io            |
| `pico-de-gallo-lib`      | `library-v*`     | crates.io            |
| `pico-de-gallo-hal`      | `hal-v*`         | crates.io            |
| `pico-de-gallo-ffi`      | `ffi-v*`         | crates.io            |
| `gallo` (CLI)            | `application-v*` | crates.io + binaries |
| `gallo-mcp` (MCP server) | `mcp-v*`         | crates.io            |
| `pyco-de-gallo`          | `pyco-v*`        | PyPI (wheels)        |
| `pico-de-gallo-firmware` | `firmware-v*`    | `.uf2` / `.elf`      |
| KiCad gerbers            | `hardware-v*`    | gerbers / PDF        |

Common typos that have bitten us: `lib-v*` (it's `library-v*`),
`app-v*` (it's `application-v*`), `fw-v*` (it's `firmware-v*`).

For crates.io, push `internal-v*` first and let it index (~60s)
before the dependent tags, or re-run the downstream publish jobs that
lose the indexing race (see `release-crates.yml`).

**Tag-triggered workflows use the workflow YAML as it existed at the
tagged commit, not at the tip of main.** If you rewrite a release
commit, you must delete and re-create the tag.

---

## 13. Common gotchas (learn from past pain)

Read this before you commit. Every entry here came from a real
regression.

### 13.1 CRLF on Windows

You created a file in PowerShell. `actionlint` fails with
`unexpected character $'\r'`, or `git diff` shows the whole file
changed. **Fix:** `dos2unix <file>` before committing.

### 13.2 Bare `cargo check` masking deps regressions

`cargo check` (without `--locked`) re-resolves the dependency graph
and pulls newer transitive versions, hiding upstream breakage. The
embassy-usb-driver 0.2.1 incident shipped because the agent's local
check used a stale lockfile and a fresh checkout pulled
`embedded-io-async` 0.7. **Always use `--locked`** when validating
dep changes.

### 13.3 Bumping a Cargo.toml without bumping the lock

CI's `lockfile` job will fail and the PR can't merge. **Fix:** run
`cargo check` in the affected workspace to update the lock in place,
then `cargo check --locked` to confirm, and commit both files together
(§7.1). Do **not** delete the lockfile — that re-resolves the whole
graph and hides unrelated transitive bumps.

### 13.4 Reordering enum variants in `pico-de-gallo-internal`

Existing devices in the field can no longer decode messages from new
hosts (or vice versa). There is **no warning** at build time —
postcard happily encodes whatever you give it. **Fix:** append-only.
Bump the schema version (minor pre-1.0). Coordinate firmware + all
host crates in the same release.

### 13.5 Writing the wrong git-remote URL into docs

`origin` may be a personal fork. The canonical repo is
**`OpenDevicePartnership/pico-de-gallo`** (= the `upstream` remote
on this checkout). All issue templates, docs, mdBook links, badges,
and READMEs should point there.

### 13.6 Forgetting AI attribution trailers

The repo requires `Co-authored-by: Copilot <…>` and `Assisted-by:`
trailers on AI-written commits. **Never** add `Signed-off-by:` from
an AI agent.

### 13.7 Using `println!` / `log` in firmware

The firmware is `no_std`. It only has `defmt` (over RTT). Anything
else won't compile.

### 13.8 Editing `SCHEMA_VERSION_*` constants directly

They're generated by `pico-de-gallo-internal/build.rs` from the
crate's `[package].version`. **Fix:** bump the crate version.

### 13.9 `elf2uf2-rs 2.2.0` on crates.io is stale

The release CI installs from git or uses picotool. Don't "fix" a
build by pinning to the crates.io version — it doesn't have
`--family`. `elf2uf2-rs --version` cannot distinguish the two builds:
both report `2.2.0`. Use `cargo install --list`; a correct installation
currently shows `elf2uf2-rs v2.2.0
(https://github.com/JoNil/elf2uf2-rs#f14bf2d9)`. Confirm that
`elf2uf2-rs --help` contains `--family` before relying on it for a
release. See `.github/copilot-instructions.md` "Known traps" for the
gory details.

### 13.10 `embassy-usb` bumped to 0.6

postcard-rpc 0.12 only ships `embassy-usb-0_5-server`. Don't bump
`embassy-usb` past 0.5 until postcard-rpc ships a 0.6 server.

### 13.11 Adding a `Cargo.toml` exact pin without documenting it

Every `=X.Y.Z` pin must be listed in
`.github/copilot-instructions.md` "Pinned dependency rationale"
with upstream link and removal criterion. Otherwise the next
contributor (or you, in three months) can't tell why it's there.

### 13.12 Squash-merging or rewriting clean history

Repo policy is one logical change per commit, each commit builds
cleanly, no squash-merge. If the user asks you to clean up
fix-up/typo commits, do it via interactive rebase **before** merge,
not by squashing on merge.

### 13.13 Force-pushing without permission

Especially over a release commit — that breaks tag-triggered
workflows (see §12). If you must amend a commit you already pushed,
ask the user, then use `--force-with-lease`, and re-tag if there
were tags pointing at the old SHA.

### 13.14 `pico-de-gallo-internal` `cargo test` without `--features use-std`

The `vec!` macro test fails because `alloc::vec!` isn't in scope
under `#![no_std]`. **Fix:** test via the workspace (`cargo test`
from the repository root) or pass `--features use-std`.

### 13.15 PyO3 `Vec<MyPyclassEnum>` without `Clone`

PyO3 cannot extract a `Vec<T>` of `#[pyclass]` enums unless `T:
Clone` and `from_py_object` is set. **Fix:** `#[derive(Clone)]` +
`#[pyclass(from_py_object)]`.

### 13.16 Shipping code without the matching book change

If your PR touches a CLI flag, endpoint, status code, FFI
function, Python binding, configuration enum, schema version, or
hardware-revision capability and the corresponding `book/src/...`
chapter is **not** in the same diff, the PR is incomplete. See
§15.1 for the parity rule, per-area mapping, and reviewer
checklist. Reviewers (including the GitHub Copilot reviewer)
should flag this as a blocker.

### 13.17 Past regressions log

When you fix a new regression, **add a one-line row here** so the
next agent doesn't repeat it.

| Date       | Trigger                                        | Symptom                                                                        | Fix                                                                                       |
|------------|------------------------------------------------|--------------------------------------------------------------------------------|-------------------------------------------------------------------------------------------|
| 2026-05-04 | `embassy-usb-driver 0.2.1` (transitive)        | `EndpointError: embedded_io_async::Error` trait bound fails on firmware build. | Pin `embassy-usb-driver = "=0.2.0"` in firmware Cargo.toml; commit firmware `Cargo.lock`. |
| 2026-05-04 | `elf2uf2-rs 2.2.0` on crates.io is stale       | Release CI fails: `--family` flag does not exist in published binary.          | Install elf2uf2-rs from git (`cargo install --git … --locked`) or revert to picotool.     |
| 2026-05-04 | Tag-triggered workflow uses tagged-commit YAML | After force-pushing the release commit, GitHub still ran the old workflow.     | Always re-tag after rewriting a release commit; verify with `git show <tag>:<workflow>`.  |
| 2026-05-29 | Host crash/kill while a GPIO subscription was active | Pin permanently owned by firmware monitor task until power cycle; new host got `PinMonitored`. An orphaned subscription survives across CLI invocations until an MCP reset or a power-cycle. | Added `system/reset-subscriptions` endpoint. `pico-de-gallo-lib` exposes it and `gallo-mcp` calls it after `validate()`; the `gallo` CLI never calls it. Lockstep schema bump (internal 0.5→0.6, lib 0.5→0.6, hal 0.5→0.6, ffi 0.6→0.7, app 0.6→0.7, pyco 0.2→0.3, firmware 0.9→0.10). |
| 2026-05-29 | release-please defaults (missing `bump-minor-pre-major`) | `feat!` on the 0.x `internal` crate caused release-please to propose `internal 1.0.0` (plus six sibling 1.0.0 release PRs). PR #48 was merged before the trap was spotted; only a repo ruleset blocking `Cannot create ref` prevented the `internal-v1.0.0` tag, GitHub Release, and crates.io publish from going out. | Reverted the version bump on `main` (54573fa); added `bump-minor-pre-major: true` and `bump-patch-for-minor-pre-major: true` to `.github/release-please-config.json` so `feat!` on a 0.x crate bumps the minor and `feat:` bumps the patch. Closed the stale 1.0.0 release PRs so release-please regenerates them at the correct minor bumps. |
| 2026-06-03 | `gpio_wait_for_*` on a never-transitioning pin after host crash | Firmware dispatcher wedged device-wide; every other endpoint queued behind the stuck handler until power-cycle (worse than the 2026-05-29 row — that one blocked one pin, this one bricks the device). Postcard-rpc 0.12 dispatches handlers serially on a shared `&mut Context`, so any `await` inside a handler blocks the whole `server.run()` loop. embassy-usb-driver 0.2.0 does NOT expose `wait_disconnected`, so a `select(edge, disconnect)` fix is not viable on the host-process-death path. A later, different dispatcher wedge (2026-08-19, SPI transfer framing) was observed to recover after USB re-enumeration rather than a power cycle; that observation was never tested against this row's GPIO-wait trigger, so the power-cycle assumption here stands unrefuted rather than confirmed. | Appended `timeout_ms: u32` to `GpioWaitRequest` (shared by all five `gpio/wait-*` endpoints) and `GpioError::Timeout` variant. Firmware `gpio_wait_for_*` handlers wrap `flex.wait_for_*_edge()` in `embassy_time::with_timeout` when `timeout_ms != 0`. Also enabled the embassy-rp watchdog at 2 s, fed by a dedicated `watchdog_feeder_task` (defense-in-depth against any future infinite-await). Lockstep schema bump (internal 0.6→0.7, lib 0.6→0.7, hal 0.6→0.7, ffi 0.7→0.8, application 0.7→0.8, pyco 0.3→0.4, firmware 0.10→0.11). |
| 2026-06-03 | `PicoDeGallo::validate()` only checked `schema_minor`, not `schema_major` | A firmware reporting a bumped major with matching minor would silently pass validation; the host would then mis-decode subsequent RPC responses (postcard happily decodes whatever bytes come back into the *host's* enum layout, so e.g. `NoAcknowledge` could be read as `Bus`). Failure mode is silent garbage out, no error to the caller. | Fixed `validate()` at `lib.rs:667` to check both major and minor, extended `ValidateError::SchemaMismatch` payload with `expected_major`/`actual_major`. Extracted the policy into a private `check_schema_compatible(&DeviceInfo)` helper with four regression tests. Also enforced validation up-front in `Hal::new_validated` (new), `gallo_init_strict` (new), `PycoDeGallo.open_strict` (new), and `gallo` CLI `Cli::run` (every subcommand except `list`/`version`). Closes Category A finding #1; host crates: lib 0.6.0→0.6.1, hal 0.6.0→0.7.0, ffi 0.7.0→0.7.1, application 0.7.0→0.7.1, pyco 0.3.0→0.3.1. |
| 2026-06-11 | `Cargo.toml` `[package].version` edited by hand for the §13.17 6-03 fixes (internal/lib/hal 0.6→0.7, ffi/application 0.7→0.8, pyco 0.3→0.4, firmware 0.10→0.11), **without** going through a release-please PR. | Manifest (`.github/.release-please-manifest.json`) and crates.io were left at the previous release (internal/lib/hal 0.5.0, ffi 0.6.0, application 0.6.0, pyco 0.2.0, firmware 0.9.0). release-please then saw `Cargo.toml == manifest + N` for every crate and proposed **seven separate** release PRs (#60–#66) that each *downgrade* the in-repo `Cargo.toml` back to the next legitimate target (`crates.io + 0.1 minor`: internal/lib/hal 0.6.0, ffi/application 0.7.0, pyco 0.3.0, firmware 0.10.0). The schema-0.7 fixes built locally, ran in CI, and were **never published** to crates.io / PyPI / GitHub Releases. No `firmware-v0.10*` or `firmware-v0.11*` tag exists; only dev builds ever reported schema 0.7. Drift was invisible because `cargo install --git` and local builds happily used the in-repo versions. | Plan Z (the chosen path): do **not** hand-rewrite any `Cargo.toml`. Instead ship a small infra-only PR (this branch) that (a) adds the `linked-versions` plugin to `.github/release-please-config.json` grouping all seven released components, (b) hardens AGENTS.md §4 rule #12 + §6.5 + this row, (c) rewrites `.github/RELEASE-PLEASE.md` (wire-protocol section, new "Linked versions" + "Manual version bumps are forbidden" sections, corrected `bootstrap-sha` semantics, removed `cargo-workspace` plugin references). After it merges, close the seven stale per-component release PRs; release-please regenerates them as **one combined release PR** that downgrades every `Cargo.toml` to the target version, updates the manifest, and writes the CHANGELOG entries. Refresh both `Cargo.lock`s on that combined PR (`cargo update --workspace --locked` at the repo root and `cargo update --locked` in `crates/pico-de-gallo-firmware/`) and merge. The seven tags (`internal-v0.6.0`, `library-v0.6.0`, `hal-v0.6.0`, `ffi-v0.7.0`, `application-v0.7.0`, `pyco-v0.3.0`, `firmware-v0.10.0`) and GitHub Releases are created by release-please automatically; `release-crates.yml` / `release-pyco.yml` / `release-firmware.yml` fire from those tags. Going forward: never edit `[package].version` by hand — see §4 rule #12. The repo now uses release-please with both `cargo-workspace` and `linked-versions` plugins so dep specs and version numbers stay in lockstep automatically across the host workspace; only the firmware's `pico-de-gallo-internal` dep spec needs a manual touch-up per release (see §6.5 and `.github/RELEASE-PLEASE.md`). |
| 2026-06-11 | Plan-Z infra-only PR scoped to add `linked-versions` plugin alone. | release-please's `linked-versions` plugin only coordinates version *numbers*; it does not rewrite cross-crate `version = "..."` dep specs. After internal 0.5.0 → 0.6.0, every dependent's `pico-de-gallo-internal = { version = "0.5.0", path = "..." }` spec would be stale: local `cargo build` fails between release-please merges, and `lib 0.6.0` published to crates.io would resolve `internal ^0.5.0` (silent wire mismatch). | Hoisted host workspace manifest from `crates/Cargo.toml` to repo-root `Cargo.toml` (chore(repo)! commit) so release-please's `cargo-workspace` plugin can find it. Added `cargo-workspace` plugin with `"merge": false` alongside `linked-versions`. Combined plugins now auto-bump every host-side dep spec; firmware is excluded from the host workspace, so its `pico-de-gallo-internal` dep spec is manually edited and its `Cargo.lock` is refreshed per release PR (documented in AGENTS.md §6.5 and RELEASE-PLEASE.md "Firmware dep-spec edit"). |
| 2026-07-20 | `gallo` CLI opened a throwaway USB connection for the up-front `validate()` added 2026-06-04 (Category A #4), then opened a *second* connection for the actual subcommand (`spi write-read` opened a third). | On Windows every validated subcommand (`i2c scan`, `i2c get-config`, `adc info`, …) panicked at `postcard-rpc raw_nusb.rs:330` with `Failed claiming interface: … Access is denied` (`ACCESS_DENIED`). WinUSB grants exclusive access to one session per interface, and the first connection's background `nusb` worker had not released the handle before the second `claim_interface`. `version`/`list` (single/zero connections) always worked, so it looked like a driver/permissions problem; Linux/macOS release the interface synchronously on drop, so CI (Linux-only) never caught it. | Refactored `Cli::run` (approach A) to open exactly one `PicoDeGallo` per invocation, validate on it, and thread `&pg` into every device handler. `list` returns before connecting; `version` shares the one connection but skips validation. Verified on hardware (Windows, usbipd/VBoxUSBMon present): `i2c scan`/`i2c get-config`/`adc info` now succeed. Also corrected the book USB PID (`ffff`/`B33C` → `067d`) in `getting-started/usb.md` + `appendix/troubleshooting.md`. Host-only (`gallo`/application); no wire-protocol or CLI-surface change. |
| 2026-07-23 | release-please retired (issue #83) after the recurring pain in the 2026-05-29 / 2026-06-11 rows (version↔manifest drift silently disabling releases, seven-PR fan-out, plugin collisions). | Not a regression — a deliberate policy change. Removed `.github/workflows/release-please.yml`, `.github/release-please-config.json`, `.github/.release-please-manifest.json`, and `.github/RELEASE-PLEASE.md`. | Releases are now **manual** (§12, `.github/RELEASE.md`): a maintainer hand-bumps every released crate's `[package].version` **and** the cross-crate `version = "..."` dep specs, hand-writes `CHANGELOG.md`, regenerates both `Cargo.lock`s, commits `chore(release): …`, merges, then pushes per-component tags (`internal-v*`, `library-v*`, `hal-v*`, `ffi-v*`, `application-v*`, `pyco-v*`, `firmware-v*`) that fire the unchanged `release-*.yml` publish workflows. §4 rule #12 flipped from "never hand-edit versions" to "hand-edit them, but only as a complete release commit". The old release-please rows above are kept as history; they describe tooling that no longer exists. |
| 2026-07-29 | Two boards attached; `gallo-mcp` running unpinned, so `connect()` fell back to `PicoDeGallo::try_new()` ("first match"). | An agent asked to find which board carried a temperature sensor saw `list_devices` report **both** serials, `i2c_scan` report an **empty** bus, and `device_info`/`status` give no indication of which board answered. The only conclusion the evidence supported — "neither board has a sensor" — was wrong: the server was bound to the empty board and the sensor board was unreachable. Two independently configured server instances returned byte-identical `device_info`, so nothing revealed they had both grabbed the same board. Caught only because an unrelated `gallo` CLI result contradicted the MCP scan. Worse than one bad read: `i2c_set_config`→`i2c_write`, `gpio_set_config`→`gpio_put`, and `onewire_search`→`onewire_search(continue)` are stateful across calls, so an ambiguous target can drive the wrong pins on the wrong board. | Added a pure `select::resolve_target` deciding the target from the attached serials, the `--serial-number` pin, and a new per-call `serial_number` argument on every device tool. Fallback is now conditional: kept at N==1 (frictionless single-board path), an **error** at N>=2 that names the available serials so the agent self-corrects. Every device response is wrapped as `{serial_number, result}`, making the binding observable on every call (`list_devices` and `status` excepted — one opens no board, the other already carries its own serial). `--serial-number` became a hard pin that refuses any other board. `status` never errors and reports ambiguity explicitly instead of `attached:false`. The connection lock was re-keyed from the server to the board, so a `gpio_wait_*` on one board no longer stalls calls to another. Verified on two boards, including a mutation control that reintroducing "first match" fails 3 of the 7 hardware tests. Host-only, `gallo-mcp` only; no wire-protocol or firmware change. Issue #89. |
| 2026-08-17 | `spi/batch` accepted a user GPIO explicitly configured as an input and unconditionally ran `set_as_output(); set_high()` on it. | The pad became a driven output-high while tracked `pin_modes` still said input. `gpio/get` returned the firmware's own drive, `gpio/put` returned `WrongDirection`, and nothing reported the divergence. Hardware reproduction showed a witness pin never named as CS going LOW→HIGH, which only an external drive explains. Two test traps also surfaced: RP2350 pull-downs can hold a low node but cannot reliably pull down an already-high node, while a floating pad drifts high within seconds, so tests must pre-drive low, release to pull-down, and verify the baseline; and cbindgen emits `typedef int32_t Status`, so C consumers need `switch ((enum Status)x)` with no `default:` inside the switch plus `-Werror=switch`, otherwise new enum values silently fall through. | Added three ordered firmware refusals before touching the pin: invalid index → monitored slot → `ExplicitInput`. Only `ExplicitInput` corrupts, so `LegacyAuto` and `ExplicitOutput` remain accepted. Deliberately do not restore the prior level because deasserted-high is the correct terminal chip-select state, and do not write `pin_modes` because both accepted modes are already self-consistent. Host surfaces validate `cs_pin < num_gpios` from validated `device/info`. Zephyr child `reg` is now a selector into `cs-gpio-indices`. A post-switch unknown-value fallback to `-EIO` remains intentional. Issue #104. |
| 2026-08-19 | Zephyr's temporary `cs-gpio-indices` mapping was replaced with standard `cs-gpios` through a real `odp,pico-de-gallo-gpio` controller; GPIO, I2C, and SPI became children of one MFD parent, and M5 then exercised the SPI framing boundary (`zephyr/drivers/spi/pdg_spi.c`, `PDG_SPI_MAX_BUFFER` comment; plan §12). | The old flat topology duplicated board selection and made chip select non-standard. M5 exposed two transferable traps. First, a boundary measured in one direction does not bound a duplex operation: 4096-byte TX-only failed `-ECOMM`, and a guessed 3072-byte full-duplex candidate also failed because request and response framing share the packet budget; TX-only 1013 succeeded, TX-only 1015 wedged the serial dispatcher, 1014 was not tested, and full duplex was verified only at 512 bytes. Second, a local consumer limit can contain a crash-class defect without fixing its wire/firmware root cause: CLI, Rust, C, Python, and MCP callers remain able to reach the 1015-byte device-wide wedge. The independently fed watchdog proves executor liveness, not dispatcher progress. In the reproduced SPI tests the device resumed after USB re-enumeration (`usbipd detach` followed by attach on Windows/WSL); this is an observed procedure, not proof that detach cancels the handler, and it has not been generalized to other dispatcher-wedge triggers. `system/reset-subscriptions` cannot help while the dispatcher is blocked. | **FIXED:** added the MFD parent and GPIO child, moved `serial-number` to the parent, deleted `cs-gpio-indices`, required same-parent `cs-gpios`, moved the Zephyr path from atomic `spi/batch` to four checked GPIO/SPI RPCs, and supported `SPI_HOLD_ON_CS \| SPI_LOCK_ON`. **CONTAINED, NOT FIXED:** `PDG_SPI_MAX_BUFFER = 1013` rejects longer Zephyr transfers before allocation, locking, chip-select edges, or transport; it is not a duplex-capacity guarantee. Applications needing a documented-safe Zephyr duplex size must use 512 bytes or less. The framing ceiling, non-returning RPC, watchdog progress gap, and host-surface reachability remain root defects outside the Zephyr module. On Linux/macOS, force equivalent re-enumeration by cable reconnect or USB unbind/rebind; power-cycle if re-enumeration is unavailable or ineffective. |
| 2026-08-26 | Any zero-length I2C write: `i2c/write` with empty `contents`, or `i2c/batch` carrying `Write { data: &[] }`. Reachable from `gallo_i2c_write`, `gallo_i2c_batch` (whose C API explicitly documents `data == NULL` when `data_len == 0`), `pico-de-gallo-lib`, the embedded-hal `I2c::write` impl, `pyco-de-gallo`, and `gallo-mcp` (`parse_bytes("")` returns an empty vec, asserted by a test), plus Zephyr's canonical bus scan. Only the `gallo` CLI was safe, and only by accident: clap's `num_args(1..)` requires at least one byte. | Device-wide dispatcher wedge recoverable only by USB re-enumeration, the third of this class after the 2026-06-03 GPIO-wait and 2026-08-19 SPI-framing rows. Three layers. RP2040/RP2350 `DW_apb_i2c` cannot emit an address-only transaction at all: the address phase is driven solely by pushing bytes into `IC_DATA_CMD`, so `START + ADDR + STOP` is physically unreachable (rp-rs/rp-hal#678, documented by merged rp-rs/rp-hal#679; embassy-rs/embassy#4474). embassy-rp 0.10.0 guards an empty payload in `read_blocking_internal`, `write_blocking_internal` and `read_async_internal`, but **not** in `write_async_internal`: that path queues no command, starts no transaction, then still awaits a `STOP_DET`/`TX_ABRT` interrupt that only a started transaction can raise, so the future never completes. postcard-rpc dispatches handlers serially, so the parked handler blocks every endpoint rather than just I2C. The watchdog does not fire, because `watchdog_feeder_task` is an independent task and keeps feeding while the dispatcher is stuck. `i2c/write-read` with empty `contents` is safe by accident: `write_read_async` passes `send_stop = false`, so `wait_stop_det` returns early instead of parking. | Firmware refuses an empty payload in `i2c_write_handler`, and in `i2c_batch_handler` during **validation** rather than in the execution loop, so a batch containing one is rejected atomically instead of driving its earlier operations onto the bus first. Appended `I2cError::ZeroLengthWrite` at index 7 rather than reusing `BufferTooLong`, whose Display reads "buffer exceeds firmware limit" and is actively misleading for a zero-byte buffer on a defect that otherwise presents as a silent hang. FFI maps it onto the existing `InvalidArgument` rather than a new code, because Status values are stable C ABI and a new one falls through the exhaustive `switch ((enum Status)x)` C consumers are told to write (§8, and the 2026-08-17 row). Rode the already-pending unreleased schema-0.7 bump, so no version moved (§4 rule 12). **Hardware-verified 2026-08-26 (#135).** A/B on `b324e9e` against the same tree minus both guards: without them the empty write never returned (88.9 s) and `version`, `ping`, `device/info`, `gpio/get`, `adc/read` and `gpio/put` all timed out, so the wedge is device-wide, not I2C-only; the watchdog never fired in a 40 s hold at its 2 s timeout; and a fresh host process still reached no endpoint, so it outlives host disconnect and is not a WinUSB claim artifact. With them, `i2c/write` returns `ZeroLengthWrite` in 1 ms, `i2c/batch` refuses at the offending index with the leading write never reaching the bus (TMP102 pointer witness unchanged, re-checked against the #139 `transaction()` handler), and empty `i2c/write-read` still returns data. `validate()` cannot tell the two builds apart, since both report fw 0.11.0 and schema 0.7; that misidentified a flash during this very verification, so track the flashed image yourself. The guard still has no automated coverage because embassy-rp's I2C needs real registers. Host-surface guards and the Zephyr `buf == NULL` relax remain outstanding. Issue #101, PR #133. |
| 2026-08-26 | `i2c/batch` executed every operation through a separate `read_async` or `write_async` call, giving each its own START and STOP instead of implementing one embedded-hal transaction. The behaviour was documented as intentional in `pico-de-gallo-internal` and the C FFI while the book asserted the opposite, so only C consumers were ever told the truth. `pico-de-gallo-hal` implements only `I2c::transaction`, and embedded-hal's default `read`, `write`, and `write_read` methods all routed through the broken endpoint; the common `write_read` path was therefore silently affected too. | Hardware reproduction with a TMP102 at `0x48`: seed TLOW to `0x1230`, then run `i2c_batch(0x48, [Write[0x02], Write[0x03,0x00]])`. The call returned `Ok`, left TLOW at `0x1230`, and changed THIGH from `0x5000` to `0x0000` even though the caller never named THIGH. After the fix, TLOW reads `0x0300` and THIGH remains `0x5000`. | Firmware now materialises the decoded operations and calls `embedded_hal_async::i2c::I2c::transaction()` once: adjacent same-type operations concatenate without an intervening STOP, a direction change emits a repeated START, and only the final operation is followed by a STOP. Bus errors report `failed_op = 0` because the atomic transaction fails as a unit; validation errors retain their exact index. Write-to-Read repeated START was originally only *inferred* from the RP2350 vendor SVD (`IC_CON` reset `0x00000065`, `IC_RESTART_EN` bit 5; embassy-rp never writes it), and the TMP102 used for this reproduction tolerates either framing, so the reproduction could not distinguish repeated START from STOP then START. That gap is now closed: the framing was **measured on a logic analyser on 2026-09-03** and matches the documented behaviour on all three counts. See the 2026-09-03 row for the capture and its residual limits. This wire-behaviour change is inside unreleased schema 0.7 and is invisible to `validate()`: schema-0.7 firmware built before this commit reports the same version but frames the bus differently, so host and firmware must be built from the same tree. Issue #128. |
| 2026-08-27 | Zephyr's `i2c_burst_write()` / `i2c_burst_write_dt()`, and every hand-rolled gather write, against `zephyr/drivers/i2c/pdg_i2c.c`. | `-ENOTSUP`. `validate_group_()` accepted a STOP-delimited group only when it held one message, or a write followed by a repeated-start read. `i2c_burst_write()` emits two *writes* (`I2C_MSG_WRITE`, then `I2C_MSG_WRITE \| I2C_MSG_STOP`), matching neither. Loud and atomic -- validation is a complete pre-pass before the mutex and before any FFI call -- so it is a compatibility bug, not a data-integrity one. The existing samples dodged it because `ti,tmp11x` uses a single `i2c_write_dt()`. | Generalised the grouping: N writes concatenate into one `gallo_i2c_write()`, a single read stays a read, and N writes plus one terminating read become `gallo_i2c_write_read()`; only a non-final read or a second read is still refused. **Deliberately NOT routed through `i2c/batch`**, even though the row above made that endpoint atomic: the atomicity lives in unreleased schema 0.7 and `validate()` cannot distinguish a schema-0.7 firmware built before it from one built after, so depending on it would turn a loud `-ENOTSUP` into silent register corruption. Two traps generalise beyond I2C. **Introducing concatenation invalidates every per-message bound** -- the size check had to become an overflow-safe per-group running total, or two individually legal 4096-byte writes merge into 8192. And **`k_malloc(0)` does not return NULL** in Zephyr: `z_alloc_helper()` adds a heap reference before allocating, so an allocation-failure guard is not how you special-case an empty payload. `PDG_I2C_MAX_BUFFER` was left at 4096 rather than lowered by analogy with `PDG_SPI_MAX_BUFFER = 1013`; concatenation does not widen the reachable payload range, and the real I2C write ceiling has never been measured (#146). Regression coverage is `zephyr/tests/pdg_i2c_burst`, board-attached and therefore **not run by CI**, which builds and links it only. Issue #102. |
| 2026-09-01 | Two firmware builds reporting the same schema version behaved differently on the wire — `i2c/batch` framing and the zero-length-write guard (both 2026-08-26), the latter of which **misidentified a flash during its own hardware verification**. | `validate()` compares schema versions only, and `SCHEMA_VERSION_*` is derived from `pico-de-gallo-internal`'s package version, so it tracks type changes rather than handler behaviour. Nothing distinguished the two images, and "track the flashed image yourself" was not a mitigation. Two secondary defects surfaced while adding identity: interpolating a git tag directly into a Rust string literal was unsafe because git refnames forbid backslash but permit `"` (empirically producing a syntax error), and the new MCP `info!` connect event was unreachable under the default `error,gallo_mcp=warn` filter. | Appended informational `DeviceInfo::build_id` (`heapless::String<64>`), generated from `git describe --always --dirty --tags --match firmware-v*` and surfaced by `gallo version`, MCP `status` / `device_info` / connect logs, the FFI `GalloDeviceInfo`, and Python. Regression tests pin that it is never a compatibility gate. `build.rs` emits the value with `{:?}` so Rust syntax is escaped, truncates on a character boundary, falls back to `"unknown"`, and reruns unconditionally. Its comments record that `--tags` is required because firmware tags mix annotated and lightweight forms (without it describe resolved 302 commits too far back), `--match` prevents an `application-v*` result, and the old `rerun-if-changed=memory.x` would otherwise leave stale identity. The MCP default filter now enables `gallo_mcp=info`: every new log event must be checked against the default filter or the feature silently does nothing. Also deleted two stale `SCHEMA FREEZE` markers whose bump had already shipped as `internal-v0.7.0`. **A schema-version bump does NOT make a re-keyed `device/info` detectable, and assuming it does was the wrong call made twice on this branch.** postcard-rpc keys each endpoint by `Key::for_path::<T>(path)`, a hash of the response type's `Schema` plus the path; the crate version is not an input. Verified by mutation: appending a variant to `I2cError` re-keyed `i2c/read` (`02c2…` to `24e9…`) while `device/info` stayed at `638a52f9b6daea52`, and compiling `internal` at 0.8.0 left `device/info` byte-identical to 0.7.0. So for any type other than `DeviceInfo` a skew is self-describing -- `device/info` still answers and `check_schema_compatible` returns `SchemaMismatch`. For `DeviceInfo` it is not, because the endpoint that re-keys *is* the compatibility probe: the reply is dropped unmatched, `check_schema_compatible` is unreachable behind `Ok(Ok(info))` in `fetch_validated_info`, and `validate()` can only return `Timeout` indistinguishable from a dead board. The schema numbers naming the incompatibility travel inside the message that is dropped. **`DeviceInfo` is a blind spot for its own versioning mechanism**, so changing its shape is qualitatively worse than changing any other wire type and warrants an explicit release note; `.github/RELEASE.md` step 5 now says so. Issue #159. |
| 2026-09-02 | Zero-length `i2c/write` against `wedge-test` firmware, reached by temporarily stubbing `check_i2c_write_payload` (`pico-de-gallo-lib/src/lib.rs`) and relaxing the CLI's `num_args(1..)` to `num_args(0..)`. **Three host surfaces had to be bypassed, not one**: the plan assumed the lib and MCP routes were open, but #135 closed every host path, so the firmware hatch alone is unreachable. Images were told apart by #159's `build_id`: clean `firmware-v0.11.0-59-g3fa18d5a4c98` versus `-dirty` for the mutated builds — the flash-misidentification hazard from the #135 verification is now closed by that field. | **A/B on one board, TMP117 at `0x48` and SPI in loopback.** Mutation control (supervisor detects expiry but feeds instead of resetting): the write hung **90.18 s** with no reset and the 2-second watchdog never fired; the board **still enumerated on USB** while `version` — an endpoint unrelated to I2C — hung **>30 s** from a fresh process, so the wedge is device-wide and the executor was demonstrably alive the whole time. This is #157's thesis reproduced directly: executor liveness is not dispatcher progress. Recovery required physical BOOTSEL. With the supervisor enabled, the same trigger reset the device in **10.48 s, 10.46 s and 10.57 s** across three runs (spread 0.11 s) against a designed `DEFAULT_DISPATCH_BUDGET + SUPERVISOR_POLL` bound of 10.25 s, the ~0.25 s excess being USB teardown and client detection. Recovery was automatic every time with no replug and no power cycle. Because the only difference between the two arms is whether `Action::Expired` calls `trigger_reset()`, the recovery can only come from the supervisor. | Replaced the unconditional `watchdog_feeder_task` with `watchdog_supervisor_task`. `WatchedRx` arms a dispatch slot after a frame arrives; `WatchedTx` tracks aggregate TX progress; slow handlers declare budgets; caller timeouts clamp at 30 minutes; and expiry records a scratch-register breadcrumb before `trigger_reset()`. **No false positives observed**: 20 mixed I2C/SPI ops, a 40-second idle at 4× the default budget (which specifically proves `WatchedRx` disarms inside `receive()` — a broken disarm resets there), 512-byte duplex, 1013-byte TX-only, and oversized duplex at 1015/2048/3072 bytes, which fails cleanly with exit 1 rather than wedging. Reset detection without an RTT probe used a PWM duty sentinel (4660 versus a 0 default), itself validated against a known reboot before being trusted. **Five limits remain.** A wedge inside `receive()` is indistinguishable from legitimate idle. The TX slot has no hardware trigger and rests on inspection; its shared slot measures aggregate, not per-sender, progress, so one permanently starved sender is masked while another completes at least once per 60-second `TX_BUDGET`. **The scratch-register breadcrumb path is unverified** — no probe was attached, so `supervisor: ... slot expired`, the boot-time `previous boot ended in a supervisor-forced reset` line, and `reset_reason() == Forced` with `WEDGE_MAGIC` were never observed; had that path been silently broken these tests would look identical. **The 30-minute clamp and the debugger-discontinuity rule are both untested**, the former because the `gallo` CLI exposes no `gpio wait-*` subcommand and the latter because it needs a debugger. Separately, **`spi/write` at 1015 bytes did not wedge** (0.36 s, success): the 2026-08-19 boundary was measured through the Zephyr driver's framing and does not transfer to the raw endpoint, so reading that row as a universal `spi/write` ceiling is a mistake. Issue #157. |
| 2026-09-03 | Verification of the 2026-08-26 `i2c/batch` framing claims on a logic analyser (#160). **Not a regression — all three claims held.** The row above had asserted repeated-START framing on the strength of an SVD reset value, while the TMP102 used to "confirm" it tolerates either framing, so nothing had actually measured the bus. | Two method traps, both of which would have produced a confident wrong answer. **A vendor decoder can render a repeated START as a plain `start`**: the Saleae tabular export labels every START identically, so the finding cannot rest on reading `Sr` in a decode — it rests on the *absence* of a `stop` row between the two phases. **A capture with no negative control proves nothing**, because "decoder never emitted STOP here" and "decoder cannot see STOP" look the same; the run therefore included two deliberately separate `i2c_write` + `i2c_read` calls as a known-genuine STOP-then-START. Separately, the board under test predates #159 and reports no `build_id`, so the flashed image could not be shown to contain the #128 fix from its version alone. | **Measured on one board, TMP102 at `0x48`, Standard 100 kHz, SDA GPIO 2 / SCL GPIO 3.** Confirmed from raw SDA/SCL transitions (START = SDA 1->0 while SCL high; STOP = SDA 0->1 while SCL high), independent of any byte framing: (a) `[Write 0x00, Read 2]` emitted `S ... Sr ... P` with **exactly one STOP, at the end**, the second START falling **10.93 us** after the preceding ACK — about 1.09 bit periods, far too soon for a STOP plus the 4.7 us minimum t(BUF); (b) `[Write 0x02, Write 0x28,0x00]` emitted a single address phase `S 90 02 28 00 P`, so adjacent writes do concatenate; (c) the four-op `[W, R, W, R]` batch emitted **four STARTs and one STOP**, the STOP last, which also covers the Read-to-Write direction change. The stale-flash doubt was closed by behaviour rather than by version: pre-#128, (b) would have run as two transactions with the second setting the pointer to `0x28` (masked to the read-only temperature register), leaving TLOW at `0x4B00`; TLOW read back `0x2800` and THIGH was untouched at `0x5000`. **Two limits remain.** This measures Standard mode only — Fast and Fast-plus were not captured, though no mechanism suggests they differ. And the bus held a single target with no clock stretching; stretching changes timing but not the presence of a STOP. Issue #160. |

---

## 14. Testing conventions

- Tests live as `#[cfg(test)] mod tests` inline in each crate's
  `src/lib.rs`.
- **Naming:** `type_name_behavior()` (e.g.,
  `i2c_read_request_round_trip`).
- Round-trip serialization tests for **every** wire type using
  `postcard::{from_bytes, to_allocvec}` (requires the `use-std`
  feature, or run from the workspace).
- FFI tests check null pointers, status-code invariants, and
  argument validation.
- CLI tests verify clap argument parsing.
- `pyco-de-gallo` has Rust-side unit tests for its conversion and
  chip-select validation surface; broader behavior is also covered
  transitively by `pico-de-gallo-lib` tests and exercised from Python.
  Adding tests is welcome.

## 15. Documentation requirements

- All public items must have **rustdoc**.
- Every crate must have crate-level `//!` docs.
- For `pyco-de-gallo`, doc comments double as Python `__doc__`
  strings — write them in Google style (`Args:`/`Returns:`/`Raises:`)
  so Sphinx napoleon and Pyright render them well.
- Update `book/` when adding new endpoints or changing CLI behavior.
- Update the affected crate's `crates/<crate>/CHANGELOG.md` (Keep a
  Changelog format) for endpoint additions, CLI changes, wire-protocol
  changes, and any change that alters a release artifact name or path.
  There is no root `CHANGELOG.md` — it was deleted in `f4e6b52` in
  favour of per-crate files. Zephyr-module changes go in
  `zephyr/CHANGELOG.md`.
- `README.md` at the repo root reflects the high-level overview;
  keep it in sync.

### 15.1 Book ↔ code parity (hard rule)

The `book/` directory is reference documentation, not marketing
copy. It **must always describe the code that is on `main`**. Any
drift is a bug.

Concretely:

- **Code change?** Update the book in the *same* PR. If you add,
  rename, or remove a CLI flag, endpoint, status code, struct
  field, FFI function, Python binding, configuration enum, or
  hardware-revision capability, the corresponding `book/src/...`
  chapter must change in lockstep. A PR that ships code without
  the matching book edits is incomplete.
- **Book change?** Re-verify the code still does what the book
  now claims. Re-run the CLI snippets, re-derive the endpoint
  list from `pico-de-gallo-internal/src/lib.rs`, re-derive the
  status-code table from `pico-de-gallo-ffi/src/lib.rs`. If the
  book is being fixed because it had drifted, also open an issue
  (or fix in the same PR) for whichever side regressed.
- **No "I'll do the docs next."** Documentation debt rots faster
  than code debt because nobody runs it. The PR template
  enforces this with an explicit checkbox; reviewers should
  block PRs that tick "no docs needed" without justification.

**Per-area mapping** — when you change a file on the left, also
update at least the chapter(s), and check the consumers, on the
right:

| Code area                                                   | Book chapter(s) / consumers                                              |
|-------------------------------------------------------------|--------------------------------------------------------------------------|
| `pico-de-gallo-internal/src/lib.rs` — endpoints / topics    | `book/src/appendix/endpoints.md`, `book/src/internals/wire-protocol.md`; `zephyr/` reaches endpoints only through the FFI — see the `gallo_*` row |
| `pico-de-gallo-internal/src/lib.rs` — wire enums (variants) | `book/src/internals/wire-protocol.md`, relevant `book/src/interfaces/*`; `zephyr/tests/pdg_mfd_m5/common/m5_bottom.c` `_Static_assert`s the mirrored FFI discriminants (§8) |
| `pico-de-gallo-ffi/src/lib.rs` — `Status` enum              | `book/src/appendix/status-codes.md`; `zephyr/drivers/common/common.c` maps every `Status` to an `errno` |
| `pico-de-gallo-ffi/src/lib.rs` — `gallo_*` functions        | `book/src/crates/ffi.md`; `zephyr/drivers/**` links the FFI and calls `gallo_*` |
| `pico-de-gallo-app/src/...` — CLI subcommands/flags         | `book/src/crates/app.md`, the relevant `book/src/interfaces/*` chapter   |
| `pico-de-gallo-mcp/src/...` — MCP tools / tool arguments    | `book/src/crates/mcp.md`, `crates/pico-de-gallo-mcp/README.md`           |
| `pico-de-gallo-lib/src/lib.rs` — public methods             | `book/src/crates/lib.md`                                                 |
| `pico-de-gallo-hal/src/...` — trait impls                   | `book/src/crates/hal.md`, `book/src/driver/*`, `docs/ai-agents/pico-de-gallo-hal-examples.md` |
| `pyco-de-gallo/src/...` — Python surface                    | `book/src/crates/python.md`                                              |
| `pico-de-gallo-firmware/src/...` — peripheral behaviour     | `book/src/internals/firmware.md`, `book/src/interfaces/*`                |
| `crates/pico-de-gallo-internal/build.rs` — schema version   | `book/src/internals/releases.md`, `book/src/internals/wire-protocol.md`  |
| `zephyr/` — drivers, DT bindings, sample overlays           | `zephyr/README.md`, `zephyr/CHANGELOG.md`, the relevant `book/src/interfaces/*` chapter |
| `hardware/` — KiCad changes (new revision, pin remap)       | `book/src/hardware/{overview,revisions,pinout}.md`                       |
| `crates/<crate>/src/...` — any released behaviour           | That crate's `crates/<crate>/CHANGELOG.md`; hand-written (Keep a Changelog), not auto-generated. There is no root `CHANGELOG.md`. |

**Reverse direction — `zephyr/` is an FFI consumer.** The four rows
above point *out* of the FFI and wire protocol into `zephyr/` because
the Zephyr drivers call `gallo_*` through the generated
`pico_de_gallo.h`. An FFI or wire-enum change can therefore break them
while every host gate stays green: `check.yml` builds the FFI crate,
which still compiles, and cbindgen regenerates the header without
complaint. Two specific obligations:

- **`Status` → `errno`.** `pdg_common_status_to_errno()` in
  `zephyr/drivers/common/common.c` switches over every `Status`
  variant with **no `default:` inside the switch**, so `-Werror=switch`
  (`zephyr/drivers/CMakeLists.txt`) turns an omitted case into a build
  failure rather than a silent `-EIO`. The `(enum Status)` cast there
  is load-bearing — under C11/C17 cbindgen emits `typedef int32_t
  Status` (only C23 gets `typedef enum Status Status`), so an uncast
  switch gets no `-Wswitch` coverage at all (§8, and the 2026-08-17
  row in §13.17).
- **Wire-enum variant order.** The FFI's `GalloGpioEdge`,
  `GalloGpioDirection`, and `GalloGpioPull` values mirror the
  `pico-de-gallo-internal` wire enums, whose variant order is itself
  ABI (§6.1). `zephyr/tests/pdg_mfd_m5/common/m5_bottom.c` pins that
  correspondence with `_Static_assert`s, so reordering a wire enum
  breaks the Zephyr test build.

**Both gates now run in CI, within limits.** `.github/workflows/zephyr.yml`
builds the module on every PR touching `zephyr/`, `crates/pico-de-gallo-ffi/`,
`crates/pico-de-gallo-internal/`, either root Cargo file, or its own
`.github/workflows/zephyr.yml` definition, so
`-Werror=switch` and those `_Static_assert`s fire automatically. The
`_Static_assert`s only compile in the M5 targets, which the gate builds
for exactly that reason.

Two limits still bind. The workflow is **path-filtered**, so a change
outside those paths does not run it. And it is **build-only for
everything that touches a board**: every sample and every M5 app sets
`build_only`, because booting them reaches `gallo_init_strict()` and
needs hardware. The one exception is `tests/pdg_fake/i2c`, which
deliberately omits `build_only` and is executed by twister against a
recording fake.

So a green run is evidence that `zephyr/` still *compiles and links*,
plus that one hardware-free suite still passes. It is not evidence
that the module still *works* against a real board. Behavioural claims
about hardware still require the manual, board-attached
`zephyr/tests/pdg_mfd_m5/run-m5.sh` procedure.

**Carve-out — `zephyr/` has no book chapter, deliberately.** The
Zephyr module is documented in `zephyr/README.md` (the authoritative
detailed guide) and `zephyr/CHANGELOG.md`, **not** in `book/`. The
book carries only deliberate cross-references into it: the Zephyr
sections in `book/src/interfaces/{spi,gpio}.md`, the `BufferTooLong`
entry in `book/src/appendix/troubleshooting.md`, and the 1013-byte
containment warnings in `book/src/crates/{app,ffi,lib,mcp,python}.md`.

This is a ruling, not an oversight. See
`docs/superpowers/specs/2026-08-24-zephyr-mfd-m6-docs.md` §6.3 and
§9 item 3: *"Add a dedicated Zephyr book chapter. Rejected as
duplication while the module remains WIP and `zephyr/README.md` is
intentionally authoritative."* Consequences:

- **Reviewers must not block a `zephyr/`-only PR for lacking a
  `book/src/**` change.** Updating `zephyr/README.md` and
  `zephyr/CHANGELOG.md` satisfies §15.1 for that PR.
- A `zephyr/` change that alters something the book *does* describe
  (an interface chapter's Zephyr section, a transfer limit, a
  status code) still needs the paired book edit.
- **Revisit trigger:** the chapter question reopens when the
  upstreaming work in #98 lands. M6 §12 defers it explicitly to
  "after upstreaming" — until then, do not re-litigate it per PR.

**Reviewer checklist (humans *and* the GitHub Copilot reviewer).**
For every PR, confirm:

1. Every code change has a paired book change (or an explicit
   one-line note in the PR body explaining why none was needed).
2. CLI examples in any modified `book/src/**` page still match
   the actual `gallo --help` output for that subcommand.
3. Tables of endpoints, status codes, wire enums, and capability
   bits in the book match the source-of-truth files listed above.
4. New endpoints in `pico-de-gallo-internal` show up in
   `book/src/appendix/endpoints.md` **and** are linked from the
   relevant interface chapter.
5. Wire-protocol changes (variant adds, request/response shape
   changes) include a schema-version bump (see §6) **and** a
   `book/src/internals/releases.md` mention.
6. `mdbook build book` is clean (no broken links, no missing
   referenced files) — CI builds the book on every PR via
   `.github/workflows/gh-pages.yml`'s build step.
7. FFI or wire-protocol changes name the `zephyr/` consumer they
   affect, or state in the PR body that none is affected. A green
   `zephyr.yml` run is acceptable evidence that the consumer still
   compiles and links. It is **not** evidence that behaviour is
   unchanged, and it does not run at all if the PR touches none of
   that workflow's filtered paths — check that it actually ran.

Reviewers, including the automated Copilot reviewer, should flag
PRs that violate any of the above as a **blocker**, not a nit.

## 16. Pre-release checklist

Every release is cut by hand now (§12, `.github/RELEASE.md`). Before
you push any `*-v*` tag:

1. From a clean checkout, run the full preflight. Build **both**
   firmware revisions — `release-firmware.yml` publishes an artifact for
   each, so a preflight that exercises only the default (`hw-rev2`)
   leaves the deprecated-but-still-published `hw-rev1` image untested:
   ```bash
   cargo fmt --check && \
     cargo clippy --all-targets -- -D warnings && \
     cargo test --locked
   cd crates/pico-de-gallo-firmware && cargo fmt --check && \
     cargo clippy --target thumbv8m.main-none-eabihf -- -D warnings && \
     cargo build --release --locked --target thumbv8m.main-none-eabihf && \
     cargo clippy --target thumbv8m.main-none-eabihf \
       --no-default-features --features hw-rev1 -- -D warnings && \
     cargo build --release --locked --target thumbv8m.main-none-eabihf \
       --no-default-features --features hw-rev1
   ```
2. Confirm the schema version is honest before tagging. If the branch
   changed any wire shape — a request/response field, an endpoint, or an
   appended enum variant — `pico-de-gallo-internal`'s `[package].version`
   must already carry the matching bump, because `SCHEMA_VERSION_*` is
   derived from it (§6.2) and `PicoDeGallo::validate()` cannot detect a
   shape change hidden behind an unchanged version. Land the lockstep
   version bump, dep-spec rewrites, and both lockfiles first; build every
   host and firmware artifact from that bumped commit.
3. Confirm `git tag --points-at HEAD` matches expectation **and**
   that the workflow YAML at HEAD is the version you want CI to run
   (see §13.13).
4. Push the commit first; wait for CI green; **then** push tags.

Verify the tagged-commit workflow with:

```bash
git --no-pager tag --points-at HEAD
git --no-pager show <tag>:.github/workflows/release-firmware.yml \
    | grep -E 'elf2uf2|picotool'
```

## 17. Where to look next

- **`.github/RELEASE.md`** — manual release playbook.
- **`CONTRIBUTING.md`** — human-facing contribution guide.
- **`book/`** — user-facing documentation
  ([online](https://balbi.sh/pico-de-gallo/)).
- **`crates/<crate>/src/lib.rs`** — every crate has top-level `//!`
  docs that summarize its public surface.
- **`deny.toml`** — dependency policy (advisory ignores, license
  allow-list, ban rules).
- **`.github/copilot-instructions.md`** — stub pointing back here.

---

## 18. When in doubt

- **Run CI commands locally before pushing.** Especially `cargo
  clippy --all-targets --locked -- -D warnings` and `cargo check
  --locked` per crate.
- **Ask the user before making destructive or wide-reaching
  changes** — force-pushes, dependency major bumps,
  wire-protocol breaks, file deletions outside the immediate task.
- **Don't fabricate.** If you don't know whether something is
  pinned, look at the `Cargo.toml`. If you don't know whether an
  endpoint exists, grep `pico-de-gallo-internal/src/`.
- **Cite your sources** in commit bodies and PR descriptions.
  Reference issue numbers, upstream commits, RUSTSEC IDs, datasheet
  page numbers.

Welcome aboard. 🌶️
