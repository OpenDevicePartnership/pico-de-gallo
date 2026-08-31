# Assembly & Flashing

Getting a working Pico de Gallo on your desk takes two steps:

1. **Get a populated PCB.**
2. **Flash the firmware.**

Step 1 has three options, ordered from easiest to most hands-on.
Step 2 is the same regardless of how you got the board.

## Step 1: Get a Populated PCB

### Option A — Have the PCB house assemble it

Most modern PCB fabrication services (JLCPCB, PCBWay, OSH Park
with their assembly partners, Aisler, etc.) will both fabricate
**and assemble** the board for you. Upload the gerbers, BOM, and
pick-and-place files from the
[`hardware-v*`](https://github.com/OpenDevicePartnership/pico-de-gallo/releases)
release, choose your solder mask color, and a fully-built board
arrives at your door. This is the recommended path — it's cheap
at small quantities, and your time is worth more than the
assembly fee.

> [!NOTE]
>
> Pico de Gallo PCB assembly is not affiliated with any specific
> PCB house. Any cost, mistake, or damage associated with PCB
> fabrication and assembly is your responsibility.

### Option B — Fabricate bare, solder yourself

If you'd rather solder, the board uses through-hole and
medium-pitch components only — there's nothing exotic.

**Order of operations:**

1. **Solder the Pico 2 first.** It's the lowest component on the
   board. Tack one corner pad, check alignment, tack the opposite
   corner, then run a bead along all remaining pads. A bit of
   no-clean flux makes this much easier — solder follows flux onto
   exposed copper.
2. **Right-angle headers next.** Hold them in place with a piece
   of polyimide ("Kapton") tape or a third hand, tack one pin,
   verify the header sits flush, then solder the rest.
3. **Straight headers last.** Same approach — one pin first, check
   alignment, finish the rest.
4. **Clean off the flux** with 99% IPA and an ESD-safe brush in a
   well-ventilated area.

> [!CAUTION]
>
> Isopropyl alcohol is flammable. Don't smoke or have open flames
> near it. Use it in a well-ventilated area.

After cleanup, eyeball the board for solder bridges between
adjacent pins before applying USB power.

### Option C — Skip the PCB, wire a bare Pico 2

The firmware works on a bare Pico 2 too. Wire your peripherals
directly to the RP2350 GPIOs listed in [Pinout &
Connector](./pinout.md). You'll need to provide your own I²C
pull-ups (4.7 kΩ to 3.3 V on SDA and SCL) if you want I²C to work.

## Step 2: Flash the Firmware

The Pico 2 ships with a built-in UF2 bootloader, so you don't need
a programmer, a debug probe, or any extra software. Just a USB
cable.

1. Download the latest `firmware.uf2` from the
   [Releases](https://github.com/OpenDevicePartnership/pico-de-gallo/releases)
   page (look for a tag like `firmware-v0.8.0`). Pick the build
   that matches your board revision:
   - `firmware-rev2.uf2` for the v1.1 board — this is the default
   - `firmware-rev1.uf2` for the v1.0 board — `hw-rev1` is
     [deprecated](./revisions.md#hw-rev1-is-deprecated) but still
     built and published; removal not before 2031-09-01
2. With the Pico 2 **unplugged**, press and **hold** the `BOOTSEL`
   button on top of the module.
3. Plug the USB cable in while still holding `BOOTSEL`, then
   release.
4. A USB mass-storage drive named `RP2350` appears on your host.
5. Drag-and-drop the `firmware.uf2` onto that drive (or
   `cp`/`Copy-Item` from a shell).
6. The drive vanishes; the Pico 2 reboots into the new firmware
   automatically.

That's it — no command-line flashing tool required.

> [!TIP]
>
> If the `RP2350` drive doesn't show up, the Pico 2 didn't enter
> bootloader mode. Unplug, hold `BOOTSEL`, plug back in. Don't
> release `BOOTSEL` until you see the drive.

## Step 3: Verify

Confirm the firmware is alive by running `gallo version`. See
[Verifying Your Device](../getting-started/verify.md) for the
expected output and what each field means.

## When Things Go Wrong

- **Drive doesn't appear in BOOTSEL mode** — try a different USB
  cable (some "charge-only" cables don't carry data) or a
  different USB port.
- **`gallo` can't find the device after flashing** — on Linux you
  may need a udev rule; on Windows the WinUSB driver may need to
  be installed via Zadig. See
  [USB & OS Notes](../getting-started/usb.md).
- **You flashed `hw-rev1` onto a v1.1 board (or vice versa)** —
  no damage done; just re-enter BOOTSEL and flash the right
  build.
