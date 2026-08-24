# Verifying Your Device

Plug the freshly-flashed Pico de Gallo into your host and run:

```console
$ gallo version
Pico de Gallo FW v0.8.0
Schema v0.4.0
HW revision 2
Capabilities: I2C ✓ | SPI ✓ | UART ✓ | GPIO ✓ | PWM ✓ | ADC ✓ | 1-Wire ✓
```

If you see that block, you're done. Success 🎉.

## What Each Field Means

| Field           | What it tells you                                        |
|-----------------|----------------------------------------------------------|
| `Pico de Gallo FW v...` | The firmware semver. Lockstepped with the wire crate at release time. |
| `Schema v...`   | The wire-protocol schema version. The host crate must understand this. |
| `HW revision`   | `1` if you flashed `hw-rev1` firmware, `2` for `hw-rev2`. |
| `Capabilities`  | Which peripherals **this firmware build** exposes. A `✗` means the endpoint returns `Unsupported`. |

> [!IMPORTANT]
>
> The `HW revision` line reflects the **firmware build**, not the
> PCB you have. If you flashed the wrong build, re-enter BOOTSEL
> and flash the right one. See [Assembly &
> Flashing](../hardware/assembly.md).

## Ping

For a quick round-trip sanity check:

```console
$ gallo ping
Ping OK
```

`ping` sends a random `u32` to the firmware and asserts the echo
matches. If you can ping, USB and the wire protocol are fully
functional.

The payload is randomised per invocation, so a stale or
default-initialised response can't pass as a healthy round trip. If
the round trip completes but the value comes back wrong, you get the
mismatch instead of a generic transport error:

```console
$ gallo ping
Error: ping echo mismatch: sent 0x9f2c41ab, received 0x00000000
```

> [!NOTE]
>
> `ping` and `version` are the only device subcommands that skip the
> up-front schema check described below. That is deliberate: a board
> whose schema doesn't match your `gallo` build should still be able
> to prove its USB path works.

## Listing Multiple Devices

If you have more than one Pico de Gallo connected, `gallo list`
shows them:

```console
$ gallo list
Serial Number         Bus    Address
E6633861A34B8C24      2      14
E6633861A34B9F17      1      8
```

Pick a specific device with `-s` (or `--serial-number`):

```console
$ gallo -s E6633861A34B8C24 version
```

Without `-s`, `gallo` uses the first device it finds — which is
non-deterministic if you have more than one plugged in.

## Schema Mismatch

If the firmware and your host CLI disagree on the wire protocol,
`gallo version` will tell you:

```console
$ gallo version
Error: schema mismatch (firmware v0.5.0, host expects v0.4.x)
```

The fix is to update whichever side is behind. See
[Releases & Compatibility](../internals/releases.md) for the rules
on which versions are compatible.

On the `zephyr` branch, also read the schema-freeze warning there: matching
schema 0.6.1 does not prove that branch-built and released peers are compatible.

## Next

You're up and running. Pick an interface to play with — start
with [I²C](../interfaces/i2c.md) or [GPIO](../interfaces/gpio.md),
or skip to [Writing a Device Driver](../driver/index.md) for the
guided tour.
