# Getting Started

There are just a few steps needed in order to get a functioning *Pico
de Gallo* on your hands:

1. Fabricate the landing board
2. Solder components
3. Flash latest Firmware

We will look at the each in the following sections, but first we need
to discuss materials and equipment needed to assemble a *Pico de
Gallo* PCB.

## Equipment and materials

1. Soldering iron or soldering station
2. Solder wire spool
3. Soldering iron tip cleaner (either sponge or brass will work)
4. Good quality no-clean liquid flux
5. Raspberry pi Pico 2 board
6. *Pico de Gallo* landing board
7. ESD-safe cleaning brushes
8. (Optional) PCB holder or *helping hands*
9. (Optional) Soldering iron tip tinner

## PCB Fabrication

There are several suitable PCB fabrication services which can consume
our Gerbers and produce high quality PCBs delivered to your door. You
are free to choose whichever your want.

**DISCLAIMER: we are not responsible for anything related to PCB
fabrication. Any costs, mistakes, damages associated with it are your
responsibility.**

Our gerbers are hosted in the
[Releases](https://github.com/OpenDevicePartnership/pico-de-gallo/releases)
page for our [Github
repository](https://github.com/OpenDevicePartnership/pico-de-gallo). For
convenience, [here's the
link](https://github.com/OpenDevicePartnership/pico-de-gallo/releases/tag/hardware-v0.1.0)
to the latest Gerber release at the time of this writing.

For most of these PCB fabrication services, you can just upload the
`gerbers.zip` file downloaded from our Hardware release above, select
your desired silk screen color, solder mask color, and surface finish
and place the order. Once the boards are ready, they should be mailed
to your door.

## Soldering Components

> [!TIP]
>
> Soldering takes practice. Before starting make sure you have all
> necessary equipment and materials around you and no unnecessary
> obstructions.
>
> Make sure that the tip of your soldering iron touches both
> components being soldered together. That is, if you're soldering the
> Pico 2 to the landing board, the iron tip should touch both the pico
> 2 and the landing board's pad at the same time.
>
> Let the area heat up for a couple seconds before introducing the
> solder wire.

When soldering, it's wise to start with the lowest height components
first. In the case of *Pico de Gallo*, that will be the Pico 2
itself.

### Soldering the Pico 2

Place the Pico 2 onto the landing board aligning the edge of
the Pico 2 board to the edge of the landing board as shown below:

**TODO INSERT IMAGE: pico placed on landing board**

Start by soldering one and only one corner pad:

**TODO INSERT IMAGE: pico with one corner soldered**

This will allow you to reflow &mdash; that is, remelt &mdash; the
solder and move the board around in case it doesn't exactly align with
the other pads.

> [!TIP]
>
> Solder flux makes the process of soldering these components a lot
> easier. Grab some no-clean flux, spread around the pads and the
> solder will be *pulled onto* the pads by surface tension.
>
> As it turns out, solder really likes to stick to exposed copper and
> truly despises solder mask.

Once you align the board correctly &mdash; see the example below
&mdash;, you can move on to the opposite corner. This will help
prevent any further movement during the remainder of the soldering
process.

**TODO INSERT IMAGE: two pads soldered**

At this point you are ready to solder all remaining pads. The landing
board and pico 2 should be strongly attached to one another as shown
below:

**TODO INSERT IMAGE: pico 2 soldered to landing board**

### Soldering right angle headers

Next, you should move on to the right angle headers. These a bit
tricky to solder, but with some patience and, perhaps, a piece of
Polyimide Electrical Tape &mdash; also known as *Kapton Tape* &mdash;
to hold the headers while you solder, should make the process a little
easier.

Similarly to how we soldered the pico 2 to the landing board, here,
too, we want to solder one pin only and make sure everything remains
aligned before proceeding. Below you can see the process as a sequence
of images.

**TODO INSERT IMAGE: right angle header 1**

**TODO INSERT IMAGE: right angle header 2**

**TODO INSERT IMAGE: right angle header 3**

Finally, we can solder all the remaining headers as they are all the
same height.

### Soldering remaining headers

Once again, solder one pin on each header:

**TODO INSERT IMAGE: header 1**

If any of them hardened *crooked* or not flush with the landing board,
fix it before proceeding. Continue by soldering one more pin on the
opposite corner of each header.

**TODO INSERT IMAGE: header 2**

Finish by soldering all remaining pins.

**TODO INSERT IMAGE: header 2**

### Finishing touches

Now is a good time to visually inspect the board. Make sure there are
no *bridged* pins or pads &mdash; that is, no solder shorting any
neighboring pins to each other &mdash; as that could cause unexpected
problems if you were to plug the USB cable.

After verifying that the board looks correct, now is the time to scrub
the excess flux residue off the board. This is achieved with ESD safe
brushes and 99% Isopropyl Alcohol.

> [!CAUTION]
>
> Isopropyl Alcohol (IPA) is highly flammable. Care should be taken to
> not inhale IPA fumes. Make sure you are in a well-ventilated area
> while cleaning the board with IPA.
>
> IPA can also some dryness upon skin contact.

## Flashing Firmware

With an assembled *Pico de Gallo*, the next step is to flash the
latest Firmware. In order to do so, we download the latest
`firmware.uf2` from the [Releases](https://github.com/OpenDevicePartnership/pico-de-gallo/releases)
page from our [Github
repository](https://github.com/OpenDevicePartnership/pico-de-gallo). At
the time of this writing, that is [Fimware
v0.4.1](https://github.com/OpenDevicePartnership/pico-de-gallo/releases/tag/firmware-v0.4.1).

It's very easy to update firmware on *Pico de Gallo*, simply:

1. Insert the USB cable to pico 2's micro-USB port
2. Press and hold the `BOOTSEL` button
3. Insert the other end of the USB cable to an available USB port on
   your computer

A new USB drive named `RP2350` should show on your computer. Simply
drag and drop the `firmware.uf2` file to this new USB drive. *Pico de
Gallo* will automatically disconnect and reconnect with the new
Firmware.

## Verifying Firmware Version

Finally, let's verify that the firmware running on the device matches
our expectation. Grab the `pico-de-gallo-app` suitable for your host
Operating System from our
[Releases](https://github.com/OpenDevicePartnership/pico-de-gallo/releases)
page. At the time of this writing, the latest version is [Application
v0.2.1](https://github.com/OpenDevicePartnership/pico-de-gallo/releases/tag/application-v0.2.1). We
currently support `x86_64` and `Aarch64` for both Windows and Linux,
and `Aarch64` for macOS.

Once you have the correct binary, run the `version` command:

```console
$ gallo version
Pico de Gallo FW v0.4.1
```

Success 🎉. With this completed we can get familiarized with the other
parts of the *Pico de Gallo* ecosystem and write a driver for the
`tmp102` temperature sensor.
