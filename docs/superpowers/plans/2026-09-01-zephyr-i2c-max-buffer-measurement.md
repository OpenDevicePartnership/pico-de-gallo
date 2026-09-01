# I2C Transfer-Ceiling Measurement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Measure the real `i2c/write` and `i2c/write-read` payload ceilings on attached hardware, and set `PDG_I2C_MAX_BUFFER` from that measurement instead of from the unmeasured 4096-byte protocol constant.

**Architecture:** A throwaway Rust harness in `/tmp/opencode/i2c-probe/` drives one RPC per probe under a timeout, classifies the result into returned / errored / hung, and recovers a hung board via `USBDEVFS_RESET`. Three sweep phases (write edge, read edge, write+read frontier) locate the boundary by coarse ladder then bisection. The resulting numbers drive a one-line constant change plus documentation across the Zephyr module and the book.

**Tech Stack:** Rust 1.97 / edition 2024, `pico-de-gallo-lib` 0.8.0 by path dependency, `tokio` (rt-multi-thread, macros, time), `libc` for the `USBDEVFS_RESET` ioctl, Linux sysfs for device resolution. Zephyr via `~/zephyrproject` with `west` and `twister`. `mdbook` for the book.

**Spec:** `docs/superpowers/specs/2026-09-01-zephyr-i2c-max-buffer-measurement.md`

**Board under test:** serial `49742081C885AC69`, firmware 0.11.0, schema 0.7, hardware revision 2. Bus has exactly one target, a TMP102 at `0x48`. Address `0x50` is unpopulated and is the write-probe target.

---

## File Structure

**Harness — throwaway, never committed.** Lives outside the repo so it cannot be committed by accident.

| File | Responsibility |
|---|---|
| `/tmp/opencode/i2c-probe/Cargo.toml` | package manifest, path deps into the repo |
| `/tmp/opencode/i2c-probe/src/record.rs` | CSV line formatting and append-and-flush recorder |
| `/tmp/opencode/i2c-probe/src/usb.rs` | sysfs device resolution and `USBDEVFS_RESET` |
| `/tmp/opencode/i2c-probe/src/probe.rs` | `Outcome`, the single-RPC probe, liveness ping, recovery loop |
| `/tmp/opencode/i2c-probe/src/search.rs` | pure `bisect` helper, unit-tested |
| `/tmp/opencode/i2c-probe/src/main.rs` | phase orchestration and CLI phase selection |
| `/tmp/opencode/i2c-probe/results.csv` | raw measurement output |

Split by responsibility: USB recovery knows nothing about I2C, the search is pure and testable without hardware, and the recorder is independent of both.

**Repository — what actually lands.**

| File | Change |
|---|---|
| `zephyr/drivers/i2c/pdg_i2c.c:101` | `PDG_I2C_MAX_BUFFER` value and its comment |
| `zephyr/README.md` | measured-limits paragraph and the `-EMSGSIZE` table row |
| `zephyr/CHANGELOG.md` | Changed entry |
| `book/src/interfaces/batching.md` | the "no general ceiling is published" claim |
| `book/src/interfaces/i2c.md` | I2C containment note |
| `book/src/appendix/troubleshooting.md` | I2C limit narrative and host-surface reachability warning |
| `AGENTS.md` | §13.17 row, only if a hang is found |

---

## Task 1: Scaffold the harness and the recorder

**Files:**
- Create: `/tmp/opencode/i2c-probe/Cargo.toml`
- Create: `/tmp/opencode/i2c-probe/src/main.rs`
- Create: `/tmp/opencode/i2c-probe/src/record.rs`

- [ ] **Step 1: Create the package directory**

```bash
mkdir -p /tmp/opencode/i2c-probe/src
```

- [ ] **Step 2: Write the manifest**

Create `/tmp/opencode/i2c-probe/Cargo.toml`:

```toml
[package]
name = "i2c-probe"
version = "0.1.0"
edition = "2024"
publish = false

[dependencies]
pico-de-gallo-lib = { path = "/home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-lib" }
pico-de-gallo-internal = { path = "/home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-internal", features = ["use-std"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"] }
libc = "0.2"
```

- [ ] **Step 3: Write the failing test for CSV formatting**

Create `/tmp/opencode/i2c-probe/src/record.rs`:

```rust
//! Append-and-flush CSV recording. Every probe is written immediately so a
//! crash or a power-cycle never loses data already gathered.

use std::fs::{File, OpenOptions};
use std::io::Write;

/// Formats one probe result as a CSV line, without the trailing newline.
///
/// `detail` is free text and is quoted, because error Debug output contains
/// commas.
pub fn csv_line(kind: &str, w_len: u32, r_len: u32, outcome: &str, detail: &str, elapsed_ms: u128) -> String {
    let escaped = detail.replace('"', "'");
    format!("{kind},{w_len},{r_len},{outcome},\"{escaped}\",{elapsed_ms}")
}

/// Owns the results file and appends to it.
pub struct Recorder {
    file: File,
}

impl Recorder {
    /// Opens `path`, creating it and writing the header if it is new.
    pub fn open(path: &str) -> std::io::Result<Self> {
        let is_new = !std::path::Path::new(path).exists();
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        if is_new {
            writeln!(file, "kind,w_len,r_len,outcome,detail,elapsed_ms")?;
            file.flush()?;
        }
        Ok(Self { file })
    }

    /// Appends one line and flushes immediately.
    pub fn record(&mut self, line: &str) -> std::io::Result<()> {
        writeln!(self.file, "{line}")?;
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_line_quotes_detail_and_orders_fields() {
        let line = csv_line("write", 1013, 0, "nack", "NoAcknowledge", 7);
        assert_eq!(line, "write,1013,0,nack,\"NoAcknowledge\",7");
    }

    #[test]
    fn csv_line_replaces_embedded_quotes() {
        let line = csv_line("write", 1, 0, "err", "Comms(\"boom\")", 1);
        assert_eq!(line, "write,1,0,err,\"Comms('boom')\",1");
    }
}
```

- [ ] **Step 4: Write a placeholder main so the crate builds**

Create `/tmp/opencode/i2c-probe/src/main.rs`:

```rust
mod record;

fn main() {
    println!("i2c-probe");
}
```

- [ ] **Step 5: Run the tests and verify they pass**

```bash
cd /tmp/opencode/i2c-probe && cargo test
```

Expected: `test result: ok. 2 passed`. If the path dependency fails to resolve, the repo checkout has moved — fix the paths in `Cargo.toml`, do not vendor the crate.

---

## Task 2: USB resolution and reset

**Files:**
- Create: `/tmp/opencode/i2c-probe/src/usb.rs`
- Modify: `/tmp/opencode/i2c-probe/src/main.rs`

- [ ] **Step 1: Write the module**

Create `/tmp/opencode/i2c-probe/src/usb.rs`:

```rust
//! Device resolution via sysfs and re-enumeration via USBDEVFS_RESET.
//!
//! The bus path changes across re-enumeration, so it is resolved fresh on
//! every call and never cached.

use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

const VID: &str = "045e";
const PID: &str = "067d";

/// `_IO('U', 20)` — Linux `USBDEVFS_RESET`.
const USBDEVFS_RESET: libc::c_ulong = 0x5514;

/// Returns the `/dev/bus/usb/BBB/DDD` node for the board with `serial`.
///
/// Walks `/sys/bus/usb/devices/` matching vendor, product and serial, then
/// reads `busnum` and `devnum` to build the device node path.
pub fn resolve_node(serial: &str) -> Option<String> {
    let entries = std::fs::read_dir("/sys/bus/usb/devices").ok()?;
    for entry in entries.flatten() {
        let dir = entry.path();
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok().map(|s| s.trim().to_string());

        if read("idVendor").as_deref() != Some(VID) {
            continue;
        }
        if read("idProduct").as_deref() != Some(PID) {
            continue;
        }
        if read("serial").as_deref() != Some(serial) {
            continue;
        }

        let bus: u32 = read("busnum")?.parse().ok()?;
        let dev: u32 = read("devnum")?.parse().ok()?;
        return Some(format!("/dev/bus/usb/{bus:03}/{dev:03}"));
    }
    None
}

/// Issues `USBDEVFS_RESET` on the board, forcing re-enumeration.
///
/// Returns an error string rather than panicking: a failed reset is a
/// result worth recording, not a crash.
pub fn reset(serial: &str) -> Result<(), String> {
    let node = resolve_node(serial).ok_or_else(|| format!("no device node for serial {serial}"))?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&node)
        .map_err(|e| format!("open {node}: {e}"))?;

    // SAFETY: `file` is an open USB device node and USBDEVFS_RESET takes no
    // argument, so the third parameter is ignored by the kernel.
    let rc = unsafe { libc::ioctl(file.as_raw_fd(), USBDEVFS_RESET, 0) };
    if rc != 0 {
        return Err(format!("USBDEVFS_RESET on {node}: {}", std::io::Error::last_os_error()));
    }
    Ok(())
}

/// Blocks until the board reappears in sysfs, up to `timeout`.
pub fn wait_for_node(serial: &str, timeout: Duration) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(node) = resolve_node(serial) {
            // The node exists but udev may not have applied its ACL yet.
            std::thread::sleep(Duration::from_millis(500));
            return Ok(node);
        }
        if Instant::now() >= deadline {
            return Err(format!("device {serial} did not reappear within {timeout:?}"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
```

- [ ] **Step 2: Register the module**

Replace `/tmp/opencode/i2c-probe/src/main.rs` with:

```rust
mod record;
mod usb;

fn main() {
    let serial = "49742081C885AC69";
    match usb::resolve_node(serial) {
        Some(node) => println!("resolved {serial} to {node}"),
        None => println!("could not resolve {serial}"),
    }
}
```

- [ ] **Step 3: Verify resolution works against the live board**

```bash
cd /tmp/opencode/i2c-probe && cargo run
```

Expected: `resolved 49742081C885AC69 to /dev/bus/usb/001/025` (the device number will differ if the board has re-enumerated since).

If it prints `could not resolve`, the board is unplugged or the serial is wrong. Confirm with `lsusb -d 045e:067d` before continuing — every later task assumes resolution works.

- [ ] **Step 4: Verify reset works, before relying on it for recovery**

This is the single riskiest assumption in the plan (spec §8), so test it in isolation now rather than discovering it mid-sweep.

Temporarily replace the body of `main` with:

```rust
fn main() {
    let serial = "49742081C885AC69";
    println!("before: {:?}", usb::resolve_node(serial));
    match usb::reset(serial) {
        Ok(()) => println!("reset issued"),
        Err(e) => println!("reset failed: {e}"),
    }
    match usb::wait_for_node(serial, std::time::Duration::from_secs(15)) {
        Ok(node) => println!("after: {node}"),
        Err(e) => println!("did not reappear: {e}"),
    }
}
```

Run it:

```bash
cd /tmp/opencode/i2c-probe && cargo run
```

Expected: `reset issued` followed by `after: /dev/bus/usb/001/NNN` with a **different** device number than `before`. A changed device number is the evidence that re-enumeration actually happened.

**Stop and report if the reset fails or the node does not reappear.** Without working recovery the sweep cannot bisect through a hang, and the plan needs revisiting with the user rather than pressing on.

---

## Task 3: The pure bisection helper

**Files:**
- Create: `/tmp/opencode/i2c-probe/src/search.rs`
- Modify: `/tmp/opencode/i2c-probe/src/main.rs`

Written before the hardware probe deliberately: it is the only part of the harness that can be tested without a board, and a bisection bug would silently produce a wrong boundary.

- [ ] **Step 1: Write the failing test**

Create `/tmp/opencode/i2c-probe/src/search.rs` with the test module only, plus a stub:

```rust
//! Pure boundary search. No hardware, no I/O — so it can be tested.

/// Finds the largest value in `(lo, hi)` for which `passes` holds.
///
/// Caller must establish the invariant: `passes(lo)` is true and
/// `passes(hi)` is false. Returns the largest known-passing value.
pub fn bisect<F>(_lo: u32, _hi: u32, _passes: F) -> u32
where
    F: FnMut(u32) -> bool,
{
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_exact_edge_of_a_step_function() {
        // passes for n <= 1013, fails above.
        let edge = bisect(1, 4096, |n| n <= 1013);
        assert_eq!(edge, 1013);
    }

    #[test]
    fn handles_adjacent_bounds_without_probing() {
        let mut probes = 0;
        let edge = bisect(100, 101, |_| {
            probes += 1;
            true
        });
        assert_eq!(edge, 100);
        assert_eq!(probes, 0, "adjacent bounds leave nothing to probe");
    }

    #[test]
    fn finds_edge_at_the_bottom_of_the_range() {
        let edge = bisect(1, 4096, |n| n <= 1);
        assert_eq!(edge, 1);
    }

    #[test]
    fn finds_edge_just_below_the_top() {
        let edge = bisect(1, 4096, |n| n <= 4095);
        assert_eq!(edge, 4095);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

```bash
cd /tmp/opencode/i2c-probe && cargo test search
```

Expected: FAIL, panicking at `not yet implemented`.

- [ ] **Step 3: Implement**

Replace the `bisect` stub with:

```rust
pub fn bisect<F>(mut lo: u32, mut hi: u32, mut passes: F) -> u32
where
    F: FnMut(u32) -> bool,
{
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if passes(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cd /tmp/opencode/i2c-probe && cargo test search
```

Expected: `test result: ok. 4 passed`.

- [ ] **Step 5: Register the module**

Add `mod search;` to `/tmp/opencode/i2c-probe/src/main.rs`.

---

## Task 4: The probe

**Files:**
- Create: `/tmp/opencode/i2c-probe/src/probe.rs`
- Modify: `/tmp/opencode/i2c-probe/src/main.rs`

- [ ] **Step 1: Write the module**

Create `/tmp/opencode/i2c-probe/src/probe.rs`:

```rust
//! One RPC per probe, under a timeout, on a fresh connection.
//!
//! A fresh connection per probe means a hung probe cannot leak transport
//! state into the next one. On Linux the interface is released
//! synchronously on drop, so the repeated open/close does not hit the
//! WinUSB exclusive-claim problem recorded in AGENTS.md 13.17 (2026-07-20).

use std::time::{Duration, Instant};

use pico_de_gallo_lib::PicoDeGallo;

use crate::usb;

/// 4096 bytes at 100 kHz standard-mode I2C is roughly 0.4 s of bus time,
/// so this is about a 10x margin. A false `Hang` would corrupt the
/// boundary, so the bias is deliberately toward over-waiting.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// What happened to a single probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The call returned success.
    Ok,
    /// `I2cError::NoAcknowledge` — the healthy answer at an unpopulated
    /// address. Counts as "the call returned".
    Nack,
    /// Any other error, recorded verbatim. A working transport refusing a
    /// request.
    Err(String),
    /// The timeout fired. A wedged dispatcher.
    Hang,
}

impl Outcome {
    /// True when the call returned at all, which is the property under
    /// measurement. Both `Ok` and `Nack` qualify.
    pub fn returned(&self) -> bool {
        matches!(self, Outcome::Ok | Outcome::Nack)
    }

    /// Short tag for the CSV `outcome` column.
    pub fn tag(&self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Nack => "nack",
            Outcome::Err(_) => "err",
            Outcome::Hang => "hang",
        }
    }

    /// Free-text detail for the CSV `detail` column.
    pub fn detail(&self) -> String {
        match self {
            Outcome::Err(e) => e.clone(),
            other => format!("{other:?}"),
        }
    }
}

/// Classifies a `Result` from either I2C call into an `Outcome`.
fn classify<T>(result: Result<T, pico_de_gallo_lib::PicoDeGalloError<pico_de_gallo_internal::I2cError>>) -> Outcome {
    match result {
        Ok(_) => Outcome::Ok,
        Err(pico_de_gallo_lib::PicoDeGalloError::Endpoint(pico_de_gallo_internal::I2cError::NoAcknowledge)) => {
            Outcome::Nack
        }
        Err(e) => Outcome::Err(format!("{e}")),
    }
}

/// What kind of RPC a probe issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `i2c/write` of `w_len` bytes. `r_len` is ignored.
    Write,
    /// `i2c/write-read` of `w_len` bytes out and `r_len` bytes back.
    WriteRead,
}

impl Kind {
    pub fn tag(&self) -> &'static str {
        match self {
            Kind::Write => "write",
            Kind::WriteRead => "write_read",
        }
    }
}

/// Opens the board, issues exactly one RPC under `PROBE_TIMEOUT`, closes.
///
/// `w_len` of 0 is never issued: the host library refuses an empty write
/// with `ZeroLengthWrite` and the firmware refuses it too (issue #101), so
/// zero is not part of the measurable range.
pub async fn probe(serial: &str, kind: Kind, address: u8, w_len: u32, r_len: u32) -> (Outcome, u128) {
    let started = Instant::now();

    let pg = match PicoDeGallo::try_new_with_serial_number(serial) {
        Ok(pg) => pg,
        Err(e) => return (Outcome::Err(format!("open failed: {e}")), started.elapsed().as_millis()),
    };

    let payload = vec![0u8; w_len as usize];

    let outcome = match kind {
        Kind::Write => {
            let call = pg.i2c_write(address, &payload);
            match tokio::time::timeout(PROBE_TIMEOUT, call).await {
                Ok(result) => classify(result),
                Err(_) => Outcome::Hang,
            }
        }
        Kind::WriteRead => {
            let call = pg.i2c_write_read(address, &payload, r_len as u16);
            match tokio::time::timeout(PROBE_TIMEOUT, call).await {
                Ok(result) => classify(result),
                Err(_) => Outcome::Hang,
            }
        }
    };

    (outcome, started.elapsed().as_millis())
}

/// Cheap liveness check. Run before every probe so that a wedge is
/// detected before its failures get attributed to the next length.
pub async fn alive(serial: &str) -> bool {
    let Ok(pg) = PicoDeGallo::try_new_with_serial_number(serial) else {
        return false;
    };
    matches!(tokio::time::timeout(Duration::from_secs(2), pg.ping(0xC0FFEE)).await, Ok(Ok(_)))
}

/// Resets the board and waits for it to answer a ping again.
///
/// Returns `Err` if the board is still unreachable after `attempts`
/// rounds, so the caller can stop rather than record garbage from a
/// still-wedged board.
pub async fn recover(serial: &str, attempts: u32) -> Result<(), String> {
    for attempt in 1..=attempts {
        eprintln!("  recover: attempt {attempt}/{attempts}");

        if let Err(e) = usb::reset(serial) {
            eprintln!("  recover: reset failed: {e}");
        }
        match usb::wait_for_node(serial, Duration::from_secs(15)) {
            Ok(node) => eprintln!("  recover: reappeared at {node}"),
            Err(e) => {
                eprintln!("  recover: {e}");
                continue;
            }
        }
        if alive(serial).await {
            eprintln!("  recover: board is answering again");
            return Ok(());
        }
    }
    Err(format!(
        "board {serial} still unreachable after {attempts} reset attempts; power-cycle it"
    ))
}
```

- [ ] **Step 2: Register the module**

`/tmp/opencode/i2c-probe/src/main.rs` module list becomes:

```rust
mod probe;
mod record;
mod search;
mod usb;
```

- [ ] **Step 3: Verify it compiles**

```bash
cd /tmp/opencode/i2c-probe && cargo build 2>&1 | tail -20
```

Expected: compiles. If `PicoDeGalloError` or `I2cError` paths are wrong, check the real ones with:

```bash
grep -n "pub use\|pub enum PicoDeGalloError" /home/balbi/workspace/pico-de-gallo/crates/pico-de-gallo-lib/src/lib.rs | head
```

---

## Task 5: Phase orchestration

**Files:**
- Modify: `/tmp/opencode/i2c-probe/src/main.rs`

- [ ] **Step 1: Write the orchestrator**

Replace `/tmp/opencode/i2c-probe/src/main.rs` entirely:

```rust
//! Measures the i2c/write and i2c/write-read payload ceilings.
//!
//! Issue #146. See
//! docs/superpowers/specs/2026-09-01-zephyr-i2c-max-buffer-measurement.md

mod probe;
mod record;
mod search;
mod usb;

use probe::{Kind, Outcome};
use record::Recorder;

const SERIAL: &str = "49742081C885AC69";

/// Unpopulated. The full payload still crosses USB and is decoded, so
/// request framing is fully exercised, but nothing is written to a real
/// device. The healthy outcome here is `Nack`, not `Ok`.
const EMPTY_ADDR: u8 = 0x50;

/// The TMP102. Clocks out its register pair repeatedly for arbitrary read
/// lengths, so a genuine N-byte response is produced.
const TMP102_ADDR: u8 = 0x48;

const RESULTS: &str = "/tmp/opencode/i2c-probe/results.csv";

/// Comparable to the SPI table, and checks the SPI edge pair 1013/1015
/// explicitly even if the real I2C edge is elsewhere.
const LADDER: &[u32] = &[1, 64, 256, 512, 1013, 1015, 1024, 2048, 3072, 4096];

/// Runs one probe with a liveness check before it and recovery after it.
async fn measured(rec: &mut Recorder, kind: Kind, address: u8, w_len: u32, r_len: u32) -> Outcome {
    if !probe::alive(SERIAL).await {
        eprintln!("board not answering before probe; recovering first");
        probe::recover(SERIAL, 3).await.expect("board unrecoverable");
    }

    let (outcome, elapsed_ms) = probe::probe(SERIAL, kind, address, w_len, r_len).await;
    println!("{:>10} w={w_len:<5} r={r_len:<5} -> {outcome:?} ({elapsed_ms} ms)", kind.tag());

    let line = record::csv_line(kind.tag(), w_len, r_len, outcome.tag(), &outcome.detail(), elapsed_ms);
    rec.record(&line).expect("failed to record result");

    if outcome == Outcome::Hang {
        probe::recover(SERIAL, 3).await.expect("board unrecoverable after hang");
    }

    outcome
}

/// Walks the ladder and returns (largest passing rung, smallest failing
/// rung). `None` for the failing rung means the whole ladder passed.
async fn ladder(rec: &mut Recorder, kind: Kind, address: u8, r_len_for: impl Fn(u32) -> (u32, u32)) -> (u32, Option<u32>) {
    let mut last_pass = 0;
    for &n in LADDER {
        let (w, r) = r_len_for(n);
        if measured(rec, kind, address, w, r).await.returned() {
            last_pass = n;
        } else {
            return (last_pass, Some(n));
        }
    }
    (last_pass, None)
}

#[tokio::main]
async fn main() {
    let phase = std::env::args().nth(1).unwrap_or_else(|| "all".to_string());
    let mut rec = Recorder::open(RESULTS).expect("failed to open results file");

    if phase == "a" || phase == "all" {
        phase_a(&mut rec).await;
    }
    if phase == "b" || phase == "all" {
        phase_b(&mut rec).await;
    }
    if phase == "c" || phase == "all" {
        phase_c(&mut rec).await;
    }
}

/// Phase A: the write-only edge, at an unpopulated address.
async fn phase_a(rec: &mut Recorder) {
    println!("=== Phase A: i2c/write edge at {EMPTY_ADDR:#04x} ===");

    let (last_pass, first_fail) = ladder(rec, Kind::Write, EMPTY_ADDR, |n| (n, 0)).await;

    match first_fail {
        None => println!("Phase A: whole ladder returned, up to {last_pass}"),
        Some(fail) => {
            println!("Phase A: bracketed ({last_pass}, {fail}); bisecting");
            let edge = bisect_async(rec, Kind::Write, EMPTY_ADDR, last_pass, fail, |n| (n, 0)).await;
            println!("Phase A edge: {edge}");
        }
    }
}

/// Phase B: the read-only edge, 1-byte pointer write to the TMP102.
async fn phase_b(rec: &mut Recorder) {
    println!("=== Phase B: i2c/write-read read edge at {TMP102_ADDR:#04x} ===");

    let (last_pass, first_fail) = ladder(rec, Kind::WriteRead, TMP102_ADDR, |n| (1, n)).await;

    match first_fail {
        None => println!("Phase B: whole ladder returned, up to {last_pass}"),
        Some(fail) => {
            println!("Phase B: bracketed ({last_pass}, {fail}); bisecting");
            let edge = bisect_async(rec, Kind::WriteRead, TMP102_ADDR, last_pass, fail, |n| (1, n)).await;
            println!("Phase B edge: {edge}");
        }
    }
}

/// Phase C: the write+read frontier.
///
/// Distinguishes a rectangular bound (independent per-direction limits)
/// from a diagonal one (a shared `w + r` budget). Two independent 1-D
/// bisections cannot tell those apart, which is the mistake that produced
/// the wrong SPI 3072-byte duplex guess.
async fn phase_c(rec: &mut Recorder) {
    println!("=== Phase C: write+read frontier at {TMP102_ADDR:#04x} ===");

    // Fill these in from the Phase A and Phase B results before running.
    let write_lengths: &[u32] = &[1, 64, 256, 512, 1013];
    let read_hi = 4096;

    for &w in write_lengths {
        // Establish the invariant: r=1 must return, r=read_hi must not.
        if !measured(rec, Kind::WriteRead, TMP102_ADDR, w, 1).await.returned() {
            println!("frontier w={w}: even r=1 does not return; skipping");
            continue;
        }
        if measured(rec, Kind::WriteRead, TMP102_ADDR, w, read_hi).await.returned() {
            println!("frontier w={w}: r={read_hi} returns; no edge below it");
            continue;
        }
        let edge = bisect_async(rec, Kind::WriteRead, TMP102_ADDR, 1, read_hi, |r| (w, r)).await;
        println!("frontier w={w}: max r = {edge} (sum {})", w + edge);
    }
}

/// `search::bisect` cannot take an async predicate, so the loop is
/// reimplemented here over the same invariant. The pure version stays
/// unit-tested as the reference for this logic.
async fn bisect_async(
    rec: &mut Recorder,
    kind: Kind,
    address: u8,
    mut lo: u32,
    mut hi: u32,
    lens: impl Fn(u32) -> (u32, u32),
) -> u32 {
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        let (w, r) = lens(mid);
        if measured(rec, kind, address, w, r).await.returned() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}
```

- [ ] **Step 2: Verify it compiles and the unit tests still pass**

```bash
cd /tmp/opencode/i2c-probe && cargo build && cargo test
```

Expected: builds; `6 passed` across `record` and `search`.

---

## Task 6: Save the TMP102 state

**Files:** none — this is a recorded observation, not a code change.

- [ ] **Step 1: Read and write down the three registers**

Phases B and C only ever write the 1-byte pointer, so this should be a no-op. It exists to catch the case where it is not.

Use the MCP tools:

```
i2c_write_read address=0x48 data="0x01" count=2   # CONFIG
i2c_write_read address=0x48 data="0x02" count=2   # TLOW
i2c_write_read address=0x48 data="0x03" count=2   # THIGH
```

Record all three values in the working notes. They are restored and verified in Task 10.

---

## Task 7: Run Phase A

**Files:**
- Produces: `/tmp/opencode/i2c-probe/results.csv`

- [ ] **Step 1: Confirm the bus is what the plan assumes**

```
i2c_scan
```

Expected: exactly `["0x48"]`. If `0x50` appears, pick a different unpopulated address and update `EMPTY_ADDR` — an ACK at the write-probe address would make `Ok` outcomes ambiguous.

- [ ] **Step 2: Run the phase**

```bash
cd /tmp/opencode/i2c-probe && cargo run --release -- a 2>&1 | tee /tmp/opencode/i2c-probe/phase-a.log
```

Expected: a line per probe, ending with either `Phase A: whole ladder returned, up to 4096` or `Phase A edge: NNNN`.

- [ ] **Step 3: Sanity-check the outcomes before trusting the edge**

```bash
cut -d, -f4 /tmp/opencode/i2c-probe/results.csv | sort | uniq -c
```

Every small-length probe should be `nack`. If small lengths report `ok`, something is answering at `EMPTY_ADDR` and the phase must be re-run against a genuinely empty address. If small lengths report `err`, the transport is broken and the run is invalid — do not read an edge out of it.

- [ ] **Step 4: Record the finding**

Write the Phase A edge, and every `hang` length, into the working notes. Note explicitly which lengths inside any hang window were **not** probed — the spec requires that stated plainly.

---

## Task 8: Run Phase B

- [ ] **Step 1: Run the phase**

```bash
cd /tmp/opencode/i2c-probe && cargo run --release -- b 2>&1 | tee /tmp/opencode/i2c-probe/phase-b.log
```

Expected: ends with `Phase B: whole ladder returned, up to 4096` or `Phase B edge: NNNN`.

- [ ] **Step 2: Sanity-check**

Small read lengths must return `ok`, not `nack` — the TMP102 acknowledges. A `nack` at r=1 means the sensor is not responding and the phase is invalid.

```bash
grep '^write_read' /tmp/opencode/i2c-probe/results.csv | head -12
```

- [ ] **Step 3: Record the Phase B edge and any hang window in the working notes**

---

## Task 9: Run Phase C

- [ ] **Step 1: Set the frontier write lengths from the real Phase A edge**

Edit `write_lengths` in `phase_c` to `[1, 25%, 50%, 75%, just-under-Phase-A-edge]` using the measured Phase A edge. If Phase A found no edge, use `[1, 1024, 2048, 3072, 4095]`.

Set `read_hi` to the Phase B edge if one was found, otherwise leave it at 4096. It must be a length that does **not** return, or the bisection invariant is violated — the code checks this and skips, so a `no edge below it` message means `read_hi` needs raising or that write length genuinely has no read edge.

- [ ] **Step 2: Run the phase**

```bash
cd /tmp/opencode/i2c-probe && cargo run --release -- c 2>&1 | tee /tmp/opencode/i2c-probe/phase-c.log
```

- [ ] **Step 3: Determine the shape of the bound**

For each write length, the output prints `max r` and `sum`. Compare across rows:

- **`max r` roughly constant across write lengths** → rectangular bound, per-direction limits are independent.
- **`sum` roughly constant across write lengths** → diagonal bound, a shared `w + r` budget.

This determination drives §5 of the spec and decides whether the constant's comment must say the real constraint is a sum. Write the conclusion, and the evidence for it, into the working notes.

---

## Task 10: Restore and verify the TMP102

- [ ] **Step 1: Read the three registers back**

```
i2c_write_read address=0x48 data="0x01" count=2
i2c_write_read address=0x48 data="0x02" count=2
i2c_write_read address=0x48 data="0x03" count=2
```

- [ ] **Step 2: Compare against Task 6**

If all three match, record that the sweep left the sensor untouched.

If any differ, restore with `i2c_write address=0x48 data="0x0N,0xHH,0xLL"` for the affected register, re-read to confirm, and **record the discrepancy in the working notes and in the issue** — an unexpected register change means a probe wrote where it was not supposed to, which is itself a finding about the endpoint.

---

## Task 11: Set the constant

**Files:**
- Modify: `zephyr/drivers/i2c/pdg_i2c.c` (the comment above line 101, and the value on line 101)

- [ ] **Step 1: Apply the derivation rule**

From spec §5: the constant is the **minimum** of the Phase A edge, the Phase B edge, and the worst case on the Phase C frontier.

Do not round to a pretty number. 1013 is not pretty either; it is what was measured.

- [ ] **Step 2: Replace the comment and the value**

Replace the comment block currently spanning roughly lines 75–101 (the block ending `* large write is to construct.\n */`) and the `#define` with a comment carrying the four parts the SPI constant has. Template — fill every `<...>` from the measurement, and delete any section that the data does not support rather than guessing:

```c
/*
 * MEASURED, NOT DERIVED. This is the largest payload for which an
 * i2c/write or i2c/write-read call was observed to RETURN on real
 * hardware. It is not pico_de_gallo_internal::MAX_TRANSFER_SIZE, and it
 * is not a bus-level capability claim.
 *
 * MEASUREMENT (issue #146, <DATE>): board <SERIAL>, firmware 0.11.0,
 * schema 0.7, hardware revision 2, Linux host. Write lengths were probed
 * against an UNPOPULATED address (0x50), so the bus never ACKed; the
 * property measured is whether the call returns, not what a peripheral
 * does with the bytes. Read lengths were probed with a 1-byte pointer
 * write to a TMP102 at 0x48, which clocks out real data for arbitrary
 * read lengths.
 *
 *   i2c/write, largest returning payload:            <A>
 *   i2c/write-read, largest returning read (w=1):    <B>
 *   write+read frontier, worst case:                 <C>
 *
 * The bound is <RECTANGULAR: max write and max read are independent |
 * DIAGONAL: the real constraint is a sum, w + r <= <K>>. <If diagonal:
 * this driver checks the write total and the read length SEPARATELY, so
 * it approximates the real bound and does not enforce it. A caller near
 * the limit in both directions at once can pass both checks and still
 * fail on the wire; keep w + r <= <K>.>
 *
 * WHAT IS STILL UNKNOWN, stated plainly so nobody mistakes this for a
 * solved problem:
 *
 *   - <Unprobed lengths, especially any inside a hang window.>
 *   - One board, one firmware build, one host stack. Nothing here shows
 *     the number transfers to another revision or another host.
 *   - A lower constant reduces exposure to a known hang. It does NOT
 *     prove no other hang window exists below it.
 *
 * <KNOWN FIRMWARE HANG, only if one was found. Root cause is in crates/,
 * out of scope here. A <N>-byte i2c/<write|write-read> never returns and
 * wedges the dispatcher for every subsequent RPC. The 2 s watchdog does
 * not catch it, because the dedicated feeder task keeps feeding while a
 * handler blocks. Recovery in these tests was USBDEVFS_RESET on Linux;
 * on other hosts use cable reconnect, USB unbind/rebind, or a
 * power-cycle. system/reset-subscriptions cannot run while dispatch is
 * blocked.>
 *
 * This constant contains nothing outside Zephyr. The CLI, Rust, C,
 * Python and MCP surfaces can all still construct a transfer above it.
 *
 * FOLLOW-UP (do not just raise this number, and do not lower it by
 * guesswork either): derive the usable payload ceiling from the
 * worst-case request and response framing, express it as one generated
 * or shared contract rather than a constant duplicated per consumer, and
 * pin limit and limit+1 tests against it. That needs a wire-crate change
 * with schema and lockstep-release implications, which is out of scope
 * for this module.
 */
#define PDG_I2C_MAX_BUFFER <VALUE>U
```

- [ ] **Step 3: If the bound is diagonal, fix the read check's message**

The message at `zephyr/drivers/i2c/pdg_i2c.c:343` says the read "exceeds the %u-byte transfer limit", which implies the read limit is independent of the write total. If Phase C showed a shared budget, that wording is now misleading. Append to it:

```c
			LOG_ERR("I2C read message %u is %" PRIu32 " bytes, which exceeds the "
				"%u-byte transfer limit. Note this limit is checked separately "
				"from the write total; see PDG_I2C_MAX_BUFFER on why that "
				"approximates rather than enforces the measured bound. "
				"Returning -EMSGSIZE.",
				first + count - 1U, read->len, PDG_I2C_MAX_BUFFER);
```

- [ ] **Step 4: Verify line endings and commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
file zephyr/drivers/i2c/pdg_i2c.c
git add zephyr/drivers/i2c/pdg_i2c.c
git commit -m "$(cat <<'EOF'
fix(zephyr): Set PDG_I2C_MAX_BUFFER from measurement

PDG_I2C_MAX_BUFFER was 4096, inherited from
pico_de_gallo_internal::MAX_TRANSFER_SIZE. That is a packet-buffer and
argument bound, not a measured end-to-end ceiling. The sibling SPI
driver carries 1013 precisely because starting from 4096 was wrong
twice over.

Measured on hardware: <summary of A, B, C>.

Set the constant to <VALUE> and rewrite its comment to state what was
measured, on what hardware, and what remains unknown, matching how
PDG_SPI_MAX_BUFFER documents its own measurement.

Issue #146.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
)"
```

---

## Task 12: Zephyr documentation

**Files:**
- Modify: `zephyr/README.md` (the `-EMSGSIZE` row near line 713, and a new measured-limits paragraph mirroring the SPI one near line 547)
- Modify: `zephyr/CHANGELOG.md` (under Changed)

- [ ] **Step 1: Update the README limitation table row**

Locate the row:

```bash
grep -n "exceed 4096\|over 4096" zephyr/README.md
```

Update the byte count to the new constant. If the bound is diagonal, the row must also say the two checks are separate.

- [ ] **Step 2: Add the measured-limits paragraph**

Add an I2C paragraph alongside the SPI one at `zephyr/README.md:547`, saying what was measured, against what addresses, and that write probes never ACKed. Same plain-statement style.

- [ ] **Step 3: Add the CHANGELOG entry**

Under `### Changed` in the unreleased section of `zephyr/CHANGELOG.md`, with the full reasoning — matching the detail level of the SPI lowering entry, which explains why the old value was wrong rather than only what the new one is.

- [ ] **Step 4: Commit**

```bash
cd /home/balbi/workspace/pico-de-gallo
git add zephyr/README.md zephyr/CHANGELOG.md
git commit -m "$(cat <<'EOF'
docs(zephyr): Record the measured I2C transfer ceiling

Issue #146.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
)"
```

---

## Task 13: Book documentation

The AGENTS.md §15.1 Zephyr carve-out does **not** cover this: the book publishes claims about transfer limits that this measurement changes.

**Files:**
- Modify: `book/src/interfaces/batching.md:277-286`
- Modify: `book/src/interfaces/i2c.md`
- Modify: `book/src/appendix/troubleshooting.md:102-119`

- [ ] **Step 1: Fix the falsified claim in batching.md**

Line 278 currently reads:

```
| Demonstrated end-to-end payload | Shape-dependent and below 4096; no general ceiling is published |
```

That is no longer true for I2C. Replace with the measured I2C figures, keeping the SPI position unchanged, and keep the surrounding paragraph's point that `MAX_TRANSFER_SIZE` is a buffer bound rather than a guarantee.

- [ ] **Step 2: Add the I2C containment note**

In `book/src/interfaces/i2c.md`, add the I2C equivalent of the SPI note at `book/src/interfaces/spi.md:255-274`: the measured limit, what it does and does not guarantee, and the warning that the containment exists only in the Zephyr driver so CLI, Rust, C, Python and MCP callers are not protected by it.

- [ ] **Step 3: Update troubleshooting.md**

The section at lines 102-119 discusses the 4096 constant and the SPI 1013 containment. Add the I2C measurement alongside, including the hang recovery procedure if a hang was found.

- [ ] **Step 4: Build the book**

```bash
cd /home/balbi/workspace/pico-de-gallo && mdbook build book
```

Expected: no broken-link or missing-file warnings.

- [ ] **Step 5: Commit**

```bash
git add book/src/interfaces/batching.md book/src/interfaces/i2c.md book/src/appendix/troubleshooting.md
git commit -m "$(cat <<'EOF'
docs(repo): Publish the measured I2C transfer ceiling

The book stated that no general end-to-end ceiling was published for
either bus. That is no longer true for I2C.

Issue #146.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
)"
```

---

## Task 14: AGENTS.md regression row

**Only if a hang was found.** If every probe returned or errored cleanly, skip this task — there is no regression to record, and say so in the final report.

**Files:**
- Modify: `AGENTS.md` §13.17 table

- [ ] **Step 1: Append a row**

Same four columns as the existing rows (Date, Trigger, Symptom, Fix), in the same voice. This is the fourth dispatcher-wedge entry after the 2026-06-03 GPIO-wait, 2026-08-19 SPI-framing and 2026-08-26 zero-length-write rows, so the row should say so and name what is shared.

The Fix column must be honest that the Zephyr constant is **containment, not a fix**: the root defect is reachable from every host surface.

- [ ] **Step 2: Commit**

```bash
git add AGENTS.md
git commit -m "$(cat <<'EOF'
docs(repo): Record the I2C transfer-length dispatcher wedge

Fourth entry in the dispatcher-wedge class. Issue #146.

Assisted-by: GitHub Copilot:claude-opus-5
Co-authored-by: Copilot <223556219+Copilot@users.noreply.github.com>
EOF
)"
```

---

## Task 15: Verification

- [ ] **Step 1: Confirm no Rust changed**

```bash
cd /home/balbi/workspace/pico-de-gallo && git diff main --stat
```

Expected: only `zephyr/`, `book/`, and possibly `AGENTS.md`. If any `crates/**` file appears, it does not belong in this branch — the host-surface guard is explicitly out of scope per spec §2.

- [ ] **Step 2: Build the Zephyr module**

```bash
cd /home/balbi/workspace/pico-de-gallo
west build -p always -b native_sim/native/64 zephyr/tests/pdg_i2c_burst 2>&1 | tail -20
```

Expected: `Memory region ... Used Size` build summary, no errors. If `west` cannot find the workspace, source the environment from `~/zephyrproject` first and re-run.

- [ ] **Step 3: Run the one hardware-free suite**

```bash
cd /home/balbi/workspace/pico-de-gallo
~/zephyrproject/zephyr/scripts/twister -T zephyr/tests/pdg_fake -p native_sim/native/64 --inline-logs 2>&1 | tail -25
```

Expected: the `pdg_fake/i2c` suite passes. This is the only Zephyr test that actually executes rather than merely linking.

- [ ] **Step 4: Build the M5 targets**

```bash
cd /home/balbi/workspace/pico-de-gallo
west build -p always -b native_sim/native/64 zephyr/tests/pdg_mfd_m5/acceptance 2>&1 | tail -10
```

Expected: builds and links. These carry the `_Static_assert`s that pin the FFI-to-wire enum correspondence.

- [ ] **Step 5: State plainly what is not verified**

`pdg_i2c_burst` is board-attached; the build above only links it. If the new constant is materially lower than 4096, run it against the board and report the result. If that is not possible, say so — a green build must not be allowed to imply a behavioural result.

---

## Task 16: Publish the raw data

- [ ] **Step 1: Post results to the issue**

```bash
cd /home/balbi/workspace/pico-de-gallo
gh issue comment 146 --repo OpenDevicePartnership/pico-de-gallo --body-file /tmp/opencode/i2c-probe/summary.md
```

Write `summary.md` first, containing: the board/firmware/schema identification, the three phase results as tables in the same shape as the SPI table in the issue, the frontier shape conclusion with its evidence, every unprobed length, and the full `results.csv` contents in a fenced block. The raw data must outlive this session.

- [ ] **Step 2: Final branch review**

```bash
git log --oneline main..issue-146
git diff main --stat
```

Confirm every commit carries the `Assisted-by:` and `Co-authored-by: Copilot` trailers and that none carries `Signed-off-by:`.

---

## Self-Review Notes

**Spec coverage.** §3 harness → Tasks 1–5. §4 Phase A/B/C → Tasks 7, 8, 9. §4 TMP102 state → Tasks 6, 10. §5 derivation rule → Task 11 Step 1. §6 deliverables → Tasks 11–14. §7 verification → Task 15. §8 reset risk → Task 2 Step 4, deliberately promoted to an early standalone gate with an explicit stop condition, because the whole sweep design depends on it.

**Known gaps, stated rather than hidden.** The `<...>` placeholders in Tasks 11–14 are measurement outputs that cannot exist before Tasks 7–9 run; they are not planning placeholders. Task 14 is conditional by design. Task 9 Step 1 requires editing the harness with Phase A/B results, which is genuine sequencing, not vagueness.

**Type consistency.** `Outcome`, `Kind`, `probe`, `alive`, `recover`, `resolve_node`, `reset`, `wait_for_node`, `csv_line`, `Recorder::open`, `Recorder::record`, `bisect`, `bisect_async`, `measured`, `ladder` are each defined once and used with consistent signatures across Tasks 1–5.

## Outcome (2026-09-01)

The harness was scaffolded, tested and used as planned in Tasks 1–8,
with two material implementation deviations. First,
`PicoDeGallo::try_new_with_serial_number` is synchronous and blocking;
opening the connection inside the timed future could prevent the probe
timeout from ever firing. Connection opening therefore had to run in
`spawn_blocking`. Second, a `Hang` was changed to stop the run outright
rather than trigger automatic recovery, because `USBDEVFS_RESET` had
not been validated on this host. The one-RPC-per-probe, fresh-connection,
5 s timeout, pre-probe liveness check, immediate CSV recording and exact
returned-length checks otherwise ran as intended.

Task 7 completed its planned Phase A ladder. Every explicitly probed
write length—1, 64, 256, 512, 1013, 1015, 1024, 2048, 3072 and
4096—returned `NoAcknowledge` in 1–4 ms, so there was no failing bracket
to bisect. Review identified an important limit on that evidence: the
address NACK stopped the transaction before payload bytes were clocked.
The result verifies request framing, delivery, decode and transaction
initiation through the wire maximum, not a 4096-byte write to an
acknowledging target.

Task 8 completed as written and was extended with an addendum. With a
one-byte pointer write to the TMP102, reads through 1014 bytes returned
their exact requested lengths. Reads at 1015, 1016, 1024, 2048, 3072
and 4096 failed with the same host-side
`Postcard(DeserializeUnexpectedEnd)` response-decode error. Two
independent bisections reproduced the 1014/1015 edge.

Task 9 could not be completed as written. Its Phase C procedure assumed
the TMP102 would accept arbitrary write lengths, analogous to its
arbitrary repeated reads. In fact, its one-byte pointer and two-byte
registers limit meaningful writes; `w = 64, 256, 512, 1000` with
`r = 1` all received a device NACK reported as `I2C bus error` in
2–3 ms. With no other target on the bus, only the `w = 1` frontier row
was measured. Rectangular versus diagonal behaviour therefore remains
an inference from request/response mechanism rather than an observed
frontier.

Task 10 completed: CONFIG `0x60A0`, TLOW `0x4B00` and THIGH `0x5000`
were unchanged after the sweep.

Task 11's instruction to derive one `PDG_I2C_MAX_BUFFER` as the minimum
across all phases was superseded. One number could not honestly express
the roughly fourfold directional difference, especially when the write
result is a request-framing bound rather than a measured bus payload
ceiling. The implementation instead uses `PDG_I2C_MAX_READ = 1014U`
and `PDG_I2C_MAX_WRITE = 4096U`, with those qualifications documented.
Tasks 12 and 13 were completed for the split limits. Task 14 was
skipped exactly as its condition required: no hang was found, so no
AGENTS.md regression row was warranted. Consequently the planned reset
path was never exercised and remains unvalidated.

Task 15 completed with the available scope: `pdg_i2c_burst` and
`pdg_mfd_m5/acceptance` built and linked for `native_sim/native/64`,
`pdg_fake/i2c` executed and passed 2/2, and `mdbook build book` was
clean. The first two are `build_only`, so they do not behaviourally
verify either new bound against attached hardware. Task 16's raw-data
publication is represented by the issue-comment summary generated from
`/tmp/opencode/i2c-probe/results.csv`.
