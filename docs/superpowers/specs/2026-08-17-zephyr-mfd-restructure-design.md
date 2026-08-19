# Zephyr MFD restructure — design

Date: 2026-08-17
Sub-project: **SP1 of N** — MFD parent, GPIO controller, `cs-gpios`, i2c/spi migration
Related: [#98](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/98) (upstreaming tracker), [#104](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/104) (SPI chip-select contract, landed)
Branch: `zephyr`
Status: approved

---

## 1. Problem

The Zephyr module presents the Pico de Gallo as **flat, unrelated devicetree
nodes** — `pdg_i2c0` and `pdg_spi0` are siblings at the root
(`zephyr/boards/shields/pico_de_gallo/pico_de_gallo.overlay:1-22`), bound to the
same physical board only by a matching `serial-number` string resolved at
runtime. It is a convention, not a devicetree relationship.

Two consequences follow.

### 1.1 Chip-select uses a bespoke property

#104 replaced a `reg`-as-GPIO-index overload with an explicit
`cs-gpio-indices` array on the controller. That fixed the silent-corruption bug,
but the property is **not** how Zephyr expresses chip-select. Every other SPI
controller uses:

```dts
cs-gpios = <&gpio0 1 GPIO_ACTIVE_LOW>;
```

A bespoke property means DT authors cannot transfer knowledge from any other
board, and an upstreaming reviewer will ask why this controller is special.

### 1.2 The module exposes no GPIO API at all

There is no `zephyr/drivers/gpio/` (verified: `drivers/Kconfig` sources only
`i2c/Kconfig` and `spi/Kconfig`). The firmware exposes four user GPIOs and the
host library drives them, but a Zephyr application cannot reach them through
`gpio_pin_set_dt()`. This is also *why* §1.1 needs a bespoke property: there is
no GPIO controller for `cs-gpios` to point at.

### 1.3 What is NOT wrong — a corrected claim

An earlier framing of this work asserted that the two drivers holding private
`k_mutex`es over one shared `PicoDeGallo` handle was a **correctness** defect.
**That was overstated.** Verified:

- `postcard-rpc-0.12.1/src/host_client/mod.rs:176-184` — `HostClient` is `Clone`,
  holds `Arc<HostContext>` plus an `mpsc::Sender`, and `send_resp` takes `&self`
  with an atomic `seq.fetch_add` (`:338-346`). Responses route by sequence
  number. Concurrent RPCs from multiple threads are **safe by design**.
- `crates/pico-de-gallo-firmware/src/context.rs:67-69` — "postcard-rpc dispatches
  handlers serially (one at a time)". Two handlers cannot interleave in firmware.
- I²C and SPI have independent device-side configuration state, and each
  driver's own mutex already covers its `set_config`+transfer pair.

There is no data race and no reachable interleaving hazard today. The MFD's
justification is **ownership topology**, not serialization. See D6.

---

## 2. Decisions

| # | Decision | Rationale |
| --- | --- | --- |
| D1 | Restructure as a Zephyr MFD: one parent node owning the USB handle, with `gpio`, `i2c` and `spi` children. | Makes the board a devicetree relationship rather than a string convention. Precedent: `nordic,npm1300` (parent+children), `nxp,sc18im704` (serial-attached bridge with multiple controller children). |
| D2 | Add a real `pdg_gpio` controller driver. | Prerequisite for `cs-gpios`, and independently valuable — Zephyr applications currently cannot use the board's GPIOs at all. |
| D3 | Adopt standard `cs-gpios`; **delete** `cs-gpio-indices`. | The module is not upstreamed (#98 open), so breaking the DT contract is free now and expensive later. |
| D4 | Let `spi_context_cs_control()` toggle CS idiomatically. **Zephyr stops using `spi/batch`.** | Maximum upstreamability, zero novelty. Costs the atomic CS interval — see §5. |
| D5 | Six mandatory `gpio_driver_api` callbacks only; interrupts deferred. | `pin_interrupt_configure` and `manage_callback` are documented-optional (`-ENOSYS`). Interrupt support needs an event-pump thread and interacts with the orphaned-subscription hazard; it earns its own sub-project. |
| D6 | **No parent lock.** | Nothing needs it (§1.3), and adding one would create a deadlock hazard (§4.2). The parent owns the handle; that was always the stronger argument. |
| D7 | No wire-protocol change. | Every endpoint SP1 needs already exists. No schema bump, no lockstep release, no firmware work. |
| D8 | SP1 covers gpio/i2c/spi only. uart, adc, pwm and 1-wire are later sub-projects. | Each is an independent child of the same parent and structurally similar to the others; they do not need to be designed together. |

---

## 3. Devicetree topology

The shield overlay becomes:

```dts
/ {
    pdg0: pico-de-gallo {
        compatible = "odp,pico-de-gallo";
        serial-number = "5256657D8A5D7F03";   /* optional */
        status = "disabled";

        pdg_gpio0: gpio {
            compatible = "odp,pico-de-gallo-gpio";
            gpio-controller;
            #gpio-cells = <2>;
            ngpios = <4>;
            status = "disabled";
        };

        pdg_i2c0: i2c {
            compatible = "odp,pico-de-gallo-i2c";
            #address-cells = <1>;
            #size-cells = <0>;
            clock-frequency = <400000>;
            status = "disabled";
        };

        pdg_spi0: spi {
            compatible = "odp,pico-de-gallo-spi";
            #address-cells = <1>;
            #size-cells = <0>;
            status = "disabled";
        };
    };
};
```

An application overlay then writes ordinary Zephyr:

```dts
&pdg_spi0 {
    status = "okay";
    cs-gpios = <&pdg_gpio0 2 GPIO_ACTIVE_LOW>;

    nor: nor@0 {
        compatible = "jedec,spi-nor";
        reg = <0>;
        spi-max-frequency = <10000000>;
    };
};
```

`reg = <0>` recovers its correct Zephyr meaning — "first slave" — and the
firmware GPIO index appears exactly where a Zephyr user expects it.

**Binding note.** Children are themselves *controllers*, not devices on a private
bus, so they follow the `nordic,npm1300-gpio` shape (`include:
gpio-controller.yaml`, no `on-bus:`) rather than the `nxp,sc18is606` shape which
declares a private bus type.

---

## 4. Components and interfaces

### 4.1 File inventory

| Action | Path | Responsibility |
| --- | --- | --- |
| Create | `zephyr/drivers/mfd/pdg_mfd.c` | Opens the handle at init; exposes `pdg_mfd_ctx(const struct device *)`. `DEVICE_DT_INST_DEFINE` with **`NULL`** API. |
| Create | `zephyr/drivers/mfd/pdg_mfd.h` | The parent's C API for children. |
| Create | `zephyr/drivers/mfd/{CMakeLists.txt,Kconfig}` | |
| Create | `zephyr/drivers/gpio/pdg_gpio.c` | Six mandatory callbacks. |
| Create | `zephyr/drivers/gpio/pdg_gpio_bottom.{c,h}` | FFI bottom half, mirroring the i2c/spi split. |
| Create | `zephyr/drivers/gpio/{CMakeLists.txt,Kconfig}` | |
| Create | `zephyr/dts/bindings/mfd/odp,pico-de-gallo.yaml` | |
| Create | `zephyr/dts/bindings/gpio/odp,pico-de-gallo-gpio.yaml` | |
| Modify | `zephyr/dts/bindings/spi/odp,pico-de-gallo-spi.yaml` | **Delete `cs-gpio-indices`**; document the `cs-gpios` contract and the atomicity trade. |
| Modify | `zephyr/dts/bindings/i2c/odp,pico-de-gallo-i2c.yaml` | Child-of-parent; `serial-number` moves to the parent. |
| Modify | `zephyr/drivers/spi/pdg_spi.c` | Remove all CS logic and the index mapping; `spi/batch` → `spi/transfer`; call `spi_context_cs_control`. |
| Modify | `zephyr/drivers/spi/pdg_spi_bottom.{c,h}` | Drop `batch`/`num_gpios`; add transfer. |
| Modify | `zephyr/drivers/i2c/pdg_i2c.c` | Handle now comes from the parent. |
| Modify | `zephyr/drivers/{CMakeLists.txt,Kconfig}` | Add `mfd`, `gpio`. |
| Modify | `zephyr/boards/shields/pico_de_gallo/pico_de_gallo.overlay` | Nested topology. |
| Modify | 3 × `zephyr/samples/*/app.overlay` | `cs-gpios`; drop `cs-gpio-indices`. |
| Modify | `zephyr/README.md`, `book/src/interfaces/spi.md`, `book/src/interfaces/gpio.md` | §15.1 parity. |

### 4.2 Why there is no parent lock

With `cs-gpios`, one Zephyr SPI transaction spans four RPCs:

```
gpio/put (assert)  →  spi/set-config  →  spi/transfer  →  gpio/put (deassert)
     ^ GPIO child            ^ SPI child                        ^ GPIO child
```

The outer two are issued **by the SPI child, through the GPIO child**. A parent
lock held across the SPI transaction and also taken by the GPIO child therefore
**deadlocks**. Upstream `sc18is606` never meets this because its SPI driver uses
an internal `ss_idx` and never calls its GPIO sibling; adopting `cs-gpios` makes
us the first to nest them.

Combined with §1.3 — no data race, no firmware interleaving — a parent lock would
be machinery that solves nothing while introducing a real hazard. **D6: do not
build it.** If a future child needs an indivisible multi-RPC sequence, it must be
designed with this nesting in mind.

### 4.3 Handle ownership

The parent calls `pdg_registry_open(serial)` once at init and stores the
`PicoDeGallo *`. Children fetch it via `pdg_mfd_ctx(config->mfd)`.

`gallo_registry.c` keeps its serial-keyed refcounting — it still serves the
two-board case, one parent node per board — but the parent becomes its **only**
caller inside the module. The mixed-selector guard (`gallo_registry.c:160-171`)
becomes unreachable in practice, because there is now exactly one open call site
per board rather than one per driver.

### 4.4 GPIO driver surface

Mandatory six, verified against `include/zephyr/drivers/gpio.h:809-839` by
checking which callbacks the `z_impl_*` wrappers dispatch without a NULL guard:

`pin_configure`, `port_get_raw`, `port_set_masked_raw`, `port_set_bits_raw`,
`port_clear_bits_raw`, `port_toggle_bits`.

`pin_interrupt_configure`, `manage_callback` and `get_pending_int` are
NULL-checked and yield `-ENOSYS` — left unimplemented per D5.

Structural requirements:

- `struct gpio_driver_config common;` **must be the first member** of the config
  struct and `struct gpio_driver_data common;` the first of the data struct — the
  API layer casts `port->config`/`port->data` directly.
- Initialise with `GPIO_COMMON_CONFIG_FROM_DT_INST(inst)`, which derives
  `port_pin_mask` from `ngpios`.
- `gpio.h:933` asserts the pin is within `port_pin_mask`, so **`ngpios` must match
  the firmware-reported GPIO count** (4 today). Mismatch is an assert, not a
  graceful error.

`port_toggle_bits` may be implemented as get-then-set, as `gpio_npm13xx.c` does.

### 4.5 Flag mapping, and what is rejected

`pin_configure` receives a `gpio_flags_t` bitfield that is richer than the
firmware's two-axis model (`GpioDirection` × `GpioPull`). The mapping is part of
the contract, not an implementation detail:

| Zephyr flag | Firmware | Notes |
| --- | --- | --- |
| `GPIO_INPUT` | `direction: Input` | |
| `GPIO_OUTPUT` | `direction: Output` | |
| `GPIO_PULL_UP` / `GPIO_PULL_DOWN` | `pull: Up` / `Down` | Neither set → `pull: None` |
| `GPIO_OUTPUT_INIT_HIGH` / `_LOW` | `set-config` then `gpio/put` | Two RPCs; the firmware has no combined form |
| `GPIO_DISCONNECTED` (neither IN nor OUT) | — | **`-ENOTSUP`** |
| `GPIO_SINGLE_ENDED` (open-drain / open-source) | — | **`-ENOTSUP`** — the firmware drives push-pull only |
| `GPIO_LINE_OPEN_DRAIN` | — | **`-ENOTSUP`** |

`GPIO_ACTIVE_LOW` needs no handling: the subsystem resolves logical-to-raw
before calling `port_set_bits_raw`/`port_clear_bits_raw`, which is precisely why
`cs-gpios = <&pdg_gpio0 2 GPIO_ACTIVE_LOW>` yields an active-low chip select
without the driver knowing.

Unsupported flags must be **rejected**, never silently ignored — silently
accepting a flag the hardware cannot honour is the #104 failure mode in a new
costume.

### 4.6 Init ordering

`parent < gpio < spi`; i2c needs only the parent. SPI must come last because
`spi_context_cs_configure_all()` calls `device_is_ready(cs_gpio->port)` and
`gpio_pin_configure_dt()` at init.

Expressed the way upstream does — Kconfig priority defaults plus a runtime
`if (!device_is_ready(config->mfd)) return -ENODEV;` in every child. Nothing in
the build system enforces the ordering; the guard is the real protection.

### 4.7 Blocking-over-USB contract

Every GPIO operation is a ~1 ms USB round-trip. This is explicitly sanctioned:
`gpio.h` documents `-EIO` ("when accessing an external GPIO chip") on nearly
every function and `-EWOULDBLOCK` ("if operation would block") on 22 of them, and
marks only `gpio_pin_interrupt_configure` as `@isr_ok`.

Required idiom, as in `gpio_npm13xx.c` and `gpio_pca95xx.c`:

```c
if (k_is_in_isr()) {
    return -EWOULDBLOCK;
}
```

at the top of every callback, plus `-EIO` on transport failure. Both documented
in the binding, since this differs from on-chip GPIO.

---

## 5. Consequences of D4 — stated plainly

Dropping `spi/batch` is a real loss and must be documented, not buried:

| | Before (batch) | After (`cs-gpios`) |
| --- | --- | --- |
| Round-trips per `spi_transceive` | 1 | 3+ |
| CS held atomically in firmware | yes | **no** |
| Host dies mid-transfer | firmware deasserts CS | **CS stays asserted** |
| `cs.setup_ns` / `hold_ns` | firmware `DelayNs`, faithful | `k_busy_wait` of ns between ms round-trips — meaningless |
| `spi/batch` endpoint | used by Zephyr | unused by Zephyr |

Accepted deliberately in exchange for a standard binding and upstreamability. The
binding must say so, so a user choosing this bridge for a
crash-sensitive peripheral is not surprised.

`spi/batch` remains fully supported for CLI, Rust, C, Python and MCP consumers —
this is a Zephyr-module decision only.

## 6. What D4 buys back — pin-mode coherence

An earlier draft listed "a second notion of pin mode in `gpio_driver_data` that
can drift from the firmware's `pin_modes`" as the main hazard of adding a GPIO
controller — a #104 recurrence one layer up.

**D4 eliminates it structurally.** The Zephyr SPI child no longer has a
chip-select pin concept at all: `spi_context_cs_configure_all()` issues
`gpio/set-config{direction: output}` through our GPIO driver, and every CS edge
is a `gpio/put`. The GPIO child is the **sole writer** of pin mode, so the
firmware's view and Zephyr's cannot diverge.

The driver must therefore **not cache pin state** — every query goes to the
firmware. Caching would reintroduce exactly the divergence D4 removes.

---

## 7. Testing

### 7.1 The fixture that actually exists

Board `5256657D8A5D7F03` (hw rev 2, M2 firmware):

- **SPI MOSI and MISO are shorted together** (header pins 6 and 5). The bus is a
  hardware loopback.
- **No SPI NOR is attached.** `spi_nor_id` therefore cannot pass — it would read
  its own command bytes back instead of a JEDEC ID.
- A jumper is fitted between header pins 13 and 14 — firmware GPIO indices **2
  and 3** — left in place from #104 acceptance.

This is more capable than a NOR would be, not less: a loopback proves the data
path is **bit-exact**, which a peripheral-specific probe does not.

### 7.2 Primary vehicle: the upstream Zephyr loopback suite

`zephyr/tests/drivers/spi/spi_loopback` is Zephyr's canonical SPI driver test and
is designed for exactly this fixture. Adopting it is worth more than any bespoke
sample, because "passes the standard SPI driver test suite" is precisely the
evidence an upstreaming reviewer (#98) will look for.

It needs only a board overlay, following the shape of
`boards/nrf52840dk_nrf52840.overlay`:

```dts
&pdg_spi0 {
    status = "okay";
    cs-gpios = <&pdg_gpio0 2 GPIO_ACTIVE_LOW>;

    slow@0 {
        compatible = "test-spi-loopback-slow";
        reg = <0>;
        spi-max-frequency = <500000>;
    };

    fast@0 {
        compatible = "test-spi-loopback-fast";
        reg = <0>;
        spi-max-frequency = <4000000>;
    };
};
```

The suite additionally supports an **independent chip-select witness** via
`cs-loopback-gpios` on `zephyr,user` — the same technique #104 acceptance built
by hand, but standardised. With the existing 13↔14 jumper:

```dts
/ {
    zephyr,user {
        cs-loopback-gpios = <&pdg_gpio0 3 (GPIO_ACTIVE_LOW | GPIO_PULL_UP)>;
    };
};
```

CS on index 2, witness on index 3, jumpered. That verifies the CS **edges**
independently of the data path — the two halves of the driver characterised
separately, with hardware already on the bench.

Note `GPIO_PULL_UP`, not pull-down: §7.4.

### 7.3 Expected loopback semantics — verify, do not assume

A MOSI↔MISO short should echo each byte exactly, but whether the sampled bit is
the current or previous one depends on CPOL/CPHA. **Confirm empirically with a
known pattern before relying on it**; a mode mismatch presents as a one-bit shift
rather than an obvious failure. `spi_loopback` exercises multiple modes, which is
part of its value.

### 7.4 The RP2350 pull-down trap still applies

From #104 (plan §8.11), measured on this board: an internal pull-**down** can
*hold* a low node low but cannot *pull down* a node that is already high, and a
floating pad drifts high within seconds. Pull-**ups** work normally.

Any test that configures a pull-down and expects LOW **without first forcing the
node low** is invalid — it will pass against broken firmware. Either pre-drive
the node low and release to a pull-down, or use a pull-up and invert the
expectation as `cs-loopback-gpios` does above.

### 7.5 Samples

All four samples must still **build** for `native_sim`. End-to-end runs are
another matter, and the honest position is that none is currently a viable
acceptance gate:

| Sample | Status |
| --- | --- |
| `spi_bridge` | **Cannot link** — `issi,is31fl3743b` exists in neither Zephyr nor this repo (drafted issue, pre-existing) |
| `combined_i2c_spi_bridge` | **Cannot link** — same cause |
| `spi_nor_id` | Links, but **no NOR is attached** — cannot pass at runtime |
| `i2c_bridge` | Needs a TMP117 at 0x48; last confirmed working, present state unverified |

The samples therefore verify **compilation and devicetree correctness only**.
Behavioural acceptance rests on §7.2.

### 7.6 GPIO acceptance, independent of SPI

`gpio_pin_configure_dt` / `gpio_pin_set_dt` / `gpio_pin_get_dt` across the 13↔14
jumper, driving from each end in turn — the same bidirectional validation used in
#104 to prove the jumper itself before trusting any measurement. Also confirm
`-ENOTSUP` for the rejected flags in §4.5 and `-EWOULDBLOCK` from ISR context.

## 8. Non-goals

- GPIO interrupts and callbacks (D5).
- A parent lock (D6).
- uart, adc, pwm, 1-wire children (D8).
- Any wire-protocol or firmware change (D7).
- Claiming `p.PIN_5` — that remains #99.
- `[package].version` bumps — the maintainer owns releases.

## 9. Risks

| Risk | Mitigation |
| --- | --- |
| `ngpios` disagreeing with firmware `num_gpios` asserts rather than erroring | Document in the binding; consider a runtime check in the GPIO driver's init against the parent's reported count. |
| Losing atomic CS surprises a user | §5 stated in the binding and the book. |
| The `k_is_in_isr()` guard makes pdg GPIO unusable from an ISR | Inherent to a USB-attached bridge; documented, and matches every other off-chip GPIO driver. |
| Init-order regression is silent until a CS toggle fails | `device_is_ready` guards in every child; SPI init fails loudly if the GPIO device is not ready. |
| No sample is a viable end-to-end gate (§7.5) | Acceptance rests on the upstream `spi_loopback` suite instead, which is stronger evidence anyway. |
| Loopback echo semantics may be mode-dependent | Verify empirically before relying on it (§7.3); a mismatch shows as a bit shift, not a clean failure. |

---

## 10. Amendments — decided after M3

Two decisions taken by the maintainer after M3 exposed a gap in §7.2. Both
expand M4 beyond §2's original decision table; they are recorded here so an
implementer does not read a spec that contradicts its brief.

### D9 — M4 supports `SPI_HOLD_ON_CS`

`pdg_spi.c:241-244` currently rejects it with `-ENOTSUP`. That rejection exists
only because the **batch** design could not hold chip-select across separate
calls; once CS is an ordinary GPIO driven by `spi_context`, the constraint
disappears.

`spi_context.h:396-401` honours the flag by *skipping* the deassert:

```c
if (!force_off && ctx->config->operation & SPI_HOLD_ON_CS) {
        return;
}
```

Two reasons to support it:

1. **It is the only interrupt-free way to verify chip-select edges** (§7.2 as
   amended, plan §10.2). Transfer with the flag → poll the witness → `spi_release()`
   → poll again. Deterministic, single-threaded, no callbacks.
2. It is a genuine capability gain — `SPI_HOLD_ON_CS` is how Zephyr expresses a
   multi-transfer transaction, and every `jedec,spi-nor`-class driver may use it.

`spi_release()` must therefore work: it reaches
`spi_context_unlock_unconditionally()`, which force-deasserts via
`_spi_context_cs_control(ctx, false, true)`.

### D10 — A failed chip-select edge must fail closed

`spi_context_cs_control()` returns **`void`** and discards both
`gpio_pin_set_dt()` results (`spi_context.h:390-418`). Upstream can afford that
because CS is a register write that cannot meaningfully fail. **Here every edge
is a fallible USB round-trip** that can return `-EIO`, `-EBUSY` (a monitored pin)
or `-EWOULDBLOCK` (ISR context).

Left unhandled, the two failure modes are:

- assert fails → **the transfer proceeds with CS unasserted**, clocking data at a
  peripheral that is not selected;
- deassert fails → **success is reported with CS still asserted**, leaving the
  peripheral selected indefinitely.

Both are silent-wrong-behaviour of exactly the class #104 was about, and neither
is detectable by a loopback.

**M4 must not call `spi_context_cs_control()` blindly.** The assert result must
be checked and a failure must abort the transfer with the underlying errno; a
failed deassert must be reported rather than swallowed. The implementation shape
is M4's to choose — driving the pin directly with `gpio_pin_set_dt()` and
checking the result is the obvious candidate — but the property is mandatory.

This is a deliberate, documented divergence from the stock driver pattern, and it
should be explained in a source comment so a reader does not "fix" it back to the
idiomatic form.

### Consequence for §5

§5's table said `spi/batch` becomes unused by Zephyr and the atomic CS interval is
lost. That stands. But D9 partially compensates: `SPI_HOLD_ON_CS` lets a caller
hold CS across several transfers deliberately, which the batch could never
express — the batch was atomic but fixed-scope. The loss is atomicity against a
host crash, not the ability to span operations.