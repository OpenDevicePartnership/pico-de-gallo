# Zephyr PWM driver for Pico de Gallo — design

Date: 2026-09-14
Issue: [#155](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/155)
Related: [#192](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/192) (firmware divider panic, filed from this work)

## 1. Goal

Add a Zephyr `pwm` driver as a child of the `odp,pico-de-gallo` MFD parent,
implementing `pwm_set_cycles()` and `pwm_get_cycles_per_sec()` on top of the
existing C FFI (`gallo_pwm_*`).

The hard part is not the plumbing. It is that Zephyr and Pico de Gallo model
PWM differently, and the mapping between them is lossy in ways that are easy
to get wrong silently:

- Zephyr expresses period and pulse in **clock cycles**, against a
  cycles-per-second the driver declares.
- Pico de Gallo expresses configuration as a **frequency** (`pwm/set-config`)
  plus a **raw compare value** (`pwm/set-duty-cycle`), whose usable range is
  the `max_duty` reported by `pwm/get-duty-cycle`.

## 2. Corrections to the issue's premises

Two statements in #155 are wrong and were verified against the source before
this design was settled.

**PWM is not `hw-rev2`-gated.** `crates/pico-de-gallo-firmware/src/handlers/info.rs:30-40`
sets `Capabilities::PWM` on both revisions, and no PWM code path carries a
`#[cfg(feature = "hw-rev2")]` — unlike `adc`, `onewire`, and `uart`, whose
`Context` fields are genuinely gated. The driver still probes the capability
bit, because that is the correct contract and costs little, but the gate will
never fire on a shipped image. Relatedly, `hw-rev2` is the default feature
today (`crates/pico-de-gallo-firmware/Cargo.toml:14`), not `hw-rev1` as the
issue states.

**The firmware PWM code is not in `main.rs`.** It is in
`crates/pico-de-gallo-firmware/src/handlers/pwm.rs`.

## 3. Facts the design depends on

Established by reading the firmware, the wire crate, and embassy-rp 0.10.0.

- `SYS_CLK_HZ = 150_000_000`, `crates/pico-de-gallo-firmware/src/context.rs:43`.
  It is `pub(crate)`: not on the wire, not in `pico_de_gallo.h`.
- Channels 0–3 map to GPIO12–15. Channels 0 and 1 share RP2350 slice 6;
  channels 2 and 3 share slice 7. `pwm/enable` and `pwm/disable` act on the
  **whole slice**.
- `max_duty == top + 1`, not `top`. The doc comment on
  `PwmDutyCycleInfo::max_duty` in `pico-de-gallo-internal` says `top` and is
  wrong; `pico-de-gallo-lib` says `top + 1` and is right.
- Legal compare range is `0..=top+1`, i.e. `0..=max_duty`. A compare of 0 is
  always-low; `top + 1` is always-high. An over-range duty is silently clamped
  by the firmware, never rejected — `PwmError::InvalidDutyCycle` is dead on
  the wire.
- `pwm/set-config` rescales both channels' compares proportionally with
  **truncating** integer division, so duty ratios drift downward on every
  reconfiguration.
- Boot default is `enable = true`, `divider = 1`, `top = 0xFFFF`, compares 0 —
  a running ~2288.8 Hz carrier at 0% duty.
- In phase-correct mode the period is `2 × top` counts while `max_duty` still
  reports `top + 1`.
- Dividers above 255.9375 panic inside embassy-rp (`src/pwm.rs:250-253`),
  while the firmware's search runs to 4095. This is #192.
- All nine PWM `Status` codes are already mapped in
  `zephyr/drivers/common/common.c`. That file needs no change.

## 4. Cycle domain

`pwm_get_cycles_per_sec()` returns a hardcoded **150 000 000**, defined in
`pdg_pwm_bottom.h` with a comment naming `firmware/src/context.rs:43` as the
source of truth.

The value reported is the PWM **source clock**, not the counter rate. This is
the decision the rest of the design rests on. The counter rate is
`150e6 / divider`, which changes with every frequency change and can differ
between the two slices — so it cannot serve as the stable per-device constant
Zephyr's API requires. Reporting the source clock instead makes `period_cycles`
a divider-independent quantity, and the divider then never needs to be known
by the driver at all. That matters because the divider is not on the wire.

Hardcoding was chosen over probing the value at init. Probing is possible —
configuring a channel to 10 kHz yields `divider == 1` exactly, making
`frequency_hz × max_duty` an exact reading of the source clock — but it costs
two RPCs and reconfigures a slice as an init side effect. The coupling to
`context.rs` is accepted and documented instead.

## 5. Conversion

All arithmetic in `uint64_t`.

```
frequency_hz = ceil(150e6 / period_cycles)
             = (150e6 + period_cycles - 1) / period_cycles

compare      = (pulse_cycles * max_duty + period_cycles / 2) / period_cycles
             clamped to max_duty
```

The compare expression is round-half-up, not truncation: the half-divisor bias
term is added before the division. Truncating here would bias every duty cycle
downward, which compounds with the firmware's own truncating rescale.

**The ceiling is load-bearing and is the rounding bug the issue predicts.**
Flooring the frequency lengthens the period, which forces the firmware to
select a *larger* divider; near the bottom of the range that pushes it past
255 and into the #192 panic. Concretely, at the maximum supported
`period_cycles` of 16 711 680, flooring gives 8 Hz, which requires a divider
of 287. Ceiling gives 9 Hz, which needs only 255. Ceiling also matches the
firmware's own behaviour: because it floors `top`, its achieved frequency is
always greater than or equal to the requested one, never below.

`max_duty` is read back from `pwm/get-duty-cycle` after configuring, rather
than computed, so the driver never has to model the firmware's divider search.

## 6. Local validation, before any RPC

| Condition | Return | Why |
|---|---|---|
| `period_cycles < 2` | `-EINVAL` | Implies above the 75 MHz ceiling; the firmware requires `top >= 1`. |
| `period_cycles > 255 * 65536` (16 711 680) | `-ENOTSUP` | Panic guard for #192. Any larger period forces a divider above 255. |
| `flags & PWM_POLARITY_INVERTED` | `-ENOTSUP` | Per #155. Deliberately not emulated by inverting the duty cycle, which would change the idle level rather than the polarity. |
| `channel >= 4` | `-EINVAL` | Neither the FFI nor `pico-de-gallo-lib` range-checks the channel, so an out-of-range value would otherwise make a full USB round trip to be refused. |
| Sibling channel holds a different period | `-EINVAL` | See §7. |

The bound `255 * 65536` is conservative by design. The exact limit is
`255 * 65537 = 16 711 935`; rounding down to a power-of-two multiple gives a
bound that is obviously safe by inspection and leaves margin.

`phase_correct` is always sent as `false`. Zephyr's PWM API has no
corresponding concept, and in phase-correct mode the period becomes `2 × top`
counts while `max_duty` continues to report `top + 1` — so a driver that
equated `period_cycles` with `max_duty` would be wrong by roughly 2×. The
binding documents that phase-correct is not reachable through Zephyr.

## 7. Slice sharing

Two Zephyr channels on the same slice cannot hold independent periods, and
`pwm/enable`/`pwm/disable` are slice-wide. Two consequences, both handled
explicitly rather than left to be discovered at runtime.

**Conflicting periods are refused.** The driver tracks each channel's last
requested period. If a channel requests a period differing from one its
sibling is actively using, it returns `-EINVAL` and logs both channel numbers
and both periods. The alternative — reconfiguring the slice — would silently
change the sibling's period behind its owner's back, and additionally drift
its duty ratio through the firmware's truncating rescale. Refusing surfaces
the constraint at the call that violates it.

**The slice is enabled on first use and never disabled.** A pulse of zero
already produces a constant-low output, because a compare of 0 is always-low,
so `pwm/disable` is not needed to express "off". Calling it would stop the
sibling channel mid-operation with no diagnostic. The binding documents that
the driver never disables a slice.

## 8. Per-call sequence

`pwm_set_cycles(dev, channel, period_cycles, pulse_cycles, flags)`:

1. Validate per §6, under the driver mutex.
2. If the slice's current period differs, call `gallo_pwm_set_config(channel,
   frequency_hz, false)`.
3. Call `gallo_pwm_get_duty_cycle(channel)` to obtain `max_duty`.
4. Call `gallo_pwm_set_duty_cycle(channel, compare)`.
5. If step 2 ran and the sibling channel is configured, re-assert the
   sibling's compare from its tracked pulse/period ratio. The firmware's
   rescale truncates, so trusting it drifts the sibling's duty downward on
   every reconfiguration.
6. On the slice's first use, call `gallo_pwm_enable(channel)`.

State: per channel, last `period_cycles` and `pulse_cycles`; per slice,
`configured` and `enabled`. All guarded by a single `k_mutex` initialised
before any early exit in `init`, following `pdg_gpio.c`'s reasoning that the
PWM API dispatches into the driver without a readiness check.

## 9. Initialisation

Follows the mandatory MFD child sequence from `pdg_mfd.h`: require
`device_is_ready(parent)`, then `pdg_mfd_ctx(parent)`, treating a NULL context
after a passing readiness check as an ownership invariant failure. Then probe
`GALLO_CAP_PWM` (bit 4) through `device/info`, following the UART driver's
pattern, and return `-ENODEV` when clear.

Init priority 45, above the MFD parent's 40. PWM has no cross-child
dependency.

## 10. Files

| File | Action |
|---|---|
| `zephyr/drivers/pwm/pdg_pwm.c` | new — top half; parent `BUILD_ASSERT` block precedes `#include "pdg_mfd.h"` |
| `zephyr/drivers/pwm/pdg_pwm_bottom.c` | new — host half; functions `__attribute__((weak))` so fakes can override, as UART's are |
| `zephyr/drivers/pwm/pdg_pwm_bottom.h` | new — FFI-free interface, plus the cycles-per-second constant |
| `zephyr/drivers/pwm/Kconfig` | new — `PWM_PICO_DE_GALLO`, `PWM_PICO_DE_GALLO_INIT_PRIORITY` default 45 |
| `zephyr/drivers/pwm/CMakeLists.txt` | new — the two-context split, copied from `gpio/CMakeLists.txt` |
| `zephyr/drivers/Kconfig` | edit — `rsource "pwm/Kconfig"` |
| `zephyr/drivers/CMakeLists.txt` | edit — `add_subdirectory_ifdef`, **and add `CONFIG_PWM_PICO_DE_GALLO` to the `if()` guarding `common.c`**; that file's own comment warns that an omitted consumer links against a `common.c` never compiled |
| `zephyr/Kconfig` | edit — add `DT_HAS_ODP_PICO_DE_GALLO_PWM_ENABLED` to the `default y if` |
| `zephyr/dts/bindings/pwm/odp,pico-de-gallo-pwm.yaml` | new — `include: [pwm-controller.yaml, base.yaml]`, `"#pwm-cells": const: 3` plus a `pwm-cells:` list, documenting slice sharing, the absence of disable, and the unsupported flags |
| `zephyr/boards/shields/pico_de_gallo/pico_de_gallo.overlay` | edit — `pdg_pwm0` node, `status = "disabled"` |
| `zephyr/samples/pwm_fade/` | new — LED fade; `tests.yaml` with `build_only: true` |
| `zephyr/tests/pdg_fake/pwm/` | new — see §11 |
| `zephyr/README.md` | edit — driver list and unsupported-flag table |
| `zephyr/CHANGELOG.md` | edit — Added entry |

`zephyr/drivers/common/common.c` is **not** modified.

Per AGENTS.md §15.1, `zephyr/` has no book chapter by deliberate ruling;
`zephyr/README.md` and `zephyr/CHANGELOG.md` satisfy the documentation
requirement for this work.

## 11. Testing

PWM cannot be hardware-verified in this environment, and the Rust and FFI
layers below are treated as already validated. The host-executed fake suite is
therefore the primary evidence this change works, not a supplement to a
hardware demo.

`zephyr/tests/pdg_fake/pwm/` follows the existing `pdg_fake/i2c` pattern:
strong symbol definitions override the weak production bottom layer, so
nothing reaches the FFI and no USB device is opened. `tests.yaml` omits
`build_only`, so twister executes it in CI.

The fake **models the firmware's `compute_pwm_params` faithfully** — the same
ascending divider search over `1..=255` and the same
`top = floor(150e6 / (div * f)) - 1` — so the `max_duty` it returns is
realistic rather than invented. A fake that returned a fixed `max_duty` would
pass while proving nothing about the conversion.

Assertions:

1. **Frequency round-trip.** For a table of periods spanning the supported
   range, the achieved frequency is within tolerance of the requested one and
   is never *below* it.
2. **Duty accuracy.** The resulting compare, divided by the fake's `max_duty`,
   matches `pulse_cycles / period_cycles` within tolerance, including the 0%
   and 100% endpoints.
3. **Bounds.** `period_cycles` of 0, 1, and `16 711 681` are refused with the
   documented errno and **no call reaches the fake**.
4. **Polarity.** `PWM_POLARITY_INVERTED` returns `-ENOTSUP` with no call
   reaching the fake.
5. **Sibling conflict.** A conflicting period on the sibling channel returns
   `-EINVAL` and issues no `set_config`.
6. **Never disables.** No test scenario, including a zero pulse, results in a
   `gallo_pwm_disable` call.
7. **Sibling re-assertion.** After a reconfiguration, the sibling's compare is
   re-asserted rather than left to the firmware's truncating rescale.

## 12. Out of scope

- **Fixing the #192 divider panic.** It is a firmware defect reachable from
  every host surface; the bound in §6 contains it for Zephyr consumers only
  and is documented as containment, not a fix.
- **Fixing the book's PWM drift.** `book/src/interfaces/pwm.md` shows C
  examples using `GalloPwmDutyCycleInfo` and `GalloPwmConfigurationInfo`,
  neither of which exists in the generated header — the real signatures take
  separate scalar out-parameters — and documents `max_duty` as a constant
  65535 when it is `top + 1` and varies with frequency. Both are AGENTS.md
  §15.1 violations predating this work.
- **Phase-correct mode**, per §6.
- **`pwm_capture` ops.** The firmware exposes no capture endpoint.
