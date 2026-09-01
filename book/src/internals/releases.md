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

### Schema 0.8 changed `DeviceInfo`, so 0.7 pairs fail opaquely

Schema **0.8** appended `build_id` to `DeviceInfo`. That is an append in the
encoding, but `DeviceInfo` is not an ordinary wire type: postcard-rpc derives
each endpoint's key from its response type's schema, so changing it re-keyed
`device/info` itself.

The consequence is that a schema 0.8 host and schema 0.7 firmware — or the
reverse — cannot diagnose each other. The peer replies under the other key,
the dispatcher drops the unmatched frame, and `validate()` returns `Timeout`
rather than `SchemaMismatch`. The schema numbers that would have named the
problem are inside the message that was dropped, and no version bump can
surface them, because the version is payload rather than key.

So this particular boundary does **not** produce the normal, self-describing
failure documented below. It looks exactly like an unresponsive board. If
`gallo` hangs or times out against a board you believe is healthy, suspect a
0.7/0.8 skew before you suspect the hardware.

`gallo version` still works across the pair and is the tool to reach for: it
reads the `version` endpoint, whose `VersionInfo` schema is deliberately held
stable precisely so one diagnostic survives a `device/info` re-key.

The fix is the usual one — build both sides from the same release. Schema 0.7
and 0.8 firmware and host components must not be mixed.

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
