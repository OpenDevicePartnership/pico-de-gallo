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
`samples/i2c_bridge`, or write your own application as below.

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
If omitted, the first matching board is used. **Do not mix omitted and
explicit selectors for the same board** — use the same value on every node that
targets it, or omit it on all of them.

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
| GPIO-controlled chip select | `-ENOTSUP`; use the hardware CS lines |
| Chip-select index outside 0–3 | `-ENOTSUP` |
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
