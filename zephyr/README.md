# pico-de-gallo Zephyr Module

This README describes the `pico-de-gallo/zephyr` Zephyr module, including how
to build and run it. It lives here rather than in `pico-de-gallo/book` because
the module is still work-in-progress (and may not live in this repo long term).

`pico-de-gallo/zephyr` is a Zephyr module that lets Zephyr applications drive
real I2C and SPI peripherals from `native_sim`. It provides the
`pico_de_gallo` [shield](https://docs.zephyrproject.org/latest/hardware/porting/shields.html),
whose drivers forward Zephyr I2C and SPI API calls through the
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
<err> i2c_pico_de_gallo: Failed to open a Pico de Gallo bridge. Returning -ENODEV.
<err> TMP11X: pdg-i2c device not ready
*** Booting Zephyr OS build v4.4.0-... ***
TMP117 not ready (Pico de Gallo bridge connected?)
```

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

Both controller nodes are declared by the shield and are **disabled by
default**. Your application enables the ones it needs and declares its
peripherals as ordinary child nodes.

### I2C

`app.overlay`:

```dts
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
&pdg_spi0 {
	status = "okay";
	cs-gpio-indices = <2 0>;

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

#### Chip select: `reg` selects, `cs-gpio-indices` maps

A child's `reg` is a chip-select **selector**, not a GPIO number and not the
board's dedicated `SPI_CS` pin. It indexes the controller's
`cs-gpio-indices` array, whose elements are **firmware GPIO indices** — the
same namespace the `gallo` CLI's `spi batch --cs` flag uses, *not* RP2350 pin
numbers.

With `cs-gpio-indices = <2 0>;` above, the mapping is deliberately not the
identity:

| Child `reg` | Firmware GPIO index | Board GPIO | Physical RP2350 GPIO |
|---|---|---|---|
| `<0>` | 2 | GPIO2 | GPIO10 |
| `<1>` | 0 | GPIO0 | GPIO8 |

Firmware indices 0–3 correspond to board GPIO0–GPIO3, which are physical
RP2350 GPIO8–GPIO11. The `SPI_CS` pin on GPIO 5 in the hardware pinout is
**not** driven by the firmware and cannot be used here.

There is **no identity fallback**. Failure modes:

| Situation | Result |
|---|---|
| Controller enabled without `cs-gpio-indices` | `-EINVAL` |
| `reg` at or beyond the array length | `-EINVAL` |
| Mapped index at or beyond the firmware-reported GPIO count | `-EINVAL` |
| Firmware reports zero GPIOs | `-ENODEV` |
| Mapped pin explicitly configured as an input | `-EACCES` |
| Mapped pin under a live GPIO event subscription | `-EBUSY` |

Each of these is logged by the controller with the selector, the mapping
length, the mapped index, and the reported count. That detail matters because
stacked drivers hide it: `jedec,spi-nor` collapses any transfer failure to
`-ENODEV`, so the sample prints only *"Flash not ready"*.

The mapped pin is asserted for the complete firmware batch and left
**deasserted-high** afterwards; the pin's prior direction and level are **not**
restored.

Duplicate indices are permitted, but every child mapped to one index selects
the same physical line. That is safe only when the hardware intentionally
shares selection. Mapping physically distinct peripherals to one index selects
them simultaneously; both may drive MISO, producing bus contention, invalid
returned bytes, and possible electrical over-drive.

Do **not** add `cs-gpios` to the controller node: driving chip select from the
Zephyr side would split one atomic firmware batch across multiple USB
round-trips, losing the batch's chip-select interval guarantee. It is rejected
with `-ENOTSUP`.

If you would rather not declare a child node, build the `spi_config` yourself
and address the controller directly:

```c
static const struct device *const bus = DEVICE_DT_GET(DT_NODELABEL(pdg_spi0));

static const struct spi_config cfg = {
	.frequency = 10000000,
	.operation = SPI_WORD_SET(8) | SPI_OP_MODE_MASTER | SPI_TRANSFER_MSB,
	.slave = 0, /* selector 0 -> cs-gpio-indices[0] */
};
```

Leave `.cs` zeroed, for the same reason you omit `cs-gpios`.

The driver allocates flattened transfer buffers from the system heap. You do
**not** need to set `CONFIG_HEAP_MEM_POOL_SIZE`: enabling the driver
contributes `CONFIG_HEAP_MEM_POOL_ADD_SIZE_PDG_SPI` (8192 by default) and the
kernel sizes the pool automatically. Lower it if you know your transfers are
small.

### Selecting a specific board

With more than one Pico de Gallo attached, pin each controller to a board with
the optional `serial-number` property:

```dts
&pdg_i2c0 {
	status = "okay";
	serial-number = "E6614C311B8C9E37";
};
```

Controllers sharing a `serial-number` share one USB connection internally, so
an I2C and a SPI node on the same board work without your code managing that.

If omitted, the first matching board is used. An omitted serial number is
therefore suitable **only for a single-board setup**: all omitted selectors
share one registry key and so resolve to one board. A genuine multi-board
setup needs a unique explicit `serial-number` on every controller targeting
each board. **Never mix omitted and explicit selectors** — use the same
explicit value on every node that targets a board, or omit it on all of them
when there is only one board.

---

## Driver limitations

These are enforced in the drivers and reported as errors, not silently ignored.

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
| Peripheral mode | `-ENOTSUP`; only `SPI_OP_MODE_MASTER` |
| Word sizes other than 8-bit | `-ENOTSUP` |
| `SPI_TRANSFER_LSB`, `SPI_MODE_LOOP`, `SPI_HALF_DUPLEX`, `SPI_HOLD_ON_CS` | `-ENOTSUP` |
| Zephyr `cs-gpios` (GPIO-controlled chip select) | `-ENOTSUP`; use `cs-gpio-indices` |
| Controller enabled without `cs-gpio-indices` | `-EINVAL` |
| `reg` selector outside the `cs-gpio-indices` array | `-EINVAL` |
| Mapped GPIO index outside the firmware-reported count | `-EINVAL` |
| Firmware reports zero GPIOs | `-ENODEV` |
| Mapped pin explicitly configured as an input | `-EACCES` |
| Mapped pin under a live GPIO event subscription | `-EBUSY` |
| Transfers over 4096 bytes | `-EMSGSIZE` |

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
No board found. Check it is attached and that you have permission to open it
(on Linux, a udev rule or membership of the right group). If several boards are
attached, see *Selecting a specific board*.

**Build fails with `does not implement iodev_submit()`**
Something enabled `CONFIG_SPI_RTIO`, very likely `CONFIG_SENSOR_ASYNC_API`.
Turn it off; this driver cannot support it. The same applies to
`CONFIG_SPI_ASYNC` and `transceive_async()`.

**`undefined reference to k_malloc`**
An older checkout of this module, before the SPI driver declared its heap
requirement. Update, or set `CONFIG_HEAP_MEM_POOL_SIZE=8192` as a stopgap.

**Devicetree errors about `pdg_i2c0` / `pdg_spi0`**
`-DSHIELD=pico_de_gallo` was not passed, so the nodes do not exist. It is
required on the first build of a clean tree.

**The application builds but the device is never ready**
Check the node is `status = "okay"` in your overlay. Both controllers ship
disabled.

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
