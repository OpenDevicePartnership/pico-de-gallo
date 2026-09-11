## Summary

Adds complete UART framing configuration across the shared wire protocol,
firmware, all seven host crates (`internal`, `lib`, `hal`, `ffi`, `app`, `mcp`,
and `pyco`), and a polling Zephyr UART controller, then records fake-suite and
first-on-hardware acceptance for issue #152. The HAL remains a data-I/O adapter
rather than gaining a configuration API; its documentation now states that
configuration performed through another handle must be quiesced.

The Zephyr module gains its binding, MFD child, polling driver, bridge sample,
executed recording-fake suite, build-only board harness, and CI coverage. The
hardware campaign verified real loopback, runtime configuration, all four data
widths, frame-length effects, the low-baud clamp, and RX-overrun recovery.

## Affected component(s)

- [x] firmware (`pico-de-gallo-firmware`, no_std)
- [x] wire protocol (`pico-de-gallo-internal`)
- [x] host library (`pico-de-gallo-lib`)
- [x] embedded-hal adapter (`pico-de-gallo-hal`)
- [x] C FFI (`pico-de-gallo-ffi`)
- [x] CLI application (`gallo` / `pico-de-gallo-app`)
- [x] MCP server (`gallo-mcp` / `pico-de-gallo-mcp`)
- [x] Python bindings (`pyco-de-gallo`)
- [ ] hardware (KiCad PCB / enclosure)
- [x] documentation (book, README, rustdoc)
- [x] CI / release tooling

## Related issues

Closes #152.

## Wire-protocol impact

- [ ] No wire-protocol impact.
- [ ] Adds a new endpoint or topic (append-only, non-breaking).
- [ ] Appends a new variant to an existing wire enum (non-breaking).
- [ ] **BREAKING**: changes existing request/response types or reorders enum
  variants. I bumped `pico-de-gallo-internal` accordingly and updated firmware
  + all host crates in lockstep.

None of the template choices precisely describes an in-flight unreleased schema
change: this PR changes existing request/response shapes but deliberately does
**not** bump a package version. `UartSetConfigurationRequest` and
`UartConfigurationInfo` now carry
`baud_rate`, `data_bits`, `parity`, and `stop_bits`. It adds three wire enums in
this exact source order:

- `UartDataBits`: `Five`, `Six`, `Seven`, `Eight`;
- `UartParity`: `None`, `Odd`, `Even`, `Mark`, `Space`;
- `UartStopBits`: `One`, `Two`.

Variant order is ABI: postcard serializes enums by variant index. Never reorder
these variants.

This change rides the already pending, **unreleased** schema 0.8.0. The highest
published internal tag is `internal-v0.7.0`; there is no
`internal-v0.8.0`. Per AGENTS.md §4 rule 12, package versions move only in a
deliberate release commit, so this feature PR does not edit any
`[package].version`. Until 0.8.0 is released, host and firmware must be built
from the same tree; two development builds can both report 0.8.0 while carrying
different endpoint keys or shapes.

## Testing performed

- [ ] `cargo fmt --check`
- [ ] `cargo clippy --all-targets --locked -- -D warnings`
- [ ] `cargo test --locked`
- [ ] Firmware, `hw-rev2` (default): build and clippy for
  `thumbv8m.main-none-eabihf`, release mode, `--locked`.
- [ ] Firmware, `hw-rev1` (deprecated, still published): same two commands with
  `--no-default-features --features hw-rev1`.
- [x] Tested on real hardware (describe below)

M6's recorded verification completed 11 Twister scenarios with 0 failed and 0
errored: 3 executed, 8 build-only, and 19/19 executed test cases passed. The UART
post-link gate checked six bottom symbols: five strong fake overrides, one
unreferenced absence, and zero surviving weak production definitions.
`ci-build.sh --self-test` reported 17 passed and 0 failed, and its then-current
12-target table met every declared contract.

M7 ran from 20:38–21:01 UTC on board `5256657D8A5D7F03`, hw-rev2, with UART TX
GPIO 0 shorted to RX GPIO 1. Firmware build
`firmware-v0.11.0-109-g6c4d42fd0796` reported firmware 0.12.0 and schema 0.8.0.
Seven identity gates matched; no reset, re-enumeration, or wedge occurred.

The first real execution of `uart_bridge` returned all 27 greeting bytes
exactly. `run_exit=124` was the intentional 120-second process bound:
`native_sim` idles after `main()` and otherwise retains the USB claim.

- **VERIFIED — hw-rev2 strict open, readiness, and sample:** ready log and exact
  27/27-byte loopback.
- **VERIFIED — runtime configure/get and invalid config:** 7N1 and restored 8N1
  matched; baud 0 returned `-EINVAL`, unsupported data returned `-ENOTSUP`.
- **VERIFIED — 5/6/7/8-bit data width:** all four returned their predicted
  masks.
- **VERIFIED — frame length:** 64 bytes at 9600 took 65/72/77/78 ms; fitted
  slope 6.1818 ms per bit/character, 92.7% of prediction.
- **VERIFIED — low-baud clamp:** measured approximately 147 baud; convergence
  began between requested rates 143 and 100.
- **VERIFIED — RX-overrun recovery:** 3/3 overflow trials errored exactly once
  and recovered; 2/2 controls did not error.
- **PARTIAL — wrong serial:** refused in 511 ms and named the selector; other
  strict-open failure modes remain untested.
- **INCONCLUSIVE — CLI timing:** process quantization overwhelmed the effect and
  the baud control had the wrong sign.
- **UNVERIFIABLE ON THIS RIG — parity selection:** one PL011 drives both ends;
  an independent peer or analyser is required.
- The M7 record gives the complete verdict for all 18 M6 obligations.

The complete evidence and residual list are in
`docs/superpowers/specs/2026-09-11-zephyr-uart-hardware-m7-record.md`.

### Known findings, not fixed

1. **`gallo uart read -c N -t T` can return short long before the timeout.**
   `read -c 64 -t 5000` returned 63 bytes in 206–306 ms, and the immediate next
   read retrieved byte 64. This reproduced 7/100 times at 115200. At low baud,
   4–12 of 16 bytes returned after about 306 ms despite a 12-second timeout.
   Callers trusting count-within-timeout can silently mis-frame. The Zephyr
   driver polls one byte at a time and is not exposed, but the CLI, library,
   FFI, and Python are. This PR does not patch the finding; file a separate
   issue.
2. `native_sim` elapsed time is simulated. The frame-length slope is physical,
   but absolute drain milliseconds are not wall-clock values and have a 2 ms
   polling quantum.
3. Separate `gallo write` and `gallo read` processes cannot measure wire timing;
   startup and scheduling dominate, and the decisive baud control had the wrong
   sign. Future timing probes must write and read in one process.
4. An RX error consumed exactly one byte in both nonce trials: `de ad be ef`
   returned as `ad be ef`.
5. A `native_sim` binary keeps the board claimed after `main()` returns until
   the process is killed.
6. LSP errors reported in the library and Python sources did not reproduce in
   compilation and were stale-index diagnostics, not defects.

### Unverifiable on the available hardware

- Odd/even and mark/space parity require an independent UART peer or logic
  analyser.
- hw-rev1 capability refusal requires a v1.0 board.
- Capability-query and initial-config failure, transport/timeout errno mapping,
  lost replies, unplug/re-enumeration/reset generations, queued mutex callers,
  lost acknowledgements, and active-console faults require a fault-injecting
  USB proxy or console fault injection plus permitted recovery operations.
- TX-ring saturation requires BOOTSEL/reflash recovery access before risking the
  supervisor-forced reset expected near 90 seconds.
- Direct observation of embassy's `rx_error` and RX interrupt-enable bits needs
  RTT or a debugger. M7 measured their externally visible consequences only.
- The two-image `REQ_KEY` A/B requires a second firmware image and a BOOTSEL
  press.

## Book ↔ code parity

- [ ] No book-visible behavior changed in this PR.
- [x] Book chapters updated in this PR to match the code change. List them:
  - `book/src/appendix/endpoints.md`
  - `book/src/appendix/status-codes.md`
  - `book/src/crates/app.md`
  - `book/src/crates/ffi.md`
  - `book/src/crates/hal.md`
  - `book/src/crates/lib.md`
  - `book/src/crates/mcp.md`
  - `book/src/crates/python.md`
  - `book/src/interfaces/uart.md`
  - `book/src/internals/firmware.md`
  - `book/src/internals/releases.md`
  - `book/src/internals/wire-protocol.md`
- [ ] If only the book changed: I re-verified the code still matches what the
  new book text claims, and re-derived any tables (endpoints, status codes,
  capability bits) from the source files in AGENTS.md §15.1.
- [ ] `mdbook build book` is clean locally.

The endpoint and wire-enum documentation was checked against
`pico-de-gallo-internal`; the enum tables retain source order. FFI documentation
was checked against the exported UART configuration enums and functions.

There is intentionally no dedicated Zephyr book chapter. AGENTS.md §15.1's
carve-out keeps `zephyr/README.md` and `zephyr/CHANGELOG.md` authoritative while
the module is WIP. The book still updates every host surface and UART behaviour
it already describes.

`.github/workflows/zephyr.yml` **will fire** because this PR changes
`zephyr/**`, `crates/pico-de-gallo-ffi/**`,
`crates/pico-de-gallo-internal/**`, and the workflow itself. A green run proves
that the module compiles and links and that the hardware-free recording-fake
suites pass. It does **not** prove behaviour against a real board; the M7 record
is the separate board-attached evidence.

## Checklist

- [x] Commits follow Conventional Commits with a correct scope.
- [ ] `Cargo.lock` is committed alongside any `Cargo.toml` change (host **and**
  firmware workspaces, as relevant). I ran with `--locked`.
- [ ] New `=X.Y.Z` exact pins are documented in
  `.github/copilot-instructions.md` under "Pinned dependency rationale".
- [x] Public items have rustdoc; PyO3 items have docstrings.
- [x] `book/` updated for new endpoints, CLI flags, or behavior changes (see
  "Book ↔ code parity" above).
- [x] Each changed crate's `crates/<crate>/CHANGELOG.md` (or
  `zephyr/CHANGELOG.md`) follows Keep a Changelog (hand-written; not
  auto-generated). There is no root `CHANGELOG.md`.
- [x] AI-assisted commits include `Co-authored-by: Copilot` and `Assisted-by:`
  trailers; no `Signed-off-by:` on AI commits.

No `Cargo.toml`, `Cargo.lock`, package version, dependency, or exact pin changes
are part of this branch, so the two unchecked dependency checkboxes are not
applicable. Build/test checkboxes above remain unchecked until the concurrently
running final verification reports its results.
