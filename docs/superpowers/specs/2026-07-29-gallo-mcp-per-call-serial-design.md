# gallo-mcp: per-call serial selection with fail-loud disambiguation

**Date:** 2026-07-29
**Crate:** `gallo-mcp` (`crates/pico-de-gallo-mcp`)
**Issue:** [#89](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/89)
**Type:** tool-surface change (host-only; no wire-protocol, no firmware, no schema bump)

## Problem

Device selection in `gallo-mcp` is server-scoped and unobservable. `--serial-number`
is fixed at startup; when it is absent, `connect()` falls back to
`PicoDeGallo::try_new()` — "first match" — which with N>1 boards silently picks an
arbitrary one. Nothing in any tool response says which board answered.

The combination produced a real silent wrong answer (issue #89): with two boards
attached, `list_devices` reported both serials, `i2c_scan` reported an empty bus,
and `device_info` gave no way to tell which board had been scanned. The only
correct-looking conclusion from that evidence — "neither board has a temperature
sensor" — was wrong. The sensor board was simply unreachable.

The failure is worse than one bad read because several operations are stateful
across tool calls (`i2c_set_config` → `i2c_write`, `gpio_set_config` → `gpio_put`,
`onewire_search` → `onewire_search(continue)`). An ambiguous target means those can
drive the wrong pins on the wrong board.

"First match" ordering is also not stable across replug, reboot, or hub
renumbering, so which board an unpinned server binds to can change with no config
change.

PR #88 moved `gallo-mcp` to a per-tool-call connection model
(`docs/superpowers/specs/2026-07-28-gallo-mcp-per-call-connection-design.md`). No
connection is held between calls, so the target can now legitimately be chosen per
call — the architectural obstacle is already gone.

## Goal

Make the bound device **observable on every call** and **unambiguous by
construction**, without adding friction to the single-board case.

## Decisions

| Question | Decision |
|----------|----------|
| `--serial-number` CLI flag | **Keep as a hard pin.** A pinned server cannot address another board. |
| Per-call `serial_number` | Optional argument on every device-touching tool. |
| Fallback when omitted | Conditional: allowed at N==1, **error at N>=2**. |
| Serial in responses | **Uniform envelope** `{serial_number, result}` on every device tool. |
| `status` | **Never errors**; reports the full situation including ambiguity. |
| `list_devices` | Enriched: per-entry `pinned` / `default_target`, top-level rule note. |
| Threading mechanism | **Explicit `serial_number` field** on every params struct. |
| Selection policy | Extracted as a **pure function**, unit-tested without USB. |

## Selection policy

A pure function with no USB dependency:

```rust
pub(crate) fn resolve_target(
    attached:  &[Option<String>],  // serials of attached boards, enumeration order
    pin:       Option<&str>,       // server --serial-number
    requested: Option<&str>,       // per-call serial_number
) -> Result<Option<String>, SelectError>
```

`Ok(Some(serial))` is the board to open. `Ok(None)` means "open the sole attached
board, which reports no USB serial" (see *Degenerate case* below).

### Unpinned server

| Attached | `serial_number` | Outcome |
|---|---|---|
| 0 | any | `NoDevice` |
| 1 | omitted | that board |
| 1 | given, matches | that board |
| 1 | given, differs | `NotFound { requested, available }` |
| >=2 | omitted | `Ambiguous { available }` |
| >=2 | given, matches one | that board |
| >=2 | given, matches none | `NotFound { requested, available }` |

Preserving the fallback at N==1 keeps the single-board path — the overwhelmingly
common one — completely frictionless. At N>=2 an omitted serial is genuinely
ambiguous, and guessing converts a recoverable mistake into a silent wrong answer.

Erroring matters specifically because the consumer is an LLM: optional parameters
with silent defaults get omitted, so the ambiguous path would become the *common*
path. The error is self-correcting — it names the available serials, so the agent
retries correctly on the next call. Correctness stops depending on agent diligence.

### Pinned server (`-s P`)

N is irrelevant; ambiguity is impossible by construction.

| `serial_number` | Outcome |
|---|---|
| omitted | `P` attached -> `P`; else `PinnedNotFound { pin, available }` |
| equals `P` | `P` |
| differs from `P` | `PinConflict { pin, requested }` |

Allowing a *matching* per-call serial through is deliberate: an agent that
correctly names the board it is already scoped to should not be punished for being
explicit.

The pin and the per-call argument provide different guarantees, which is why both
exist. A pinned server makes "this agent session is scoped to board X" true by
construction. A per-call argument makes correctness depend on the agent supplying
it correctly on every call, indefinitely. Only the first is a hard guarantee.

### Degenerate case: a board with no USB serial

`pico_de_gallo_lib::DeviceDescription::serial_number` is `Option<String>`. Firmware
always sets one from the chip ID, but the type permits `None`.

- **Sole device, serial-less, no `serial_number` given** -> `Ok(None)`. `connect()`
  opens it with `try_new()` and the envelope reports `serial_number: null`.
- **Sole device, serial-less, `serial_number` given** -> `NotFound`.
- **N>=2 with any serial-less board, no `serial_number` given** -> `Ambiguous`. A
  serial-less board can never be named, so the error must state that *k* of *N*
  boards report no USB serial, and list only the addressable ones. Printing a bare
  list would imply the omitted boards do not exist.

## Connect

```rust
async fn connect(&self, requested: Option<&str>) -> Result<Device, ErrorData>
```

1. Take the existing connection mutex (unchanged; rmcp dispatches tool calls
   concurrently).
2. Enumerate via `pico_de_gallo_lib::list_devices()`.
3. `resolve_target(&attached, self.serial_number.as_deref(), requested)`.
4. Open **by serial** (`try_new_with_serial_number`), keeping today's bounded
   claim-retry loop for the Windows async-release window (AGENTS.md §13.17).
   `try_new()` is used only for the `Ok(None)` degenerate case.
5. `validate()`, `system_reset_subscriptions()`.
6. Store the resolved serial on the `Device` guard so the envelope helper can read
   it.

Enumeration runs on **every** call, including the pinned case, so a missing pinned
board yields `PinnedNotFound` naming the pin rather than a bare enumeration
failure. One `nusb::list_devices()` is cheap next to claiming the interface.

Two consequences:

- **Zero attached is caught before touching USB.** Enumeration returns empty,
  `resolve_target` returns `NoDevice`, no connect is attempted.
- **`NOT_FOUND` after a successful resolve now means the board was unplugged
  mid-call**, not "no device attached". That path gets its own message; reusing
  today's text would be misleading.

## Response envelope

```rust
struct Envelope<T> { serial_number: Option<String>, result: T }

fn ok_device_json<T: Serialize>(dev: &Device, value: &T) -> Result<CallToolResult, ErrorData>
```

Applied by every device tool except `status`, which reports its own shape — 41
tools across 42 return sites (`onewire_search` returns from two branches). Existing
payloads move under `result` unchanged — no other response content is altered.
`serial_number` is `null` only in the serial-less degenerate case. `ok_json` is
retained for `list_devices` and `status`.

Echoing the serial on *every* response — not just `device_info`/`status` — makes
the binding continuously observable, makes transcripts auditable, and lets an agent
notice drift on any call rather than only when it thinks to ask.

## `status`

`status` never errors. It answers "what is going on" even — especially — when
nothing is resolvable. It accepts `serial_number` like every other device tool.

```rust
struct StatusResult {
    attached: bool,                   // >=1 board present
    serial_number: Option<String>,    // board actually reached; null if unresolved
    ambiguous: bool,                  // >=2 attached and server unpinned
    available: Vec<Option<String>>,   // every attached board
    pinned: Option<String>,           // the server's -s, if any
    reason: Option<String>,           // why serial_number is null, when it is
    firmware_version: Option<String>, // populated only if a board was reached
    schema_major: Option<u16>,
    schema_minor: Option<u16>,
}
```

Today's `status` turns any connect failure into `attached: false`. Under the new
rule that would report "no board" when two are attached but ambiguous — a fresh
silent lie in exactly the scenario this issue is about.

`reason` is what keeps a null from misleading: if the agent passes a serial that
conflicts with the pin or is not attached, `status` says so rather than silently
reporting some other board's firmware.

## `list_devices`

Becomes an object so it can state the rule at the point the agent is reading
serials. It remains connectionless.

```json
{
  "devices": [
    {
      "serial_number": "9A54ED7E3A1D9D98",
      "manufacturer": "...",
      "product": "...",
      "pinned": false,
      "default_target": false
    }
  ],
  "pinned": null,
  "serial_number_required": true,
  "note": "2 devices attached and this server is not pinned; pass serial_number on every device tool call."
}
```

- `pinned` (per entry) — this board is the server's `-s` target.
- `default_target` (per entry) — a call omitting `serial_number` will use this board.
- `serial_number_required` (top level) — true iff unpinned and N>=2.
- `note` (top level) — present only when `serial_number_required`.
- `pinned` (top level) — kept separately from the per-entry flag because a pinned
  board that is **not** attached produces no entry at all, and that is precisely
  when you want to be told.

## Error text

Issue §3's wording verbatim, plus the cases it implies:

```
Multiple Pico de Gallo devices attached; `serial_number` is required.
Available: 9A54ED7E3A1D9D98, 5256657D8A5D7F03
```

```
No Pico de Gallo with serial number 'X' is attached.
Available: 9A54ED7E3A1D9D98, 5256657D8A5D7F03
```

```
This server is pinned to serial number 'P' (--serial-number); it cannot
address 'X'. Omit serial_number, or pass 'P'.
```

```
This server is pinned to serial number 'P' (--serial-number), which is not
attached. Available: 9A54ED7E3A1D9D98
```

```
No Pico de Gallo device attached: connect one and retry.
```

Ambiguity with serial-less boards present:

```
3 Pico de Gallo devices attached; `serial_number` is required, but 2 of them
report no USB serial number and cannot be addressed.
Available: 9A54ED7E3A1D9D98
```

Every message that lists serials is produced by the same formatter, so the list
format cannot drift between cases.

**Classification:**

| Error | `ErrorData` kind | Rationale |
|---|---|---|
| `Ambiguous` | `invalid_params` | The argument set is wrong; the agent can fix it. |
| `NotFound` | `invalid_params` | Ditto. |
| `PinConflict` | `invalid_params` | Ditto. |
| `NoDevice` | `internal_error` | Not the agent's fault; no argument change helps. |
| `PinnedNotFound` | `internal_error` | Server misconfiguration or unplugged board. |

## Discoverability

Three independent signals, so an agent can learn the rule *before* tripping the
error rather than paying a wasted round-trip per session:

1. **`list_devices`** — `serial_number_required` and `note`, at the point the agent
   is reading serials.
2. **Param doc-comment** — the `serial_number` field's doc states the N>=2 rule and
   therefore appears in every device tool's JSON schema.
3. **Server instructions** — `get_info()` mentions the rule once.

Plus the error itself, which names the available serials and is self-correcting.

## Threading mechanism

The 42 device tools are served by **27 distinct params structs** — `GpioWaitParams`
covers 3 tools and `PwmChannelParams` covers 4 — plus 10 tools that take no
arguments at all.

- Add an explicit `serial_number: Option<String>` field to each of the 27 existing
  params structs.
- Add **one** shared `TargetParams { serial_number }` for the 10 argument-less
  device tools (`status`, `device_info`, `version`, `i2c_get_config`, `spi_flush`,
  `spi_get_config`, `uart_flush`, `uart_get_config`, `adc_get_config`,
  `onewire_reset`). They need identical schemas, so one struct suffices; the only
  visible consequence is a shared schema `title`.
- `list_devices` gets no such field — it touches no device.

That is 28 structs to touch, not 42. This guarantees a flat, plain-object JSON
schema, the shape LLM tool-calling handles most reliably.

Sharing one struct across 10 tools assumes rmcp derives each tool's input schema
from its params type without requiring a distinct type per tool. That holds for
`schemars`-derived schemas, but it is cheap to confirm — the existing registration
test can dump the generated schemas — so it is the plan's first verification step.

Rejected alternatives:

- **`#[serde(flatten)]` a shared `Target` struct.** Single-source docs, but
  schemars 1.2 renders flattened subschemas via composition rather than always
  inlining properties. If it emits `allOf`, strict MCP clients and some
  tool-calling implementations degrade — trading certainty on the one property that
  matters for cosmetics.
- **A `targeted_params!` declarative macro generating each struct.** Gets both
  single-source docs and a flat schema, at the cost of turning every params struct
  into a macro invocation: worse to read, worse to grep, and it hides the derives.

The per-field doc line is kept short; the full rule lives in the server
instructions and the error text.

## Testing

### Unit (no USB)

- **`resolve_target` matrix** — every cell of both tables, including the degenerate
  cases: sole serial-less board resolves to `Ok(None)`; two serial-less boards are
  `Ambiguous`; a mix lists only the addressable serials.
- **Error-text tests** asserting the `Ambiguous` message contains both serials and
  the literal `` `serial_number` is required ``. This pins issue §3's contract so a
  later reword cannot quietly drop it.
- **One schema test over the whole surface** — iterate
  `router_for_test().list_all()`, skip `list_devices`, and assert each tool's input
  schema has a `serial_number` property that is **not** in `required`. A 28-struct
  mechanical edit is exactly where one gets missed; this fails loudly when it does.
- **Envelope serialization** — payload lands at `.result`, serial at
  `.serial_number`.
- **Params deserialization** — `serial_number` is optional (absent -> `None`); the
  10 new params structs deserialize from `{}`.

### Hardware (two boards, `#[ignore]`d)

Integration tests reading two serials from `GALLO_MCP_TEST_SERIAL_A` and
`GALLO_MCP_TEST_SERIAL_B`, skipped in CI, run with `cargo test -- --ignored`. Cases
requiring a physical replug (8, 9, 10) remain a manual checklist executed once and
recorded in the PR body.

| # | Setup | Call | Expected |
|---|---|---|---|
| 1 | unpinned, 2 attached | `i2c_scan` no serial | **errors**, both serials listed |
| 2 | unpinned, 2 attached | `i2c_scan` serial=A | succeeds, envelope echoes A |
| 3 | unpinned, 2 attached | `i2c_scan` serial=B | succeeds, echoes B, **result differs from A's** |
| 4 | unpinned, 2 attached | `list_devices` | `serial_number_required: true`, 2 entries, neither `default_target` |
| 5 | unpinned, 2 attached | `status` | `attached:true, ambiguous:true, serial_number:null`, both in `available` |
| 6 | unpinned, 2 attached | `i2c_scan` serial=`BOGUS` | `NotFound` + list |
| 7 | pinned to A | bare / serial=A / serial=B | succeeds / succeeds / `PinConflict` |
| 8 | pinned to A, A unplugged | any device tool | `PinnedNotFound` naming A |
| 9 | unpinned, 1 attached | `i2c_scan` no serial | succeeds, echoes that serial |
| 10 | none attached | any device tool | `NoDevice` |
| 11 | unpinned, 2 attached | `i2c_set_config(fast, A)` then `i2c_get_config(B)` | B unchanged — no config leak |

Case 3 is the critical one: it is the only test that proves per-call selection
reaches *different silicon* rather than merely returning different strings. It
relies on the two boards having distinguishable I2C buses (one empty, one with a
device at a known address).

The `gallo_*` MCP tools available inside an agent session are bound to the
currently-installed server and cannot verify a freshly built binary. Verification
drives the new code directly.

## Documentation (AGENTS.md §15.1)

| File | Change |
|---|---|
| `book/src/crates/mcp.md` | Per-call `serial_number`, the N>=2 rule, the response envelope, new `status` / `list_devices` shapes, pin semantics. |
| `crates/pico-de-gallo-mcp/README.md` | Same, briefly. |
| `CHANGELOG.md` | Keep a Changelog entry under `gallo-mcp`. |
| `AGENTS.md` §13.17 | A row for the silent-misattribution incident. |

**Out of scope, noted:** AGENTS.md §2's repo layout still lists six host crates and
omits `pico-de-gallo-mcp` entirely — pre-existing drift from PR #86, not caused by
this work.

## Scope and release

`gallo-mcp` only. No `pico-de-gallo-internal` types, endpoints, or enum ordering
are touched; no firmware change; no schema-version bump (AGENTS.md §6).

`gallo-mcp` is unpublished (still `0.1.0`), so this breaking tool-surface change
lands without a released version to supersede, and **no `[package].version` bump**
is made — per AGENTS.md §4 rule 12, feature PRs never bump versions.

Commits use Conventional Commits with the `mcp` scope, LF line endings (§3), and
the required AI attribution trailers with **no** `Signed-off-by` (§10).
