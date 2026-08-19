# pico-de-gallo Zephyr Module

This README describes the `pico-de-gallo/zephyr` Zephyr module, including how
to build and run it. It lives here rather than in `pico-de-gallo/book` because
the module is still work-in-progress (and may not live in this repo long term).

`pico-de-gallo/zephyr` is a Zephyr module that lets Zephyr applications drive
real I2C, SPI and GPIO peripherals from `native_sim`. It provides the
`pico_de_gallo` [shield](https://docs.zephyrproject.org/latest/hardware/porting/shields.html),
whose drivers forward Zephyr I2C, SPI and GPIO API calls through the
`pico-de-gallo-ffi` C API to a Pico de Gallo board attached to the host over
USB.

The practical upshot: you write and debug a Zephyr device driver on your
laptop, against the real silicon, without cross-compiling or flashing.

---

## Prerequisites

| Requirement | Notes |
|---|---|
| Zephyr `main` | **Not a release.** `pdg_spi.c` reads `config->cs.setup_ns` / `cs.hold_ns`, which exist only on `main` — they are absent from v4.0.0 through v4.2.0. Only needed for SPI, but `main` is the tested configuration. |
| A 64-bit `native_sim` | Always build `native_sim/native/64`. `zephyr/Kconfig` has `depends on 64BIT`, because `corrosion_set_hostbuild()` forces the rustc host triple. Plain `native_sim` is 32-bit and will not work. |
| Rust 1.90+ and Cargo | The FFI is built from this repository by Corrosion during the Zephyr build. |
| A host C toolchain | `native_sim` compiles with host GCC/Clang. No Zephyr SDK is required. |
| CMake 3.20+ | Corrosion 0.5.2 is used specifically because it still supports Zephyr's 3.20 baseline. |

Verify the Zephyr requirement before anything else — if this prints `0`, you
are not on `main` and the SPI driver will not compile:

```bash
grep -c setup_ns "$ZEPHYR_BASE/include/zephyr/drivers/spi.h"
```

---

## Setting up a workspace

**You do not need to clone this repository inside your Zephyr workspace.** The
samples add the module themselves via `EXTRA_ZEPHYR_MODULES` in their
`CMakeLists.txt`, so the checkout can live anywhere.

If you do not already have a Zephyr workspace, create one on `main`:

```bash
python3 -m venv ~/zephyr-venv
~/zephyr-venv/bin/pip install west

source ~/zephyr-venv/bin/activate
west init -m https://github.com/zephyrproject-rtos/zephyr --mr main ~/zephyrproject
cd ~/zephyrproject
west update
west packages pip --install
west zephyr-export
```

`west update` pulls several GB and takes a while. A partial clone is much
faster and is sufficient for building:

```bash
west update --narrow -o=--filter=blob:none
```

### Environment

Every command below assumes these three:

```bash
export ZEPHYR_BASE=~/zephyrproject/zephyr
export ZEPHYR_TOOLCHAIN_VARIANT=host
export PATH=~/zephyr-venv/bin:$PATH
```

`ZEPHYR_TOOLCHAIN_VARIANT=host` is the one people miss. `native_sim` needs no
Zephyr SDK, but Zephyr still *searches* for one first and fails hard if what it
finds is too old:

```text
CMake Error at .../FindZephyr-sdk.cmake:165 (find_package):
  Could not find a configuration file for package "Zephyr-sdk" that is
  compatible with requested version "1.0".
```

Setting the variant skips that search. `ZEPHYR_BASE` additionally pins which
Zephyr is used if you have stale entries in `~/.cmake/packages/Zephyr`.

---

## Running the I2C sample

`samples/i2c_bridge` reads ambient temperature from a TI TMP117 at address
`0x48` through the Zephyr sensor API.

Wire a TMP117 to the board's I2C pins, attach the board over USB, then:

```bash
cd zephyr/samples/i2c_bridge
west build -p always -b native_sim/native/64 -- -DSHIELD=pico_de_gallo
west build -t run
```

> **Pass `-b` and `-DSHIELD=` explicitly on the first build.** The sample's
> `CMakeLists.txt` sets `BOARD` and `SHIELD` with `set(... CACHE ...)`, but west
> resolves `BOARD` *before* invoking CMake, so those defaults never take effect.
> A bare `west build -t run` in a clean tree therefore fails. Once the build
> directory is configured, plain `west build -t run` works.

With a board and sensor attached, expect:

```text
*** Booting Zephyr OS build v4.4.0-... ***
Temperature: 23.500000 C
Temperature: 23.507812 C
```

Without a board attached you get this instead, which is the normal way this
looks when the bridge is missing:

```text
gallo_init_strict: device not reachable: Failed to find matching nusb device!
<err> mfd_pico_de_gallo: pico-de-gallo: failed to open a Pico de Gallo bridge (default selector). Returning -ENODEV.
<err> i2c_pico_de_gallo: i2c: Pico de Gallo parent pico-de-gallo is not ready. Returning -ENODEV.
<err> TMP11X: i2c device not ready
*** Booting Zephyr OS build v4.4.0-... ***
TMP117 not ready (Pico de Gallo bridge connected?)
```

The MFD parent owns the USB connection, so it is the node that reports the
open failure. Each controller then fails fast against the parent rather than
retrying the open itself.

The sample loops forever. `native_sim` accepts runner arguments directly, so
bound the run or slow it to wall-clock time:

```bash
./build/zephyr/zephyr.exe -stop_at=10   # exit after 10 simulated seconds
./build/zephyr/zephyr.exe -rt           # run at host real time, not as fast as possible
./build/zephyr/zephyr.exe --help        # full list of runner options
```

Simulated time otherwise advances as fast as the host allows, which makes
`k_sleep()`-based sampling loops run far faster than the wall clock.

### The other two samples do not build yet

`samples/spi_bridge` and `samples/combined_i2c_spi_bridge` drive an
IS31FL3743B LED matrix, and that driver is **not** in Zephyr `main` — only
`is31fl319x`, `is31fl3216a` and `is31fl3733` are upstream. Until it lands,
those two samples cannot be built against an upstream checkout. Use
`samples/spi_nor_id` for SPI instead.

---

## Running the SPI sample

`samples/spi_nor_id` identifies a JEDEC SPI NOR flash. Nothing in it is Pico
de Gallo specific: it uses Zephyr's stock `jedec,spi-nor` driver and the
generic flash API, and the bridge is just another SPI controller as far as
they are concerned. Geometry is discovered from the part at runtime over SFDP
(`CONFIG_SPI_NOR_SFDP_RUNTIME`), so the values printed are read from the
device rather than echoed back out of the devicetree.

Wire the flash to SCK/MOSI/MISO and put its chip-select on **GPIO 8**, then:

```bash
cd zephyr/samples/spi_nor_id
west build -p always -b native_sim/native/64 -- -DSHIELD=pico_de_gallo
west build -t run
```

Actual output, against a GigaDevice GD25Q16 holding an iCE40 bitstream:

```text
<inf> spi_pico_de_gallo: Pico de Gallo SPI bridge ready
<inf> spi_nor: nor@0: SFDP v 1.0 AP ff with 2 PH
<inf> spi_nor: PH0: ff00 rev 1.0: 9 DW @ 30
<inf> spi_nor: nor@0: 2 MiBy flash
<inf> spi_nor: PH1: ffc8 rev 1.0: 3 DW @ 60
*** Booting Zephyr OS build v4.4.0-... ***
Flash device ready: nor@0
size:     2048 KiB
write:    1 B block, erased value 0xFF
erase:    65536 B pages, 32 total
@000000:  FF 00 00 FF 7E AA 99 7E 51 00 01 05 92 00 20 62
```

That the driver reaches `ready` at all is most of the result: it read the
JEDEC ID and walked the SFDP parameter tables across the USB bridge during
initialisation, and refuses to initialise if either fails. `PH1: ffc8` is the
GigaDevice vendor parameter header, so the tables really came from this part.

### Keeping it read-only

The sample calls only `flash_read()`. It never calls `flash_write()`,
`flash_erase()` or `flash_ex_op()`.

That alone is not sufficient, because the driver can also write during
initialisation. Every such path is gated on a devicetree property, and
`app.overlay` omits all of them:

| Property | Would allow |
|---|---|
| `has-lock` | `WREN`+`WRSR` to clear block-protect bits |
| `requires-ulbpr` | `ULBPR` |
| `enter-4byte-addr` | `WREN`+`4BA` |
| `has-dpd` | `DPD` / `RDPD` |
| `mxicy-mx25r-power-mode` | `WREN`+`WRSR` on the configuration registers |
| `use-flag-status-register` | `CLRFLSR` |

With those absent, initialisation issues only `RDID` and `RDSFDP`. Verified
on hardware: both status registers read `0x00` before and after, and flash
contents were byte-identical across the run.

**If you add any of those properties, or call a writing API, the driver will
write to your part.** That is the intended behaviour of the driver, not a
defect — just be deliberate about it when the flash holds something you care
about, such as an FPGA bitstream.

---

## Using the drivers in your own application

The shield declares a `pdg0` multi-function-device parent node and, as its
direct children, the `pdg_gpio0`, `pdg_i2c0` and `pdg_spi0` controller nodes.
All four are **disabled by default**. Your application enables `pdg0` *and* the
controllers it needs, then declares its peripherals as ordinary child nodes.

`pdg0` owns the USB connection to one physical board; the controllers borrow
it. An enabled controller whose parent is missing, disabled, or of the wrong
compatible is rejected at build time with an explanatory assertion.

### GPIO

`app.overlay`:

```dts
&pdg0 {
	status = "okay";
	serial-number = "E6614C311B8C9E37";
};

&pdg_gpio0 {
	status = "okay";
};
```

`prj.conf`:

```conf
CONFIG_GPIO=y
```

The pins are the board's firmware GPIO indices 0..3, which are RP2350 GPIO
8..11. `ngpios` must equal the firmware-reported GPIO count; a mismatch is a
local devicetree/firmware configuration error and fails initialization with
`-EINVAL`.

**`serial-number` on `&pdg0` is mandatory for an enabled GPIO child**, and the
build fails with an explanatory assertion without it. GPIO actuates physical
pins, and a selector-less connection cannot report which attached board it
selected, so an unpinned parent would drive unidentifiable hardware. Presence
is not uniqueness: two parents carrying the same explicit serial still alias to
one board. The configured serial is logged when the controller initializes
successfully.

Every operation that reaches hardware is a blocking USB round trip. Calls from interrupt context
return `-EWOULDBLOCK`; a transport failure is `-EIO`.

Multi-pin writes are **not atomic**: they are deterministic ascending, per-pin
round trips. If one fails, the acknowledged prefix definitely changed, the
failed pin is indeterminate because its request may have executed with only the
response lost, and later selected pins were never issued. The driver logs the
operation, the failed pin, the requested mask and value, and the acknowledged
prefix, and does not roll back. Output initialization is likewise two round
trips — configure, then level — so the pin becomes an output before the
requested level arrives and the previous or HAL-defined level may briefly
appear. After a failed `gpio_pin_configure()` the requested direction and pull
may nevertheless have been applied, and logical polarity is unreliable until a
successful reconfiguration.

Reads are scoped to input pins, as Zephyr specifies and as the reference
`gpio_emul` controller behaves: a pin the firmware records as an explicit
output reports a zero bit and the scan continues. That is only sound because
`GPIO_INPUT | GPIO_OUTPUT` is rejected, so a reported zero is provably not an
input pin. `gpio_pin_get()` is not a direction oracle. Direction-query APIs are
also unavailable on this controller (`gpio_pin_is_input()` and
`gpio_pin_is_output()` return `-ENOSYS`).

**Toggle and interrupts are unavailable.** `gpio_pin_toggle()` returns
`-ENOTSUP`, because an explicit output cannot be read back and this driver
deliberately caches no pin state. Interrupt configuration, callback management
and the pending-interrupt query return `-ENOSYS`. Generic toggle consumers,
including blinky, the GPIO shell, the TPS382x watchdog and the LS0xx display,
therefore do not work with this controller.

### I2C

`app.overlay`:

```dts
&pdg0 {
	status = "okay";
};

&pdg_i2c0 {
	status = "okay";

	tmp117: tmp117@48 {
		compatible = "ti,tmp11x";
		reg = <0x48>;
		status = "okay";
	};
};
```

`prj.conf`:

```conf
CONFIG_I2C=y
```

Bus speed comes from the `clock-frequency` property on the controller node
(the shield defaults to `<400000>`).

### SPI

`app.overlay`:

```dts
#include <zephyr/dt-bindings/gpio/gpio.h>

&pdg0 {
	status = "okay";
	/*
	 * REQUIRED for SPI: chip select actuates a physical GPIO, so the
	 * parent must name the board. Use the serial from `gallo list`.
	 */
	serial-number = "REPLACE_WITH_YOUR_PICO_DE_GALLO_SERIAL";
};

&pdg_gpio0 {
	status = "okay";
};

&pdg_spi0 {
	status = "okay";
	cs-gpios = <&pdg_gpio0 2 GPIO_ACTIVE_LOW>,
		   <&pdg_gpio0 0 GPIO_ACTIVE_LOW>;

	my_device: my-device@0 {
		compatible = "vendor,my-device";
		reg = <0>;
		spi-max-frequency = <1000000>;
		status = "okay";
	};
};
```

`prj.conf`:

```conf
CONFIG_SPI=y
```

#### Chip select is standard `cs-gpios`

`cs-gpios` is **required** on every enabled controller. There is no native-CS
fallback: a missing property fails devicetree processing with
`'cs-gpios' is marked as required in 'properties:'`. A child's `reg` has its
standard Zephyr meaning — an index into the `cs-gpios` array. Above, `reg = <0>`
selects firmware GPIO index 2 and `reg = <1>` selects index 0.

The pin cell is a **firmware user GPIO index**, the same namespace the `gallo`
CLI uses, *not* an RP2350 pin number and not a header pin number:

| Firmware index | Board GPIO | RP2350 GPIO | Header pin |
|---|---|---|---|
| 0 | GPIO0 | GPIO8 | 11 |
| 1 | GPIO1 | GPIO9 | 12 |
| 2 | GPIO2 | GPIO10 | — |
| 3 | GPIO3 | GPIO11 | — |

The board's separately silkscreened `SPI_CS` signal (RP2350 GPIO5) is not
claimed by the firmware and cannot be used here.

`GPIO_ACTIVE_LOW` is typical. `GPIO_ACTIVE_HIGH` is permitted, because GPIO
logical polarity determines the physical edge; the SPI operation flag
`SPI_CS_ACTIVE_HIGH` remains rejected with `-ENOTSUP`.

Every entry must target an **enabled** `odp,pico-de-gallo-gpio` controller
under the **same** `odp,pico-de-gallo` parent, and the parent must declare
`serial-number`. Each of these is a build-time failure with an assertion naming
the `cs-gpios` array index:

| Devicetree mistake | Result |
|---|---|
| No `cs-gpios` on an enabled controller | **Devicetree error** — required property |
| Entry targets a foreign (non-PDG) GPIO controller | **Build fails** — compatible assertion |
| Entry targets a disabled PDG GPIO sibling | **Build fails** — status assertion |
| Entry targets a PDG GPIO under a *different* parent | **Build fails** — same-parent assertion |
| Parent has no `serial-number` | **Build fails** — serial assertion |

The cross-parent case is the one that matters most. It is a real, enabled Pico
de Gallo GPIO port — but on a *different physical board*, so chip select would
be driven on one board while data was clocked on another.

#### What this costs

Chip select is no longer part of an atomic firmware batch. An ordinary
successful transceive is **four** USB round trips:

```text
spi/set-config -> gpio/put(assert) -> spi/transfer -> gpio/put(deassert)
```

Each can fail independently. Host death after the assert leaves chip select
asserted; recovery is a fresh session that deasserts the pin, or a power-cycle.
Only RPCs that *return* have defined behaviour — an RPC that never returns
leaves the calling thread pending forever, with no errno, no cleanup, no fault
latch update, and the SPI lock still held. There is no bounded cancellation.

Zephyr collapses a child's `spi-cs-setup-delay-ns` and `spi-cs-hold-delay-ns`
into one `DIV_ROUND_UP(MAX(setup_ns, hold_ns), 1000)` microsecond value and
applies it after the assert and before the deassert. Microsecond waits between
millisecond USB round trips cannot provide meaningful nanosecond timing.

Read-only and write-only transfers become **full-duplex** transfers of
`max(tx_len, rx_len)` bytes, with zero-filled TX or discarded RX respectively.

Declaring a pin in `cs-gpios` makes this driver the sole *driver path* for that
pin's mode; it is **not** an ownership reservation. Your application must give
SPI exclusive ownership of every declared chip-select pin, because a direct
GPIO consumer can reconfigure or drive it between SPI operations and nothing
detects it.

#### Initialization

The controller initializes at `CONFIG_SPI_PICO_DE_GALLO_INIT_PRIORITY`
(default 50), after the GPIO child (45) and the MFD parent (40). It configures
every declared chip-select pin as an **explicit output, inactive**, in
ascending array order — two round trips per pin.

Kconfig cannot check that arithmetic, so runtime readiness is authoritative: an
inverted priority makes the loop see an unready GPIO port and return `-ENODEV`
*before* configuring any pin, rather than actuating the wrong line.

There is no rollback on failure. If a pin fails, earlier entries are
acknowledged inactive, that entry's state is indeterminate, and later entries
were never issued; the device stays not-ready. `-EBUSY` means a firmware GPIO
event subscription owns the pin — reset it explicitly with
`gallo_system_reset_subscriptions()` after a strict open and reinitialize, or
power-cycle.

#### Holding chip select, and the fault latch

`SPI_HOLD_ON_CS` requires `SPI_LOCK_ON` and returns `-ENOTSUP` without it:
holding chip select while another configuration could select a second slave
would leave two peripherals selected at once. A successful hold commits the
received data and keeps the line asserted and the bus locked until
`spi_release()` is called with that same configuration. A thread or process
that never releases strands both the line and software ownership.

Received data is committed only after the deassert that ends the transaction is
acknowledged, or immediately on a successful deliberate hold. A transfer that
succeeds but whose deassert fails returns the deassert errno and does **not**
commit RX; the peripheral may still be selected.

If a forced deassert returns an error, the driver cannot tell whether the line
went inactive, so the controller **latches**. Every later transceive returns
`-EHOSTDOWN` before issuing any configuration, chip-select edge or clocking,
because a previous peripheral may still be selected. Only a `spi_release()`
whose checked deassert succeeds clears it; a failed release still releases
software ownership so nothing wedges, but leaves the latch set so the exact
configuration can be retried. If release cannot clear it, terminate and
reinitialize, deassert the pin explicitly, or power-cycle.

If you would rather not declare a child node, build the `spi_config` yourself
and address the controller directly. Note that `.cs` must be populated — a
zeroed `.cs` means "no GPIO chip select", and this driver will then drive no
line at all:

```c
static const struct device *const bus = DEVICE_DT_GET(DT_NODELABEL(pdg_spi0));

static const struct spi_config cfg = {
	.frequency = 10000000,
	.operation = SPI_WORD_SET(8) | SPI_OP_MODE_MASTER | SPI_TRANSFER_MSB,
	.cs = SPI_CS_GPIOS_DT_SPEC_GET(DT_NODELABEL(my_device)),
};
```

The driver allocates flattened transfer buffers from the system heap. You do
**not** need to set `CONFIG_HEAP_MEM_POOL_SIZE`: enabling the driver
contributes `CONFIG_HEAP_MEM_POOL_ADD_SIZE_PDG_SPI` (8192 by default) and the
kernel sizes the pool automatically. Lower it if you know your transfers are
small.

### Selecting a specific board

Board selection lives on the `pdg0` parent, not on the controllers. With more
than one Pico de Gallo attached, pin each parent to a board with the optional
`serial-number` property:

```dts
&pdg0 {
	status = "okay";
	serial-number = "E6614C311B8C9E37";
};
```

Every controller under that parent inherits the selection and shares the one
USB connection, so an I2C and a SPI child on the same board work without your
code managing that. `serial-number` is **not** accepted on a controller node;
a leftover one fails devicetree processing with an undeclared-property error
naming the node and the binding.

If omitted, the first matching board is used, and the host API cannot report
which one it chose. An omitted serial number is therefore safe **only when
exactly one matching board is attached**. A genuine multi-board setup needs a
unique explicit `serial-number` on every enabled parent; the build-time check
rejects two enabled parents that both omit it, but it verifies presence, not
uniqueness, so two parents sharing one explicit value still silently alias to
one board.

---

## Driver limitations

These are enforced in the drivers and reported as errors, not silently ignored.

### GPIO

| Limitation | Result |
|---|---|
| Enabled GPIO child whose parent has no `serial-number` | **Build fails** with an explanatory assertion |
| `ngpios` outside 1..32 | **Build fails** with an explanatory assertion |
| `ngpios` not equal to the firmware-reported GPIO count | `-EINVAL` at init |
| Any call from interrupt context | `-EWOULDBLOCK` |
| `GPIO_DISCONNECTED` (neither input nor output) | `-ENOTSUP` |
| `GPIO_INPUT \| GPIO_OUTPUT` together | `-ENOTSUP` |
| Single-ended, open-source, open-drain | `-ENOTSUP` |
| Interrupt-mode flags, including `GPIO_INT_WAKEUP` | `-ENOTSUP` |
| Any flag bit outside the supported set | `-ENOTSUP` |
| Both pull-up and pull-down | `-EINVAL` |
| Both output init levels, or an init level without `GPIO_OUTPUT` | `-EINVAL` |
| Mask or pins outside the `ngpios`-derived port mask | `-EINVAL` |
| `gpio_pin_toggle()` / `gpio_port_toggle_bits()` | `-ENOTSUP` |
| Interrupt configure, callback management, pending interrupt | `-ENOSYS` |
| `gpio_pin_get_config()`, `gpio_port_get_direction()` | `-ENOSYS` |
| Reading a pin configured as an explicit output | reports `0`; reads are scoped to input pins |
| Transport failure | `-EIO` |

`GPIO_ACTIVE_LOW` is supported and handled by Zephyr's common GPIO layer.

### I2C

| Limitation | Result |
|---|---|
| 10-bit addressing | `-ENOTSUP` |
| Target/peripheral mode | `-ENOTSUP`; only `I2C_MODE_CONTROLLER` |
| Addresses above 7 bits | `-EINVAL` |
| `I2C_SPEED_HIGH` / `I2C_SPEED_ULTRA` | `-EINVAL`; use standard, fast, or fast-plus |
| Transfers over 4096 bytes | `-EMSGSIZE` |

### SPI

| Limitation | Result |
|---|---|
| `CONFIG_SPI_ASYNC` or `CONFIG_SPI_RTIO` | **Build fails** with an explanatory assertion |
| Enabled controller without `cs-gpios` | **Devicetree error**; the property is required |
| A `cs-gpios` entry on a foreign, disabled, or cross-parent GPIO controller | **Build fails** with an assertion naming the array index |
| Enabled controller whose parent has no `serial-number` | **Build fails** with an explanatory assertion |
| Priority inversion (SPI initializes before its GPIO port) | `-ENODEV` at init, before any pin is configured |
| Peripheral mode | `-ENOTSUP`; only `SPI_OP_MODE_MASTER` |
| Word sizes other than 8-bit | `-ENOTSUP` |
| `SPI_TRANSFER_LSB`, `SPI_MODE_LOOP`, `SPI_HALF_DUPLEX`, `SPI_FRAME_FORMAT_TI` | `-ENOTSUP` |
| `SPI_CS_ACTIVE_HIGH` | `-ENOTSUP`; use `GPIO_ACTIVE_HIGH` in `cs-gpios` |
| `SPI_HOLD_ON_CS` without `SPI_LOCK_ON` | `-ENOTSUP` |
| Chip-select pin explicitly configured as an input | `-EACCES` |
| Chip-select pin under a live GPIO event subscription | `-EBUSY` |
| Transfers over 4096 bytes | `-EMSGSIZE` |
| Controller latched by an unacknowledged chip-select deassert | `-EHOSTDOWN` until a successful `spi_release()` |

Every operation is a blocking USB round trip, so the asynchronous and RTIO
driver ops are deliberately not implemented. Zephyr's SPI subsystem dispatches
`transceive_async()` and `iodev_submit()` without a NULL check, so enabling
either option is refused at build time rather than crashing at runtime. Note
that `CONFIG_SPI_RTIO` is selected transitively by `CONFIG_SENSOR_ASYNC_API`
and by several in-tree SPI sensor drivers, so you can acquire it without asking
for it.

---

## Troubleshooting

**`Could not find a configuration file for package "Zephyr-sdk"`**
Set `ZEPHYR_TOOLCHAIN_VARIANT=host`. `native_sim` does not need the SDK.

**`Failed to find matching nusb device!` / `Returning -ENODEV`**
No board found. The `pdg0` parent reports this; its controllers then fail with
`Pico de Gallo parent ... is not ready`. Check the board is attached and that
you have permission to open it (on Linux, a udev rule or membership of the
right group). If several boards are attached, see *Selecting a specific
board* — the `serial-number` goes on `&pdg0`.

**Build fails with `does not implement iodev_submit()`**
Something enabled `CONFIG_SPI_RTIO`, very likely `CONFIG_SENSOR_ASYNC_API`.
Turn it off; this driver cannot support it. The same applies to
`CONFIG_SPI_ASYNC` and `transceive_async()`.

**`undefined reference to k_malloc`**
An older checkout of this module, before the SPI driver declared its heap
requirement. Update, or set `CONFIG_HEAP_MEM_POOL_SIZE=8192` as a stopgap.

**Build fails with `odp,pico-de-gallo-gpio parent must define serial-number`**
An enabled `pdg_gpio0` requires an explicit `serial-number` on `&pdg0`. See
*GPIO* above for why: GPIO drives physical pins, and an unpinned parent cannot
report which attached board it selected.

**`gpio_pin_toggle()` returns `-ENOTSUP`, or blinky does not work**
Expected. Toggle is unavailable on this controller; use `gpio_pin_set()` with
the level you want. Interrupt-driven GPIO consumers get `-ENOSYS`.

**Devicetree errors about `pdg_i2c0` / `pdg_spi0`**
`-DSHIELD=pico_de_gallo` was not passed, so the nodes do not exist. It is
required on the first build of a clean tree.

**`'cs-gpios' is marked as required in 'properties:'`**
An enabled `pdg_spi0` has no `cs-gpios`. The property is required and there is
no native-CS fallback; add one entry per chip select. This is raised during
devicetree processing, so nothing is compiled.

**Build fails with `cs-gpios entry N must target an odp,pico-de-gallo-gpio
controller ...`**
The named entry points at a foreign GPIO controller, a disabled Pico de Gallo
GPIO sibling, or — the message ending *"under the same odp,pico-de-gallo
parent"* — a Pico de Gallo GPIO controller belonging to a **different board**.
The last one is the dangerous case: chip select would be driven on one board
while data was clocked on another. Also remember to set
`&pdg_gpio0 { status = "okay"; };`.

**Build fails with `odp,pico-de-gallo-spi parent must define serial-number`**
Same reason as the GPIO child below: SPI chip select actuates a physical pin,
so the parent must name the board.

**SPI returns `-EHOSTDOWN`**
The controller latched after a chip-select deassert that was not acknowledged,
so a previous peripheral may still be selected. Call `spi_release()` with the
retained configuration; only a release whose deassert succeeds clears it. If it
cannot be cleared, terminate the process, reinitialize and deassert the pin
explicitly, or power-cycle the board.

**SPI init fails with `-ENODEV` and a "not ready" chip-select message**
Initialization priority inversion. `CONFIG_SPI_PICO_DE_GALLO_INIT_PRIORITY`
must be greater than `CONFIG_GPIO_PICO_DE_GALLO_INIT_PRIORITY`, which must be
greater than `CONFIG_MFD_PICO_DE_GALLO_INIT_PRIORITY`. No pin was configured.

**SPI init fails with `-EBUSY` on a chip-select pin**
A firmware GPIO event subscription owns that pin — often orphaned by a host
process that died. Call `gallo_system_reset_subscriptions()` after a strict
open, then reinitialize, or power-cycle.

**The application builds but the device is never ready**
Check that **both** `&pdg0` and the controller node are `status = "okay"` in
your overlay. The parent and both controllers ship disabled, and a controller
whose parent failed to open reports `Pico de Gallo parent ... is not ready`
rather than a connection error of its own.

**Corrosion or crates.io is unreachable**
The FFI itself builds from this repository and needs no network. Corrosion is
still fetched from GitHub; point at a local checkout with
`-DFETCHCONTENT_SOURCE_DIR_CORROSION=/path/to/corrosion`.

---

## Notes

Applications are not limited to raw I2C and SPI calls. As `samples/i2c_bridge`
shows, higher-level subsystem APIs — sensor, LED, and others — work as long as
the underlying device sits on a bridged bus, because the bridge is an ordinary
Zephyr bus controller as far as the rest of the system is concerned.

Test coverage, Twister metadata, and CI for this module are tracked in
[#109](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/109);
documentation parity with `book/` is tracked in
[#110](https://github.com/OpenDevicePartnership/pico-de-gallo/issues/110).
