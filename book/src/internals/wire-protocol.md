# Wire Protocol & Schema Versioning

The wire protocol is the most important compatibility contract in Pico de
Gallo. All protocol types live in `pico-de-gallo-internal`, and both the host
and the firmware compile against that crate.

## postcard encoding rules

Pico de Gallo uses [postcard](https://docs.rs/postcard) for compact binary
encoding. That brings one rule that every contributor must understand.

> [!IMPORTANT]
> postcard encodes enum variants by **variant index**, not by the numeric value
> in `#[repr(...)]`. Reordering variants in a wire-visible enum is a silent ABI
> break.

So this is safe:

- append a new enum variant at the end.

And this is breaking:

- reorder variants,
- remove a variant,
- rename a variant that old peers still expect.

The warning comments in `pico-de-gallo-internal` are there for a reason; keep
them.

## The shared protocol crate

`pico-de-gallo-internal` defines:

- endpoint marker types,
- request and response structs,
- topic message types,
- protocol constants like `MAX_TRANSFER_SIZE`,
- schema-version constants generated at build time.

The crate also uses the `use-std` feature to switch certain response types
between owned host buffers and borrowed firmware buffers:

- host build: `Vec<u8>`
- firmware build: `&[u8]`

That lets the host own received data while the firmware can answer from a shared
scratch buffer without heap allocation.

## Endpoints and topics

Endpoints are normal request/response RPCs declared with `endpoints!`. Topics
are push-style messages declared with `topics!`.

In practice:

- endpoints cover commands like `i2c/read`, `spi/transfer`, and `device/info`,
- topics cover asynchronous server-to-client events.

Today the main topic is GPIO event streaming:

| Kind | Example | Direction | Purpose |
|------|---------|-----------|---------|
| Endpoint | `i2c/read` | host → device → host | Request/response RPC |
| Endpoint | `device/info` | host → device → host | Compatibility probe |
| Topic | `gpio/event` | device → host | Push edge notifications |

A short slice of the endpoint catalog looks like this:

| Path | What it does |
|------|--------------|
| `ping` | Echo test payload |
| `version` | Report firmware version |
| `device/info` | Report firmware/schema versions, hardware revision, capabilities, and the runtime GPIO count |
| `i2c/read` | Read from an I<sup>2</sup>C target |
| `spi/transfer` | Full-duplex SPI transfer |
| `gpio/subscribe` | Start GPIO event monitoring |

For the full list, see the [Endpoint Catalog](../appendix/endpoints.md).

## SPI chip-select contract

`SpiError` is serialized by variant index. The deployed indices are:

| Index | Variant | Meaning |
|-------|---------|---------|
| 0 | `BufferTooLong` | Request exceeds the firmware buffer limit |
| 1 | `Other` | Unspecified firmware-reported SPI failure |
| 2 | `InvalidCsPin` | Chip-select index outside `0..DeviceInfo::num_gpios` |
| 3 | `CsPinUnavailable` | Chip-select pin is explicitly configured as an input |
| 4 | `CsPinMonitored` | Chip-select pin is monitored for GPIO events |

For `spi/batch`, the chip-select pin behaves as follows:

- the pin is driven as an **output** for the duration of the batch,
- on success it is left configured as an output, deasserted high; the prior
  direction is **not** restored,
- on an execution failure *after* CS was asserted, it is likewise left
  deasserted high,
- on a pre-validation failure or a refusal, the pin is left **untouched**,
- pins explicitly configured as inputs are **refused** — firmware predating
  this contract may instead reconfigure them.

`DeviceInfo::num_gpios` is the runtime-authoritative pin count. Valid GPIO and
SPI chip-select indices are `0..num_gpios`; when it is zero, no index is
valid. It supersedes the compile-time `NUM_GPIOS` default and must never be
synthesized or defaulted when `device/info` decoding fails.

## Schema versioning

The schema version constants are not handwritten. `pico-de-gallo-internal`
generates `SCHEMA_VERSION_MAJOR`, `SCHEMA_VERSION_MINOR`, and
`SCHEMA_VERSION_PATCH` in `build.rs` from the crate's `[package].version`.

> [!CAUTION]
> Do not edit `SCHEMA_VERSION_*` constants directly. Bump the
> `pico-de-gallo-internal` crate version and let `build.rs` regenerate them.

Before 1.0, the **minor** version is the breaking axis. After 1.0, that role
moves to the major version.

That means a pre-1.0 bump is required when you:

- add or remove an endpoint or topic,
- change a request or response type,
- append a new wire enum variant.

> **Warning** — schema freeze on the `zephyr` branch.
> This branch intentionally changes the shape of `SpiError` and `DeviceInfo`
> **without** changing the reported schema version. Mixed peers are therefore
> incompatible even when both sides report the same schema: a new host cannot
> decode an old eight-field `DeviceInfo`, while an old decoder accepts only the
> eight-field prefix of a new response. Because postcard-rpc response keys
> include the response schema, a mismatched peer may wait indefinitely rather
> than fail fast. This branch is not releasable or taggable until the
> maintainer performs the lockstep version bump required by AGENTS.md §6.5.

## Host/firmware compatibility checks

The host library exposes `PicoDeGallo::validate()`. It calls `device/info`,
reads the firmware's schema version, and rejects mismatches early.

If validation fails, the host returns:

- `LegacyFirmware` when the firmware is too old to support `device/info`, or
- `SchemaMismatch` when the host and firmware disagree on the schema version.

For released components with honest schema versions, this turns an otherwise
confusing runtime failure into an explicit compatibility error. It cannot detect
the intentional schema freeze described above.

## Lockstep releases for protocol changes

A wire change is never just one crate. Per the project rules, the same release
cycle must update:

1. `pico-de-gallo-internal`,
2. `pico-de-gallo-firmware`,
3. `pico-de-gallo-lib`,
4. `pico-de-gallo-hal`,
5. `pico-de-gallo-ffi`,
6. `pico-de-gallo-app`,
7. `pico-de-gallo-mcp`,
8. `pyco-de-gallo`.

> [!IMPORTANT]
> Nothing automated knows that the protocol crate and firmware are
> wire-coupled. Lockstep is enforced by contributors, not by tooling.

For contributor policy and the full compatibility rules, see
[`AGENTS.md`](https://github.com/OpenDevicePartnership/pico-de-gallo/blob/main/AGENTS.md).
