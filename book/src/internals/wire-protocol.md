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
- directional payload constants `MAX_TRANSFER_SIZE` and
  `MAX_RESPONSE_PAYLOAD`,
- schema-version constants generated at build time.

The crate also uses the `use-std` feature to switch certain response types
between owned host buffers and borrowed firmware buffers:

- host build: `Vec<u8>`
- firmware build: `&[u8]`

That lets the host own received data while the firmware can answer from a shared
scratch buffer without heap allocation.

The two payload constants are independent and selected by byte direction:

| Direction | Constant | Value |
|---|---|---:|
| host → device | `MAX_TRANSFER_SIZE` | 4096 |
| device → host | `MAX_RESPONSE_PAYLOAD` | 1014 |

`MAX_RESPONSE_PAYLOAD` is derived from postcard-rpc's 1024-byte host inbound
USB buffer: 1024 − 7 bytes of response header (1 discriminant, 2 key, 4
sequence) − 1 postcard `Result` discriminant − 2 bytes of varint length prefix
= 1014. It is a host-transport property that firmware cannot observe.
Full-duplex `spi/transfer` proves why both constants are needed: its one length
travels in both directions, so the tighter response ceiling applies.

A third constant, `MAX_REQUEST_FRAME` (5119), bounds the whole request *frame*
rather than any one argument. It only binds the batch endpoints; the section
below derives it.

Issues #158 and #186 added local validation and firmware bounds without
changing an endpoint, request or response type. Neither is therefore a wire
change, and neither required a schema change or version bump.

## The request frame ceiling

`MAX_TRANSFER_SIZE` bounds a single *argument*. Independently of it, the whole
request **frame** — header plus postcard body — has to fit in the firmware's
receive buffer, which is `MAX_TRANSFER_SIZE + 1024` = 5120 bytes (`BufStorage`
in `crates/pico-de-gallo-firmware/src/main.rs`).

A frame that does not fit is discarded **silently**. postcard-rpc's server maps
`WireRxErrorKind::ReceivedMessageTooLarge` to `continue`, so nothing is sent
back; the caller sees only its own timeout. That is the failure mode to expect
here, not `BufferTooLong`.

The largest frame the firmware accepts is one byte less than the buffer:

| Bytes | Term |
|------:|------|
| `5120` | firmware receive buffer, `MAX_TRANSFER_SIZE + 1024` |
| `-1` | a frame that *exactly* fills the buffer is refused |
| **`= 5119`** | largest accepted request frame |

That `-1` is a mechanism, not a safety margin. postcard-rpc's
`EUsbWireRx::receive` ends a frame when a **short** USB packet arrives, and
loops `while !window.is_empty()`. A frame whose length equals the buffer size
fills the window with full 64-byte packets, so the loop runs out of window
before it ever sees a short packet, falls into its "ran out of space" branch,
reads the transfer-terminating zero-length packet there, and reports
`ReceivedMessageTooLarge`. Frames that are a multiple of 64 but *smaller* than
the buffer are unaffected, because window remains and the zero-length packet
ends the frame in the normal path.

What is left for the payload depends on the header, which is **not** a
constant:

| Bytes | Term |
|------:|------|
| `5119` | largest accepted request frame |
| `-1` | header discriminant |
| `-8` or `-2` | header key — see below |
| `-4` | header sequence number (`VarSeq::Seq4`, always; `VarSeq::resize` is defined in postcard-rpc 0.12.1 and called nowhere in it, so the `VarSeqKind` this repo passes never takes effect) |
| `-N` | whatever the request struct encodes around the payload — for a bare `&[u8]` field that is a 2-byte postcard varint length prefix |

postcard-rpc narrows the key width over the life of a connection.
`HostClient` starts at `VarKeyKind::Key8` and adopts the server's narrower key
only after it has **received a reply**; this device's server answers with
`Key2`. So a process's first request carries a 13-byte header and every request
after the first reply carries 7, and the usable payload is six bytes smaller
until then. A request that is itself dropped does not narrow anything, because
no reply came back.

For an endpoint whose request is a single byte slice, that works out to:

| State | Header | Largest payload |
|---|---:|---:|
| before the first reply | 13 | 5104 |
| after the first reply | 7 | 5110 |

**No supported single-argument call can reach this.** Since #158 every host
surface caps a write argument at `MAX_TRANSFER_SIZE`, which puts the worst-case
frame a little over 4110 bytes — a kilobyte clear of the ceiling.
`pico-de-gallo-internal` asserts that ordering at compile time, so it cannot
quietly stop being true.

Batches were the exception, because nothing bounded their aggregate *write*
bytes: `i2c/batch` and `spi/batch` could build an over-ceiling frame and lose
it silently. Since #186 both are bounded host-side against `MAX_REQUEST_FRAME`
before transmission, using the **wide** header so the answer does not depend on
whether the client has received a reply yet. The firmware cannot help here — it
never sees the frame — which is what makes this the one ceiling with no
device-side counterpart. See
[Transaction Batching](../interfaces/batching.md#the-request-frame-ceiling).

Measured on board `49742081C885AC69` (hw-rev2, firmware
`firmware-v0.11.0-79-gc3c6a3e07bec`) under Linux/nusb, and previously on
`5256657D8A5D7F03` under Windows/WinUSB with identical edges — issue #180. The
5119/5120 edge was re-measured on `firmware-v0.11.0-88-gfcb38de3a1ff` for
issue #186, with the new host guard stubbed out, and was unchanged.

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
| `device/info` | Report firmware/schema versions, hardware revision, capabilities, runtime GPIO count, and build identity |
| `i2c/read` | Read from an I<sup>2</sup>C target |
| `spi/transfer` | Full-duplex SPI transfer |
| `gpio/subscribe` | Start GPIO event monitoring |

For the full list, see the [Endpoint Catalog](../appendix/endpoints.md).

## I²C batch framing contract

`i2c/batch` executes the complete operation list as one I<sup>2</sup>C
transaction:

- a START and address precede the first operation,
- adjacent operations of the same type run back to back without a STOP or
  repeated START; two adjacent writes therefore form one gather write,
- a direction change emits a repeated START and re-addresses the target,
- a STOP follows the last operation, and only the last operation.

The repeated START on a direction change is documented by the RP2350 vendor
SVD: the DesignWare `IC_CON` register resets to `0x00000065`, with
`IC_RESTART_EN` (bit 5) set, and embassy-rp does not write that register.

Validation errors report the exact zero-based operation index in
`I2cBatchError.failed_op`. A bus failure applies to the atomic transaction as
a whole and cannot be attributed to one operation, so it reports
`failed_op = 0`.

## SPI chip-select contract

`SpiError` is serialized by variant index. The deployed indices are:

| Index | Variant | Meaning |
|-------|---------|---------|
| 0 | `BufferTooLong` | Argument exceeds its directional payload ceiling |
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

### Build identity

`DeviceInfo::build_id` carries the firmware's
`git describe --always --dirty --tags --match firmware-v*` output, captured
when the firmware was built, or `"unknown"` when git was unavailable. A
trailing `-dirty` means the image was built from a modified working tree.

It is **informational only and never a compatibility gate.** `validate()`
deliberately ignores it.

The two fields answer different questions, and conflating them would be a
mistake in both directions:

| Question | Field |
|---|---|
| *Can we talk?* | `schema_major` / `schema_minor` |
| *Are you the build I think you are?* | `build_id` |

The schema version is derived from `pico-de-gallo-internal`'s package version,
so it moves when the wire **types** change. Firmware behaviour can change while
the types do not — `i2c/batch` moved from per-operation START/STOP to a single
transaction inside an unchanged schema 0.7 — and bumping the schema version for
such a change would falsely signal a wire-format break. `build_id` covers that
gap without disturbing the compatibility axis.

The field is a `heapless::String<BUILD_ID_CAPACITY>` (64 bytes), encoded by
postcard as a plain varint-length string, so an empty value costs one byte.
Decoding an over-long string fails rather than truncating; the firmware build
script truncates.

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

## Host/firmware compatibility checks

The host library exposes `PicoDeGallo::validate()`. It calls `device/info`,
reads the firmware's schema version, and rejects mismatches early.

If validation fails, the host returns:

- `LegacyFirmware` when the firmware is too old to support `device/info`, or
- `SchemaMismatch` when the host and firmware disagree on the schema version.

This turns an otherwise confusing runtime failure into an explicit
compatibility error.

Validation compares reported *version numbers*. It is only as trustworthy as
the discipline that keeps those numbers honest: a schema bump is what makes a
shape change visible, which is why appending even a single wire enum variant
requires one.

> [!WARNING]
> Call `validate()` before doing real work. A mismatched pair is not guaranteed
> to fail fast on its own: postcard-rpc response keys include the response
> schema, so a peer that disagrees about a response type may wait indefinitely
> rather than return an error.

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
