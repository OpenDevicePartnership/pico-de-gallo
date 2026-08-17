# Zephyr MFD restructure M3 — GPIO controller specification and implementation plan

Date: 2026-08-17
Branch baseline: `zephyr` at `4e100a097c70`
Milestone: M3 — additive `pdg_gpio` controller; no SPI consumer yet

## 1. Context, scope, and final inventory

M3 adds a standard Zephyr GPIO controller as a direct child of the existing
`odp,pico-de-gallo` MFD parent. It borrows the parent's validated USB handle and
provides ABI-safe, non-NULL dispatch for all six unconditionally called
`gpio_driver_api` slots through the existing C FFI. Five operations are available;
toggle dispatch exists but returns `-ENOTSUP`. It adds no parent lock or firmware
pin-state cache, interrupts, SPI changes, Rust/firmware/wire changes, version
bumps, or lockfile changes. The shield node remains disabled; M4 will consume it.

### 1.1 Final file inventory

**Create**

- `docs/superpowers/specs/2026-08-17-zephyr-mfd-m3-gpio.md`
- `docs/superpowers/specs/2026-08-17-zephyr-mfd-m3-gpio-tests.md` — the M3
  adversarial probe suite and acceptance document: the Class A compile-time
  gates, Class B source-structural probes, mutation controls, and the assurance
  boundary against which M3 is accepted.
- `zephyr/drivers/gpio/{pdg_gpio.c,pdg_gpio_bottom.c,pdg_gpio_bottom.h,CMakeLists.txt,Kconfig}`
- `zephyr/dts/bindings/gpio/odp,pico-de-gallo-gpio.yaml`

**Modify**

- `zephyr/Kconfig`
- `zephyr/drivers/{CMakeLists.txt,Kconfig}`
- `zephyr/boards/shields/pico_de_gallo/pico_de_gallo.overlay`
- `zephyr/README.md`
- `book/src/interfaces/gpio.md`
- `zephyr/CHANGELOG.md`

Plan §3 omits this spec and its test suite, top-level `zephyr/Kconfig`, README,
GPIO book page, and
changelog. The top-level symbol must discover a GPIO-only tree; AGENTS.md §15.1
requires same-change reference docs. No sample overlay changes in M3.

## 2. Decisions

| Decision | Contract and rationale |
| --- | --- |
| Six ABI-safe non-NULL dispatches, no interrupt fields | All six wrappers dispatch unconditionally, so every slot is non-NULL. Toggle capability remains unavailable and its stub returns `-ENOTSUP`. The interrupt-related `pin_interrupt_configure`, callback-management, and pending-interrupt wrappers NULL-check and return `-ENOSYS` (D5). |
| Require explicit parent `serial-number` for enabled GPIO | Selector-less strict open cannot reveal which attached board was selected; compile-time rejection removes unidentifiable physical-pin actuation. Successful init logs the configured serial. Presence is not uniqueness. |
| Validate DT `ngpios` exactly against `gallo_num_gpios()` at init | Otherwise `gpio.h` asserts against a wrong DT mask before driver dispatch (R4). |
| Use a GPIO bottom-half count call, not a new parent accessor | Parent strict validation already populated the handle-shared cache; the child read is local. Expanding `pdg_mfd.h` earns nothing. |
| Child-local mutex, no parent lock | Serializes one GPIO operation's RPC sequence without creating M4's GPIO↔SPI deadlock (D6). |
| No level/direction/pull cache | Avoids issue #104-class host/firmware divergence (design §6). |
| Masked writes are ascending per-pin puts, with no pre-read | Writing only masked pins preserves unmasked pins; get is invalid on explicit outputs. |
| `port_toggle_bits` returns `-ENOTSUP` | The FFI has no latch-read/toggle; get-then-set fails on `ExplicitOutput`. Caching and direction-flipping are unsafe. |
| GPIO init priority 45 | Establishes parent 40 < GPIO 45 < SPI 50 rather than relying on link order. |
| Normalize currently reachable GPIO `-ECOMM` to `-EIO` locally | Common mapping collapses both `CommsFailed` and `OneWireNoPresence` to `-ECOMM`, but GPIO can currently reach only `CommsFailed` through `gpio_error_to_status()`; Zephyr GPIO's external-chip contract requires `-EIO`. Keep the normalization GPIO-local and re-examine it if either status set changes. |

The masked-write and toggle decisions correct assumptions in the parent design
and request; §15 records the parent-level consequences.

## 3. Binding and shield topology

### 3.1 Binding

Create `zephyr/dts/bindings/gpio/odp,pico-de-gallo-gpio.yaml` with:

```yaml
compatible: "odp,pico-de-gallo-gpio"
include: [gpio-controller.yaml, base.yaml]
gpio-cells: [pin, flags]
```

Actual upstream `nordic,npm1300-gpio.yaml` includes only
`gpio-controller.yaml`; that file does not include `base.yaml` in installed
Zephyr. Follow the parent design's explicit combined include. Add no `on-bus:`:
this is a controller child, not a device on a private bus.

The description is normative and documents:

- direct-child-of-enabled-MFD, borrowed connection, and mandatory explicit
  `serial-number` on the parent of every enabled GPIO child; the configured serial
  is logged on successful GPIO initialization;
- every operation blocks over host USB; ISR calls return `-EWOULDBLOCK`;
- transport failure is `-EIO`;
- exactly input or output; disconnected and input+output return `-ENOTSUP`;
- none/up/down pull; both pulls return `-EINVAL`;
- single-ended, open-source and open-drain return `-ENOTSUP`;
- `GPIO_ACTIVE_LOW` is handled by Zephyr's common layer;
- output initialization is two non-atomic RPCs and may expose the
  previous/HAL-defined level; a failed configuration may nevertheless have been
  applied, and logical polarity is unreliable until successful reconfiguration;
- `ngpios` must equal firmware `device/info.num_gpios`; a mismatch is a local
  devicetree/firmware configuration error and fails init with `-EINVAL`; Zephyr
  evaluates the devicetree-derived mask assertion before driver dispatch
  (`gpio.h:1040`);
- multi-pin writes are deterministic ascending, non-atomic per-pin RPCs with
  precisely logged acknowledged-prefix failure;
- interrupt configuration, callback management, and pending-interrupt API are
  `-ENOSYS`;
- toggle is `-ENOTSUP` because explicit outputs cannot be read and state is not
  cached. Generic toggle consumers including blinky, the GPIO shell, the TPS382x
  watchdog, and the LS0xx display therefore do not work with this controller.

Override inherited `ngpios` as required. Binding YAML cannot express an integer
range, so the binding prose is not the enforcement mechanism. A fifth
per-instance `BUILD_ASSERT` in `pdg_gpio.c` enforces it with the exact condition
`(DT_INST_PROP(inst, ngpios) >= 1) && (DT_INST_PROP(inst, ngpios) <=
GPIO_MAX_PINS_PER_PORT)` and exact diagnostic
`odp,pico-de-gallo-gpio ngpios must be between 1 and 32`. Put this assertion
immediately after the four parent/Kconfig assertions in §4.1 and before the
`pdg_mfd.h` include; it must not disturb their normative relative order.

On the pinned Zephyr `main` target, `gpio_port_pins_t` is `uint32_t` and
`GPIO_MAX_PINS_PER_PORT` is therefore 32 (`gpio.h`, typedef and maximum-width
macro). Thirty-two is the correct ceiling for `port_pin_mask`, not a firmware
schema maximum. Do not hard-code 4 and do not add reserved ranges; exact runtime
equality remains authoritative. `GPIO_COMMON_CONFIG_FROM_DT_INST` and the
driver's defensive `ngpios` checks are two consumers of this one DT value, not
independent limits: the build assertion proves the value fits
`gpio_port_pins_t` before either is instantiated. Consequently
`config->common.port_pin_mask` and `config->ngpios` remain coherent for every
buildable instance. Any future Zephyr width change must update the diagnostic
text and its probes together with this assertion.

### 3.2 Shield

Insert this direct child of `pdg0` immediately before I2C, preserving
`gpio`→`i2c`→`spi` order:

```dts
		pdg_gpio0: gpio {
			compatible = "odp,pico-de-gallo-gpio";
			gpio-controller;
			#gpio-cells = <2>;
			ngpios = <4>;
			status = "disabled";
		};
```

Do not enable it or change SPI/`cs-gpio-indices`; that is M4. Any overlay that
does enable this child for compile verification must also put `serial-number` on
`pdg0`, or the M3 compile-time identity gate intentionally fails.

## 4. Structures, topology, and init

```c
struct pdg_gpio_config {
	struct gpio_driver_config common; /* MUST be first */
	const struct device *mfd;
	const char *serial_number;
	uint8_t ngpios;
};
struct pdg_gpio_data {
	struct gpio_driver_data common; /* MUST be first */
	void *ctx;
	struct k_mutex lock;
};
```

`gpio.h` directly casts config/data to the common prefixes. Initialize `.common
= GPIO_COMMON_CONFIG_FROM_DT_INST(inst)`, `.mfd =
DEVICE_DT_GET(DT_INST_PARENT(inst))`, `.serial_number =
DT_INST_PROP_BY_PHANDLE_IDX_OR(inst, ...)` is **not** appropriate because the
selector belongs to the parent; use
`.serial_number = DT_PROP(DT_INST_PARENT(inst), serial_number)`, and `.ngpios =
DT_INST_PROP(inst, ngpios)`. Include `gpio_utils.h` for the common initializer.
The config field is the compile-time devicetree string used solely for the
successful-init identity log; it is not queried from, or used to select, a second
handle.

### 4.1 R9/R10 pattern — copy M2 exactly

Before including `pdg_mfd.h`, expand per-instance `BUILD_ASSERT`s in fixed order:

1. parent compatible is `odp_pico_de_gallo`;
2. parent status is okay;
3. the parent has an explicit serial selector:
   `BUILD_ASSERT(DT_NODE_HAS_PROP(DT_INST_PARENT(inst), serial_number),
   "odp,pico-de-gallo-gpio parent must define serial-number");`
4. `IS_ENABLED(CONFIG_MFD_PICO_DE_GALLO)`.

Then emit the fifth `ngpios` range assertion specified in §3.1. The four-item
compatible -> parent-status -> serial-presence -> Kconfig **source order** is the
normative contract. Compiler-emitted diagnostic order is not specified by C or
GCC and is only corroborating probe evidence; a build that prints simultaneous
failures in another order does not violate M3 if the source order is correct.

The serial-presence assertion follows structural parent validity and precedes the
Kconfig dependency: first prove this is the right enabled hardware node, then
prove that GPIO actuation has an explicit identity, and only then diagnose the
software dependency. Use M2's readable messages with
`odp,pico-de-gallo-gpio`. The block must precede the include because
`add_subdirectory_ifdef` removes the header path when MFD is off; an include
error must not mask the assertions. This closes R11 for GPIO actuation: M1's
parent-count assertion cannot count attached USB devices, while a selector-less
strict constructor cannot report which board it selected. Presence does not
prove uniqueness; two parents that explicitly name the same serial still alias,
as the M1 binding already documents.

Init is exactly:

1. initialize mutex before every early return;
2. parent readiness else logged `-ENODEV`;
3. call `pdg_mfd_ctx(config->mfd)` only after readiness;
4. NULL after readiness is logged invariant failure, `-ENODEV`;
5. call `pdg_gpio_bottom_num_gpios`;
6. on failure, log, clear only child `ctx`, return errno;
7. if runtime count differs from DT `ngpios`, log both, clear only child `ctx`,
   return `-EINVAL`;
8. log the configured parent `serial-number`, making the selected binding
   observable;
9. return success without touching any pin.

The child never closes/frees the borrow. Every post-borrow init failure only
sets its own `ctx = NULL`.

### 4.2 R4 decision

Exact init validation is mandatory. `gallo_registry.c` opens through
`gallo_init_strict[_with_serial_number]`; both constructors call
`PicoDeGallo::validate()`, which stores `DeviceInfo.num_gpios` in the
handle-shared `OnceLock`. Thus the parent's init already paid the single
300-second-bounded `device/info` round-trip. The child `gallo_num_gpios()` is a
warm local read with no USB traffic and no 300-second timeout exposure in this
child boot path. This R4 claim is verified against `pdg_mfd.c:76`,
`gallo_registry.c:173-177`, `pico-de-gallo-ffi/src/lib.rs:584-597`, and the
shared `OnceLock` at `pico-de-gallo-lib/src/lib.rs:1055-1067,1085-1093`.
Exact equality and fail-child `-EINVAL` are correct: clamping would either expose
firmware-invalid indices or silently hide valid GPIOs. The mismatch is a local
DT/firmware configuration error, not an FFI argument error.

Do not add `pdg_mfd_num_gpios()`. It would duplicate parent data or move a
bottom-half call for one consumer, while the validated handle already owns the
cache. Plan §8.2 permits, but does not require, an accessor.

## 5. Bottom-half contract

`pdg_gpio_bottom.h` includes only `stdbool.h`/`stdint.h`, C++ guards, and exactly:

```c
int pdg_gpio_bottom_get(void *ctx, uint8_t pin, bool *state);
int pdg_gpio_bottom_put(void *ctx, uint8_t pin, bool state);
int pdg_gpio_bottom_set_config(void *ctx, uint8_t pin,
			       uint8_t direction, uint8_t pull);
int pdg_gpio_bottom_num_gpios(void *ctx, uint8_t *out_num_gpios);
```

No open/close wrappers: this child is born into MFD ownership.

`pdg_gpio_bottom.c` includes standard C headers, `pico_de_gallo.h`, `common.h`,
and its own header, **no Zephyr header**. Functions forward exactly to
`gallo_gpio_get`, `gallo_gpio_put`, `gallo_gpio_set_config`, and
`gallo_num_gpios`, converting through `pdg_common_status_to_errno()`. A private
normalizer maps only resulting `-ECOMM` to `-EIO`; preserve all other errno.
`pdg_common_status_to_errno()` maps both `CommsFailed` and
`OneWireNoPresence` to `-ECOMM` (`common.c:31-32`), so the normalizer cannot infer
which original status it saw. The narrower source contract is that all currently
reachable GPIO `-ECOMM` outcomes originate from `CommsFailed`:
`gpio_error_to_status()` exposes only GPIO statuses (`pico-de-gallo-ffi/src/lib.rs:458-467`),
and 1-Wire status is unreachable through these four bottom-half calls. M3's
enforceable contract is a normalizer comment naming `gpio_error_to_status()`,
the `common.c:31-32` collapse, and `OneWireNoPresence`, as checked by M3-B-12.
That citation is sufficient for M3; it makes the premise a visible human-review
obligation but does not mechanically observe Rust changes. A real Rust status-set
test is assigned to the post-M6 M7 follow-up in §15. Useful
mappings remain invalid pin `-EINVAL`, wrong direction `-EACCES`, monitored
`-EBUSY`, endpoint/transport `-EIO`, uninitialized `-ENODEV`.
## 6. Callback contract

The API object sets exactly:

```c
static DEVICE_API(gpio, pdg_gpio_api) = {
	.pin_configure = pdg_gpio_pin_configure,
	.port_get_raw = pdg_gpio_port_get_raw,
	.port_set_masked_raw = pdg_gpio_port_set_masked_raw,
	.port_set_bits_raw = pdg_gpio_port_set_bits_raw,
	.port_clear_bits_raw = pdg_gpio_port_clear_bits_raw,
	.port_toggle_bits = pdg_gpio_port_toggle_bits,
};
```

Do not assign `pin_interrupt_configure`, callback management, pending-int,
`pin_get_config`, or `port_get_direction`. The three interrupt-related operations
(`pin_interrupt_configure`, callback management, and pending-int) return
`-ENOSYS` through wrapper NULL checks; `pin_get_config` and
`port_get_direction` are also optional, NULL-guarded, and return `-ENOSYS` when
called (`gpio.h:1104-1107,1233-1236`).

Every callback's operational checks are ordered:

1. `if (k_is_in_isr()) return -EWOULDBLOCK;`;
2. NULL `data->ctx` logs/returns `-ENODEV`;
3. argument/mask validation;
4. lock only after those guards.

The NULL guard is **load-bearing**. Installed `gpio.h` dispatches without
`device_is_ready`: `z_impl_gpio_pin_configure` lines 998–1052, get 1282–1291,
masked set 1351–1361, set 1414–1424, clear 1459–1469, toggle 1504–1514. Pin
helpers at 1582, 1671, and 1759 assert mask then dispatch, also without
readiness. Conversely interrupt NULL-checks at 901–904, callback add/remove at
1827/1879, and pending-int at 1929.

### 6.1 `pin_configure`

```c
static int pdg_gpio_pin_configure(const struct device *port,
				  gpio_pin_t pin, gpio_flags_t flags);
```

After common guards, defensively reject `pin >= config->ngpios` with `-EINVAL`.
Normal public use first hits Zephyr's `port_pin_mask` assertion (`gpio.h:1040`),
so this protects direct dispatch/release builds; it cannot promise graceful
invalid-public-pin recovery.

Validate all flags per §7 before RPC. Map input/output to direction 0/1 and
none/up/down to pull 0/1/2. Under one lock:

1. `pdg_gpio_bottom_set_config(ctx, pin, direction, pull)`;
2. on failure return immediately, no level RPC;
3. absent init-level flag: success;
4. `pdg_gpio_bottom_put(ctx, pin, init_high)`;
5. return its result.

Configuration-before-level is mandatory: put rejects an explicit input, while
set-config establishes explicit output. Put-before-config only works
accidentally from firmware `LegacyAuto`. Consequence: output is enabled before
the desired level arrives, so the previous/HAL-defined level can briefly appear. If put
fails, the pin remains explicit output with requested pull, but at its
previous/HAL-defined level; return the error and do not attempt rollback. If
`set_config` itself returns an error, the requested direction/pull is
indeterminate: the firmware may have applied it and only the acknowledgement may
have been lost. Do not roll back because prior state is unavailable and rollback
RPCs can also fail.

Zephyr's `z_impl_gpio_pin_configure()` updates the Zephyr-owned
`gpio_driver_data.invert` before dispatch (`gpio.h:1040-1049`). This is logical
polarity metadata, not a forbidden cache of firmware state, but a failed driver
call leaves it potentially ahead of firmware. After any failed `pin_configure`,
logical polarity is unreliable until a successful reconfiguration. Do not add a
host-side firmware-state cache to compensate.

### 6.2 `port_get_raw`

```c
static int pdg_gpio_port_get_raw(const struct device *port,
				 gpio_port_value_t *value);
```

Reject NULL `value` with `-EINVAL`. Lock once, zero a local temporary, and call
bottom-get in ascending order for every `0..ngpios-1`. The disposition is
exhaustive for every firmware pin mode and every status `gallo_gpio_get` can
produce:

| Firmware state at this pin | Firmware/FFI evidence and resulting status | `port_get_raw` disposition |
| --- | --- | --- |
| `LegacyAuto`, pin slot present | `gpio_for_input!` calls `gpio.set_as_input()` and then reads the pad (`firmware/src/handlers/gpio.rs:21-37,40-51`); `gallo_gpio_get` writes the returned level and returns `Ok` (`ffi/src/lib.rs:1594-1621`) | Record the returned bit and continue. This **mutates the hardware direction to input on every read**, but does **not** mutate `pin_modes`: the macro copies the mode and never writes it. Thus a whole-port read reconfigures every present `LegacyAuto` pin to input, including unrelated pins. |
| `ExplicitInput`, pin slot present | `gpio_for_input!` leaves direction unchanged and reads the pad (`gpio.rs:31-35,40-51`); FFI returns `Ok` | Record the returned bit and continue. |
| `ExplicitOutput`, pin slot present | `gpio_for_input!` returns `GpioError::WrongDirection` (`gpio.rs:31-35`); `gpio_error_to_status` maps it to `GpioWrongDirection` (`ffi/src/lib.rs:458-467`), then `common.c:37` maps it to `-EACCES` | Leave this non-input bit zero and continue. |
| Any of the three modes, pin slot absent because it is monitored | Slot lookup returns `GpioError::PinMonitored` before the mode match (`gpio.rs:24-30`); FFI maps it to `GpioPinMonitored` (`ffi/src/lib.rs:458-467`), then `common.c:39` to `-EBUSY` | Abort, leave caller `*value` untouched, log the monitored index, and return `-EIO`. Plan R7's orphaned subscription on pin 2 can therefore make a whole-port read fail until cleanup, but returning a confident zero would be wrong because the monitor owns an input pin. `-EBUSY` is not in Zephyr's `port_get_raw` return contract. |
| Pin index absent from `pin_modes` or `gpios` | Either bounds lookup returns `GpioError::InvalidPin` (`gpio.rs:23-30`); FFI maps it to `GpioInvalidPin` (`ffi/src/lib.rs:458-467`), then `common.c:21` to `-EINVAL` | Abort, leave caller `*value` untouched, and return `-EIO`, not `-EINVAL`: a DT-valid index rejected by firmware is a controller/firmware inconsistency after successful count validation, not a caller argument error. Log the rejected index and original `-EINVAL`. |
| Any mode/state, endpoint returns `GpioError::Other` | FFI maps it to `GpioGetFailed` (`ffi/src/lib.rs:458-467`), then `common.c:65` to `-EIO` | Abort with `-EIO`; leave caller `*value` untouched. |
| Any mode/state, RPC transport fails | `pico-de-gallo-lib::gpio_get` propagates the `send_resp` transport error (`lib/src/lib.rs:691-695`); FFI maps `PicoDeGalloError::Comms` to `CommsFailed` (`ffi/src/lib.rs:458-467`); the GPIO-local normalizer converts common `-ECOMM` to `-EIO` (§5) | Abort with `-EIO`; leave caller `*value` untouched. |
| Invalid bottom-half context or output pointer | `gallo_gpio_get` returns `Uninitialized` for NULL context and `InvalidArgument` for NULL state (`ffi/src/lib.rs:1594-1607`), mapping through `common.c:33,26` to `-ENODEV`/`-EINVAL` | These are driver invariant failures, not pin states, and the driver's own non-NULL guards make them unreachable by construction. If one nevertheless reaches the loop, fail closed: log the unspecified status, normalize it deliberately to `-EIO` so `port_get_raw` stays within `gpio.h:1275-1277`, abort, and leave caller `*value` untouched. |

`GpioError::PinNotMonitored` and `GpioError::Timeout` are in
`gpio_error_to_status()` but cannot be returned by `gpio/get`: only unsubscribe
constructs `PinNotMonitored`, and only wait handlers construct `Timeout`
(`gpio.rs:86-259,330-355`). No residual status is propagated unchanged: any
non-zero status not enumerated above is logged as absent from this exhaustive
table and normalized to `-EIO`. A future GPIO status or `gpio/get` return path
is still a specification change and must extend this table before the
implementation accepts it explicitly.

The original total-failure concern for a normal partially configured port is
disproved: `LegacyAuto` succeeds. The previous “abort on anything but
`-EACCES`” control-flow rule is therefore safe for availability, and aborting a
monitored pin is also correct because its input level is unavailable. Its errno
rule was still unsafe: it leaked `-EBUSY`, which Zephyr does not enumerate for
`port_get_raw`; this specification now normalizes that known case to `-EIO`.
The source analysis also exposes a more serious side effect: a whole-port read
switches all present `LegacyAuto` pins to hardware input while preserving
`PinMode::LegacyAuto`. This contradicts the parent design's claim that every
query is state-neutral/no-caching in effect, even though no host cache is added.
M3 cannot remove the side effect without a new firmware API or a Zephyr-side
configured-direction cache, both out of scope. Treat it as a deferred runtime
risk owned by M7 in §15; M3 documents and tests the source-defined behaviour.

On full success assign the temporary. Every call re-queries firmware; there is
no host cache. Do not emit `LOG_WRN` for skipped `-EACCES`: this is the hot
path of every `gpio_pin_get()`, and M4 chip-select activity could otherwise log
at transfer rate. Monitored-pin and firmware-index inconsistencies abort and are
logged as errors. A `LOG_DBG` for skipped outputs is permissible but not required;
`gpio_emul` logs nothing.

This is the reference behaviour, not a concession. Zephyr scopes the operation
to input pins — *"Get physical level of all input pins in a port"*
(`gpio.h:1263`) — and the reference controller implements exactly this masking:
`*values = drv_data->input_vals & get_input_pins(port);`
(`drivers/gpio/gpio_emul.c:525`; helper at lines 143-146), returning zero for
every non-`GPIO_INPUT` pin and succeeding. No in-tree driver returns a per-pin
error from `port_get_raw` on account of direction; the three drivers that error
do so unconditionally because their devices have no input pins at all. Five
in-tree controllers with this same no-simultaneous-input/output constraint reject
`GPIO_INPUT | GPIO_OUTPUT` with `-ENOTSUP` and retain ordinary read semantics:
`gpio_aw9523b.c:74-76`, `gpio_bee.c:199-201`,
`gpio_pca_series.c:1018-1019`, `gpio_rt1718s_port.c:47-48`, and
`gpio_smartbond.c:124-126`.

**This rule is valid only in combination with §7's rejection of
`GPIO_INPUT | GPIO_OUTPUT` with `-ENOTSUP`, and the two must not be changed
independently.** §7 makes it impossible to configure a pin for readback we cannot
perform. Consequently a zero reported here is, by construction, not an input
pin, and no caller has been promised its value. Were §7 relaxed, this rule would
return confident false levels — under `GPIO_ACTIVE_LOW`, a false logical `1`,
because `z_impl_gpio_port_get` XORs `data->invert` over our zero
(`gpio.h:1322-1325`) — and recreate the issue-#104 failure mode.

Propagating `-EACCES` or `-EBUSY` is rejected independently because neither is
an enumerated return (`gpio.h:1275-1277` lists only `0`, `-EIO`,
`-EWOULDBLOCK`) and propagating direction failure would fail
`tests/drivers/gpio/gpio_basic_api`: that suite requires
`gpio_port_get_raw() == 0` while `PIN_OUT` is an output on the same port as
`PIN_IN` (`test_gpio_port.c:95-97`; `test_gpio.h:26,44-45`, pins 2 and 3).
`gpio_pin_get()` is not a direction oracle, and this controller's optional
direction-query slots are NULL, so `gpio_pin_is_input()` /
`gpio_pin_is_output()` return `-ENOSYS` (`gpio.h:1116-1170`). Output bits are
never latch queries.

### 6.3 Masked, set, and clear

```c
static int pdg_gpio_port_set_masked_raw(const struct device *port,
	gpio_port_pins_t mask, gpio_port_value_t value);
static int pdg_gpio_port_set_bits_raw(const struct device *port,
	gpio_port_pins_t pins);
static int pdg_gpio_port_clear_bits_raw(const struct device *port,
	gpio_port_pins_t pins);
```

Reject `(mask & ~config->common.port_pin_mask) != 0U` (or analogous `pins`) with
`-EINVAL`. Ignore value outside mask. Zero mask succeeds without RPC. Under one
lock, call bottom-put for each selected pin in ascending order with
`(value & BIT(pin)) != 0U`; stop on first failure.

There is no port-write transaction. Ascending pin order is deliberate and
stable: generic GPIO cannot identify safety-critical pins, M4 chip-select is a
single-pin operation, and deterministic prefix semantics are more useful than an
arbitrary alternative. On failure, earlier acknowledged selected pins definitely
changed; the failed pin is specifically indeterminate because its request may
have executed and only the response may have been lost; later selected pins were
not issued and therefore were not changed by this operation, although their
external state remains generally unverified. Do not roll back: prior state is
unavailable and rollback RPCs can also fail.

Any multi-pin partial failure must emit `LOG_ERR` naming the operation, failed
pin index, requested mask/value, and acknowledged prefix mask. The caller receives
only errno; deterministic order makes this diagnostic actionable at negligible
cost. There is **no read-modify-write**: not writing unmasked pins preserves them,
and pre-read is invalid for explicit outputs.

Set/clear use a private locked helper with value `pins`/zero. Each public
callback owns ISR/context/mask checks; do not recursively call a public callback
or double-lock.

### 6.4 Toggle

```c
static int pdg_gpio_port_toggle_bits(const struct device *port,
	gpio_port_pins_t pins);
```

After ISR/context/mask checks, zero pins succeeds; nonzero returns `-ENOTSUP`
without lock or RPC. Normal Zephyr output configuration records
`ExplicitOutput`, for which `gallo_gpio_get` returns
`GpioWrongDirection`/`-EACCES`. Temporarily switching input reads the pad rather
than latch, glitches/tri-states it, and requires forbidden cached config.
Returning `-ENOTSUP` is honest. ABI-safe non-NULL dispatch exists, but toggle
capability does not. Generic toggle consumers including blinky, the GPIO shell,
the TPS382x watchdog, and the LS0xx display therefore do not work with this
controller. M4 is unaffected because `spi_context_cs_control()` uses
`gpio_pin_set_dt()`, not toggle (`spi_context.h:390-405`).

## 7. Exhaustive flag mapping

The Zephyr wrapper resolves logical output initialization first: active-low
flips INIT_LOW/HIGH where needed and strips only `GPIO_OUTPUT_INIT_LOGICAL`
(`gpio.h:1032-1038`). The driver sees physical init bits. `GPIO_ACTIVE_LOW`
remains allowed; common `gpio_driver_data.invert` handles logical raw conversion.
Standard interrupt-mode bits are assertion-rejected, not stripped:
`z_impl_gpio_pin_configure()` asserts `(flags & GPIO_INT_MASK) == 0` at
`gpio.h:1011-1012`, but in `CONFIG_ASSERT=n` builds bits 21-26 reach the driver.
`GPIO_INT_WAKEUP` is bit 6 and is not in `GPIO_INT_MASK`
(`gpio.h:157-162`; `dt-bindings/gpio/gpio.h:83`), so it reaches the driver in all
builds. Both classes are rejected by the residual allow-list check.

Validate in this order:

| Case | Exact detection | Result |
| --- | --- | --- |
| Disconnected | `(flags & (GPIO_INPUT | GPIO_OUTPUT)) == 0U` | `-ENOTSUP` |
| Input and output | `(flags & (GPIO_INPUT | GPIO_OUTPUT)) == (GPIO_INPUT | GPIO_OUTPUT)` | `-ENOTSUP` |
| Single-ended/open-source/open-drain | `(flags & GPIO_SINGLE_ENDED) != 0U` | `-ENOTSUP` |
| Open-drain selector | `(flags & GPIO_LINE_OPEN_DRAIN) != 0U` | `-ENOTSUP` |
| Both pulls | `(flags & (GPIO_PULL_UP | GPIO_PULL_DOWN)) == (GPIO_PULL_UP | GPIO_PULL_DOWN)` | `-EINVAL` |
| Both init levels | `(flags & (GPIO_OUTPUT_INIT_LOW | GPIO_OUTPUT_INIT_HIGH)) == (GPIO_OUTPUT_INIT_LOW | GPIO_OUTPUT_INIT_HIGH)` | `-EINVAL` |
| Init without output | `(flags & (GPIO_OUTPUT_INIT_LOW | GPIO_OUTPUT_INIT_HIGH)) != 0U && (flags & GPIO_OUTPUT) == 0U` | `-EINVAL` |
| Standard interrupt-mode bits | `(flags & GPIO_INT_MASK) != 0U` | `-ENOTSUP` (reachable in assertion-free/direct dispatch) |
| Interrupt wakeup | `(flags & GPIO_INT_WAKEUP) != 0U` | `-ENOTSUP` (always reaches driver) |
| Any other residual bit | `(flags & ~PDG_GPIO_ALLOWED_FLAGS) != 0U` | `-ENOTSUP` |

```c
#define PDG_GPIO_ALLOWED_FLAGS \
	(GPIO_INPUT | GPIO_OUTPUT | GPIO_PULL_UP | GPIO_PULL_DOWN | \
	 GPIO_OUTPUT_INIT_LOW | GPIO_OUTPUT_INIT_HIGH | GPIO_ACTIVE_LOW)
```

Assertions may reject some combinations first, but driver checks remain for
direct dispatch/assertion-free builds. Explicit drive checks precede residual so
the contract is visible. Open-source's line-selector value is zero, but it sets
`GPIO_SINGLE_ENDED`, so it is rejected.

The positive allow-list prevents silent ignore. A new Zephyr bit is rejected
unless a reviewer deliberately adds and maps it. Reviewers enumerate every
nonzero configuration flag from installed `gpio.h` and `dt-bindings/.../gpio.h`
and require it in mapping or rejection tests. Zero aliases
(`GPIO_DISCONNECTED`, `ACTIVE_HIGH`, `PUSH_PULL`, `LINE_OPEN_SOURCE`) are checked
through composite semantics.

## 8. Build glue and ordering

GPIO CMake mirrors I2C: Zephyr library with `pdg_gpio.c` and local include path;
`pdg_gpio_bottom.c` plus include path on `native_simulator INTERFACE`.

GPIO Kconfig defines:

- `GPIO_PICO_DE_GALLO`: bool, default y, depends on
  `DT_HAS_ODP_PICO_DE_GALLO_GPIO_ENABLED` and `ARCH_POSIX`, selects `GPIO`;
- `GPIO_PICO_DE_GALLO_INIT_PRIORITY`: int, depends on driver, default 45; help
  requires after MFD and before SPI.

The device is `POST_KERNEL/CONFIG_GPIO_PICO_DE_GALLO_INIT_PRIORITY`.

Top-level edits:

- `rsource "gpio/Kconfig"` and `add_subdirectory_ifdef(... gpio)` between MFD
  and I2C;
- add `DT_HAS_ODP_PICO_DE_GALLO_GPIO_ENABLED` to top-level
  `PICO_DE_GALLO` default;
- add `CONFIG_GPIO_PICO_DE_GALLO` to the common-source/`-Werror=switch` guard.

A valid GPIO implies MFD and thus currently makes the existing common guard true,
but GPIO directly links the common mapper, malformed probes override MFD, and a
GPIO-only app can disable I2C/SPI. The guard should name every direct consumer.
Priorities default parent 40 < GPIO 45 < I2C/SPI 50; runtime readiness remains
load-bearing because priorities are configurable.

## 9. Invariants and failure modes

### 9.1 Invariants

1. Compile-time direct, okay PDG parent and MFD Kconfig proof.
2. Common config/data first; mask derives from DT `ngpios`.
3. Mutex initialized before every init return.
4. Readiness→accessor→invariant-NULL→count equality.
5. Child never owns/closes context; post-borrow failure clears child pointer.
6. ISR guard precedes context; context guard precedes lock in every callback.
7. Multi-RPC operations hold the child mutex throughout; no parent lock.
8. No level/direction/pull cache; flags accepted only by positive allow-list.
9. Multi-pin writes and output initialization are explicitly non-atomic.
10. Interrupt/callback/pending fields remain NULL; all six unconditional slots
    are non-NULL, while toggle capability remains unavailable.
11. Every enabled GPIO child's parent has explicit `serial-number`; successful
    init logs that configured serial. Presence does not prove selector uniqueness.
12. No GPIO callback calls SPI. M4 creates only SPI -> GPIO ordering, so no
    reverse lock path or deadlock is introduced.
13. GPIO stays in the `zephyr/drivers/CMakeLists.txt` common-source guard.
14. No SPI/crate/firmware/wire/version/lockfile change.

### 9.2 Exact failure residue

- Parent/count failure or mismatch: child not ready; parent/siblings remain valid.
- Mid-masked/set/clear: earlier acknowledged pins definitely changed; the failed
  pin is indeterminate because execution may precede a lost response; later pins
  were not issued and were not changed by this operation. Log operation, failed
  index, requested mask/value, and acknowledged prefix; no rollback.
- Toggle: `-ENOTSUP` before RPC, hardware unchanged; therefore no mid-sequence
  transport state exists in M3.
- Failed `set_config`: requested direction/pull may already be applied despite the
  error. Failed output-init put leaves explicit output with requested pull at its
  previous/HAL-defined level. No rollback.
- Failed `pin_configure` after an active-low transition: Zephyr's `invert`
  metadata may have changed before dispatch, so logical polarity is unreliable
  until successful reconfiguration.
- Port read records `LegacyAuto`/explicit-input levels, masks only
  explicit-output `-EACCES` to zero, and commits only after the full scan. A
  monitored-pin `-EBUSY` is logged/normalized to `-EIO`; every abort leaves
  caller output untouched. A DT-valid
  index rejected by firmware is logged and normalized from `-EINVAL` to `-EIO`.
- A successful read of any present `LegacyAuto` pin changes its hardware
  direction to input without changing firmware `pin_modes`; this source-defined,
  non-idempotent side effect is an M7 runtime-risk follow-up, not hidden by M3.
- Selector-less GPIO actuation is compile-time rejected. Duplicate explicit
  serial selectors can still alias and remain a deployment error.
## 10. Documentation parity

`zephyr/README.md` must update overview/topology to include GPIO, add an enabled
parent+GPIO usage example, list §7 flags/errors, blocking/ISR, non-atomic writes
and init, unavailable toggle/interrupts, output-read limitation, mandatory
parent `serial-number`, duplicate-selector residual, configured-serial init log,
and GPIO troubleshooting. Do not pre-write M4 `cs-gpios` truth.

`book/src/interfaces/gpio.md` adds a bounded Zephyr section with topology,
enablement, mandatory explicit parent serial, indices, supported/rejected flags,
blocking, non-atomicity, no toggle or interrupts, and the known incompatibility of
blinky, GPIO shell, TPS382x watchdog, and LS0xx display toggle consumers. Correct the existing false “pins default to input” statement:
firmware starts in `LegacyAuto` and lazily selects direction until explicit
configuration. Leave unrelated CLI/Rust/C/HAL material unchanged.

`zephyr/CHANGELOG.md` Unreleased/Added records the controller/binding, six API
slots, count equality, MFD ownership, blocking/ISR, flag mapping, partial
multi-pin/init failures, toggle `-ENOTSUP`, and deferred interrupts. No SPI
semantic claim in M3.

## 11. Implementation plan

Only `@integrator` commits. Tasks are serialized review units.

### Task 1 — Binding/topology tests (RED)

1. Add source-contract checks for compatible, combined include, two cells,
   required `ngpios`, mandatory explicit parent serial, every binding
   limitation, and no `on-bus`.
2. Require exact disabled shield child before I2C.
3. Prepare negative probes: root child, ready unrelated parent, two-level child,
   disabled PDG parent, enabled GPIO child without parent `serial-number`, and
   explicit MFD Kconfig off; add a valid anti-vacuity control whose parent has a
   serial selector.

### Task 2 — Binding, shield, and discovery

**Create:** binding, GPIO CMake/Kconfig skeleton.
**Modify:** shield, `zephyr/Kconfig`, root driver CMake/Kconfig.

Implement §§3 and 8 discovery/priority. Run source tests; malformed probes remain
RED until assertions. Normalize only touched files.

### Task 3 — Bottom-half tests then implementation

1. RED tests require exact four signatures, no open/close, no Zephyr include,
   argument forwarding, output-pointer behaviour, common mapping, and
   `-ECOMM`->`-EIO`. M3-B-12 requires the normalizer comment to cite
   `gpio_error_to_status()`, the `common.c:31-32` collapse, and
   `OneWireNoPresence`. This makes status-set changes a visible review obligation;
   it does not claim to prove Rust reachability. The post-M6 M7 follow-up in §15
   owns that mechanical Rust test.
2. Create `pdg_gpio_bottom.{c,h}` per §5 and finish GPIO CMake host context.
3. Run tests and normalize LF.

### Task 4 — Driver safety/flag tests (RED)

Test common-member-first/DT initializer, the exact fifth `ngpios` condition and
diagnostic, and assertion order/location including compatible -> parent status ->
serial presence -> Kconfig -> `ngpios`; source order is normative and emitted
order corroborating. Then test mutex-first init,
readiness/accessor/count/mismatch/clear-never-close/configured-serial success log,
ISR-before-context and context-before-lock for all callbacks, every §7 row
including unknown residual, standard interrupt-mode bits in an assertion-free
direct-dispatch case, `GPIO_INT_WAKEUP` separately, zero aliases by composition,
and no RPC on rejection.

### Task 5 — Driver init and six callbacks

Create `pdg_gpio.c` per §§4, 6, and 7. Run safety tests, all malformed probes,
and valid control. Do not add a parent lock/cache/SPI code. Normalize LF.

### Task 6 — Port failure tests and corrections

In one mandatory coupled suite, test that get re-queries all pins, records
`LegacyAuto` and explicit-input success, skips only output `-EACCES`, converts
monitored `-EBUSY` and a DT-valid/firmware-invalid `-EINVAL` to logged `-EIO`, and
commits output only on complete success. Require no warning for skipped outputs
and require `pin_configure(pin, GPIO_INPUT | GPIO_OUTPUT) ==
-ENOTSUP`. Structure the test so deleting either the read masking or the
input+output rejection breaks the suite. Source-contract checks must also anchor
that `LegacyAuto` calls `set_as_input()` without updating `pin_modes`; M5/M7 own
runtime observation of that side effect.

Also test ascending masked/set/clear puts, zero no-op, mask rejection, ignored
value outside mask, and stop-on-first-failure residue: acknowledged prefix changed,
failed pin indeterminate/lost-ACK-capable, later pins unissued. Require the exact
partial-failure `LOG_ERR` fields. Test toggle no-RPC `-ENOTSUP`, output-init
ordering and previous/HAL-defined-level residue, failed `set_config` as possibly
applied, active-low failure leaving polarity unreliable until successful
reconfigure, and no interleaving within one GPIO operation.

### Task 7 — Non-vacuous build gate

Add GPIO to common guard. Build-slot owner uses plan §4's
`native_sim/native/64` command and extra overlay enabling parent+GPIO in a
baseline-clean sample. Its enabling overlay must set `serial-number` on the parent
or the identity assertion correctly fails. Prove
`CONFIG_{PICO_DE_GALLO,MFD_PICO_DE_GALLO,GPIO,GPIO_PICO_DE_GALLO}=y`; prove
MFD/GPIO embedded TUs participate. `pdg_gpio_bottom.c` is compiled by the
native-simulator Makefile through `target_sources(native_simulator INTERFACE ...)`,
exactly like `common.c`, so it will never appear in `compile_commands.json`.
Prove bottom-half participation with its object file or `nm` on the linked ELF;
a compile-database grep is vacuous and forbidden. Require zero new DT warnings.
Repeat four-sample categories. Known R5 link failures must stay
identical by undefined-symbol count + each build's resolved node path, never
literal ordinal (plan §9.2). Do not run the image/hardware.

### Task 8 — Documentation and final integration

Implement §10, run `mdbook build book` when build slot is free, and search for
false “GPIO absent/default input/atomic/toggle/interrupt” claims. Confirm no SPI,
sample overlay, crates, Cargo, firmware, or wire diff; LF and `git diff --check`;
source/unit/probe/build gates; `cargo test --workspace --locked` (M2 baseline 561
passed, 0 failed, 7 ignored unless HEAD changed intentionally). Integrator makes
one `feat(zephyr): ...` commit with conductor-supplied current-agent trailers;
never push.

## 12. Acceptance criteria and safety gate

Do not call `gallo_*` MCP, `probe-rs`, `cargo run -p gallo`, run a sample, or
build concurrently. This architect performs no build.

Acceptance requires:

1. §1 inventory only; no prohibited path;
2. binding/overlay exact, disabled, no SPI edit;
3. common prefixes and DT common initializer;
4. normative source order compatible -> status -> serial presence -> Kconfig,
   then fifth exact `ngpios` assertion, all above MFD include; compiler diagnostic
   order is corroborating only; missing-serial, zero, 33, and valid probes exist;
5. mutex first and readiness -> accessor -> NULL -> warm exact count; verified no
   second USB round-trip or 300-second child-path exposure;
6. count mismatch logged as local DT/firmware `-EINVAL`, child pointer cleared only;
7. configured parent serial logged on success; duplicate explicit serial residual
   documented;
8. six non-NULL dispatches, optional interrupt fields NULL, toggle capability
   explicitly unavailable with named incompatible generic consumers;
9. direct GPIO wrapper dispatch evidenced by §6 lines;
10. ISR then NULL guards before every lock;
11. exhaustive allow-list and exact direction/pull bytes, with separate standard
    interrupt-mode and `GPIO_INT_WAKEUP` rejection tests;
12. config then put; failed set-config possibly applied; previous/HAL-defined
    output level and Zephyr polarity-metadata residue documented and tested;
13. every query reaches firmware; no firmware pin-state cache;
14. exhaustive §6.2 disposition: LegacyAuto/explicit-input success, only
    output `-EACCES` masked to zero, monitored `-EBUSY` and firmware-invalid index
    logged/normalized to `-EIO`, all aborts leave output untouched; source-defined
    LegacyAuto direction mutation recorded; masking remains coupled to rejection
    of `GPIO_INPUT | GPIO_OUTPUT`; no warning-level hot-path log;
15. masked/set/clear deterministic ascending prefix failure with exact lost-ACK
    residue and actionable `LOG_ERR`; toggle no-RPC `-ENOTSUP`;
16. GPIO `-ECOMM` -> `-EIO` normalizer comment names
    `gpio_error_to_status()`, the `common.c:31-32` collapse, and
    `OneWireNoPresence`; M3-B-12 satisfies M3, while M7 owns the Rust status-set
    test that M3's inventory cannot provide;
17. default priorities 40 < 45 < 50 and no reverse GPIO -> SPI lock path;
18. GPIO in top-level discovery and common-source guard; embedded and bottom
    halves proved non-vacuously, with object/ELF evidence—not
    `compile_commands.json`—for `pdg_gpio_bottom.c`;
19. malformed probes readable; valid probe compiles driver;
20. four samples preserve M2 categories with structural ordinal comparison;
21. README/book/changelog same change; all text LF.

## 13. Alternatives rejected

- Trust shield count: downstream can change it; firmware is authority.
- Parent count accessor: unnecessary duplication over validated handle cache.
- Skip equality to avoid timeout: parent already paid it; fail-safe dominates.
- Cache output for toggle: recreates #104 divergence.
- Get-then-set toggle: get is forbidden on explicit output.
- Flip output to input: glitches, reads pad not latch, needs cached restore data.
- Pre-read masked write: unnecessary and invalid for explicit outputs.
- Put before config: fails for explicit input and only accidentally works legacy.
- Roll back partial failure: prior state is uncached and rollback can fail.
- Change common mapping globally: unrelated I2C/SPI behaviour change.
- Rely on MFD for common guard: hides direct dependency/malformed states.
- Permit selector-less GPIO parent: strict open cannot reveal which attached board
  was selected, so actuation would be unidentifiable and unsafe.
- Roll back failed configure/write: prior state is unavailable and rollback can
  itself lose acknowledgement.
- Warn for every skipped output read: hot-path log flooding; reference controller
  is silent.
- Defer docs to M6: violates AGENTS.md §15.1.
- Add interrupts: contradicts D5 and subscription-risk history.

## 14. Verified conclusions and stale parent citations

The following review points are settled and must not be re-litigated during M3:

1. **R4 cache warmth is verified.** Strict parent open validates and fills the
   handle-shared `OnceLock`; `gallo_num_gpios()` is a warm local read. Exact
   equality and fail-child `-EINVAL` remain correct.
2. **No deadlock is introduced.** GPIO callbacks hold only the GPIO mutex and
   never call SPI. M4 creates one-way SPI -> GPIO ordering with no reverse path.
3. **No-caching is honoured** for firmware level, direction, and pull on every
   path. Zephyr-owned `gpio_driver_data.invert` is logical metadata, not such a
   cache.
4. **Boot coupling is loud.** Once M4 uses `cs-gpios`,
   `spi_context_cs_configure_all()` propagates an unready GPIO port as `-ENODEV`
   (`spi_context.h:309-326`).
5. **The common-source guard is mandatory.** A GPIO-only tree would otherwise
   omit `common.c` while `pdg_gpio_bottom.c` references
   `pdg_common_status_to_errno`.
6. **Scope stays clean:** no SPI or `cs-gpio-indices` change, no `crates/`, wire,
   firmware, version, or lockfile change, and no interrupt implementation.
7. **Priority is fixed:** parent 40 < GPIO 45 < SPI/I2C 50.
8. Parent design §4.4 and plan R4 cite `gpio.h:933` for the pin/
   `port_pin_mask` assertion. That is the interrupt-wrapper assertion; the
   `pin_configure` assertion used by this spec is `gpio.h:1040`. The parent
   documents have a stale citation; M3 does not edit them.

## 15. Findings for the plan and parent design (not actioned in M3)

These findings belong to the conductor's parent plan/design. They are recorded
here but must not expand M3 implementation scope.

1. **M5 blocker — the nominated `spi_loopback` witness contradicts D5.**
   `spi_loopback/src/spi.c:219-240` calls
   `gpio_pin_interrupt_configure_dt()` and `gpio_add_callback()`. M3 leaves both
   slots NULL, so initialization returns `-ENOSYS` before CS-edge verification
   (`gpio.h:901-904,1827-1830`). A strong substitute is polled
   `gpio_basic_api`: its default `PIN_OUT 2` / `PIN_IN 3` topology
   (`test_gpio.h:26,44-45`) exactly matches this project's fitted firmware-index
   2-to-3 jumper.
2. **M4 blocker — upstream CS control discards errors.**
   `spi_context_cs_control()` returns `void` and discards both
   `gpio_pin_set_dt()` results (`spi_context.h:390-418`). Blind use can transfer
   with CS unasserted or report success with CS asserted after `-EIO`, `-EBUSY`,
   or `-EWOULDBLOCK`. M4 must make CS failures observable or fail closed.
3. **Plan R7 is stale.** `gallo_system_reset_subscriptions` is an idempotent
   reconnect-cleanup software path (`pico-de-gallo-ffi/src/lib.rs:706-748`). The
   plan must require a power cycle before M5 or assign one post-strict-open reset
   to a milestone; never insert a global reset into an ordinary pin callback.
   Forward hazard: first `set_config` on monitored pin 2 returns
   `GpioPinMonitored` -> `-EBUSY`
   (`firmware/src/handlers/gpio.rs:270-277`, `common.c:37-42`). M3 init touches no
   pin, but M4 CS initialization on pin 2 would fail and leave SPI not ready.
4. **Parent design §4.4 toggle is infeasible.** Get-then-set cannot toggle an
   explicit output because firmware rejects `gallo_gpio_get` in that mode.
5. **Documentation sequencing conflicts with repository policy.** Plan §1/§3
   assigns book/README parity to M6, but AGENTS.md §15.1 requires behaviour and
   docs in the same change. M3/M4 must each carry local parity; M6 can only
   consolidate.
6. **Plan §3's M3 inventory is short.** Beyond its listed driver files, binding,
   root driver glue, and shield overlay, M3 also adds this specification and
   modifies `zephyr/Kconfig`, `zephyr/README.md`,
   `book/src/interfaces/gpio.md`, and `zephyr/CHANGELOG.md`.
7. **Post-M6 M7 follow-up — make GPIO status reachability mechanically
   checked.** Add a real `#[test]` under `crates/pico-de-gallo-ffi` that pins the
   exact status set reachable from `gpio_error_to_status()`, including that only
   `CommsFailed` can become GPIO `-ECOMM`. M3 cannot own this because its inventory
   forbids `crates/`; M3-B-12's required citation is the bounded interim gate.
8. **Post-M6 M7 follow-up — remove or explicitly accept stateful whole-port
   reads.** Firmware `gpio/get` on `LegacyAuto` calls `set_as_input()` but leaves
   `pin_modes` as `LegacyAuto` (`firmware/src/handlers/gpio.rs:21-37`). Therefore
   M3's required all-pin scan reconfigures unrelated legacy pins and is not
   idempotent in hardware. M7 must choose a wire/firmware direction-query/read
   primitive or explicitly accept and runtime-characterize this behaviour; a
   Zephyr firmware-state cache remains rejected because it can diverge as in
   issue #104. M5 should at minimum observe the side effect on a disposable pin
   after the required power-cycle/reset cleanup.

## 16. Open questions

None. `port_toggle_bits = -ENOTSUP` is accepted: true toggle requires a new
output-latch/toggle wire+firmware+FFI primitive and lockstep release work;
caching is not an alternative. M4 uses `gpio_pin_set_dt()` rather than toggle.
Exact diagnostics and build outcomes are implementation-time gates already
specified above.
