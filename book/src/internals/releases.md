# Releases & Compatibility

Pico de Gallo releases are cut by hand, and compatibility depends on humans
understanding which pieces move together.

## Tag prefixes

Each published surface has its own release tag prefix:

| Component | Tag |
|-----------|-----|
| `pico-de-gallo-internal` | `internal-v*` |
| `pico-de-gallo-lib` | `library-v*` |
| `pico-de-gallo-hal` | `hal-v*` |
| `pico-de-gallo-ffi` | `ffi-v*` |
| `gallo` CLI | `application-v*` |
| `gallo-mcp` | `mcp-v*` |
| `pyco-de-gallo` | `pyco-v*` |
| `pico-de-gallo-firmware` | `firmware-v*` |
| hardware artifacts | `hardware-v*` |

## What drives a release?

Releases are **manual**. A maintainer bumps each crate's
`[package].version`, updates the cross-crate dependency specs and each
changed crate's `crates/<crate>/CHANGELOG.md`, merges to `main`, and then
pushes one tag per component. The tag-triggered `release-*.yml` workflows
publish to crates.io / PyPI and build the binary artifacts.

There is no root changelog. Every crate owns its own, and the Zephyr module
uses `zephyr/CHANGELOG.md`.

Contributors still land Conventional Commits with crate scopes such as
`feat(internal): ...` or `fix(firmware): ...` — that scoped history is what a
maintainer reads when hand-writing those changelogs and deciding which crates
to bump.

> [!TIP]
> The scope is not decoration. It tells the release author what changed in each
> crate and where the version bump belongs.

## Protocol changes are lockstep changes

When the wire protocol changes, compatibility is broader than one crate tag.
The protocol crate, firmware, and every host-facing crate must move in the same
release cycle.

That means coordinating:

- `pico-de-gallo-internal`,
- `pico-de-gallo-firmware`,
- `pico-de-gallo-lib`,
- `pico-de-gallo-hal`,
- `pico-de-gallo-ffi`,
- `pico-de-gallo-app`,
- `pico-de-gallo-mcp`,
- `pyco-de-gallo`.

> [!IMPORTANT]
> Nothing enforces wire coupling for you. If a protocol change lands without its
> matching host and firmware version bumps, users will feel it.

### Schema 0.7 is on `main` but not yet published

`main` carries schema **0.7**: `SpiError` gained `InvalidCsPin`,
`CsPinUnavailable`, and `CsPinMonitored`, `DeviceInfo` gained a final
`num_gpios` field, and `I2cError` gained `ZeroLengthWrite`. The lockstep
version bump for those changes has landed, so `pico-de-gallo-internal` reports
0.7 honestly.

The newest *published* release is still schema 0.6. That is a normal,
detectable mismatch rather than a hazard: a host built from `main` and a
released firmware disagree on the reported schema, so `validate()` returns
`SchemaMismatch` and the two refuse to talk. Build both sides from the same
commit, or keep both on a published release.

## How users check compatibility

There are two main compatibility checks:

- `gallo version` prints firmware version, schema version, hardware revision,
  and capabilities.
- `PicoDeGallo::validate()` checks compatibility programmatically and fails with
  `SchemaMismatch` or `LegacyFirmware` when the pair should not talk.

For most users, `gallo version` is the first stop. For library users,
`validate()` is the guardrail you call before doing real work.

## “I flashed new firmware and now my host is broken”

That usually means the firmware and host were built against different versions
of `pico-de-gallo-internal`.

Typical symptoms include:

- `validate()` returning `SchemaMismatch`,
- a new firmware exposing endpoints an older host does not know about,
- older firmware lacking `device/info`, which shows up as `LegacyFirmware`.

The fix is simple: upgrade the matching host component for the firmware you
flashed, or downgrade the firmware to the host release you are using.

> **Warning.** The protocol is typed, but a mismatched pair is not guaranteed to
> fail fast. Because postcard-rpc response keys include the response schema, a
> mismatched peer may wait indefinitely. See the
> [wire-protocol warning](./wire-protocol.md#hostfirmware-compatibility-checks).

## MSRV and release hygiene

The workspace tracks Rust 1.90 as its MSRV, and CI checks it explicitly. That
includes the host workspace and the firmware workspace.

For contributor-only release details, including the full manual-release
checklist, see
[`AGENTS.md`](https://github.com/OpenDevicePartnership/pico-de-gallo/blob/main/AGENTS.md)
and the repository's
[`RELEASE.md`](https://github.com/OpenDevicePartnership/pico-de-gallo/blob/main/.github/RELEASE.md).
