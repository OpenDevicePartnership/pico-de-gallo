# USB & OS Notes

The Pico de Gallo firmware uses a generic WinUSB-compatible
descriptor, so most operating systems pick it up without a custom
driver. The notes below cover the cases where you need to nudge
the OS.

## Linux

Out-of-the-box, libusb (and therefore `nusb`) requires root to
open arbitrary USB devices. To let your regular user account talk
to Pico de Gallo, drop a udev rule:

```text
# /etc/udev/rules.d/99-pico-de-gallo.rules
SUBSYSTEM=="usb", ATTR{idVendor}=="045e", ATTR{idProduct}=="067d", MODE="0666"
```

Then reload udev:

```console
$ sudo udevadm control --reload-rules
$ sudo udevadm trigger
```

Unplug and replug the device. `gallo version` should now work
without `sudo`.

> [!NOTE]
>
> The device enumerates with VID `045e` (Microsoft) and PID
> `067d`, defined by the firmware (`MICROSOFT_VID` /
> `PICO_DE_GALLO_PID` in `pico-de-gallo-internal`). They may
> change across firmware versions, so avoid hard-coding them in
> long-lived tooling.

## Windows

The firmware advertises a Microsoft OS 2.0 descriptor that tells
Windows to bind the WinUSB driver automatically. The first time
you plug in a Pico de Gallo, you may see a brief "installing
device" notification — that's normal. After that, `gallo` works
without any extra setup.

If for some reason WinUSB doesn't bind (e.g., a stale Zadig
override, or driver-signing policy on a corporate machine), use
[Zadig](https://zadig.akeo.ie/) to manually install the WinUSB
driver against the Pico de Gallo interface.

> [!IMPORTANT]
>
> Zadig lists **two** Pico de Gallo interfaces, because the device
> exposes a second vendor-class interface to carry its WebUSB
> descriptor. Bind WinUSB to **interface 0** — the one with the bulk
> endpoints. Interface 1 has no endpoints, and binding it leaves
> `gallo` unable to reach the device.

## macOS

No extra setup. macOS picks the device up automatically.

If `gallo list` returns nothing, check System Information →
USB and confirm the device enumerates. If it shows up there but
`gallo` can't find it, you might have a code-signing issue with a
locally-built `gallo` binary — try the pre-built release artifact.

## Browsers (WebUSB)

The firmware advertises a WebUSB platform capability in its BOS
descriptor, along with a landing-page URL. Chrome and Edge use this to
show a notification pointing at
<https://balbi.sh/pico-de-gallo/> when you plug the device in.

Two browser requirements are worth knowing up front, because neither is
something the firmware can influence:

- **Secure context.** `navigator.usb` only exists on pages served over
  HTTPS, or on `localhost`.
- **User gesture.** `navigator.usb.requestDevice()` must be called from
  a click or keypress handler. There is no way to connect
  automatically on page load; the user picks the device from a browser
  dialog every time a new origin asks.

```js
const device = await navigator.usb.requestDevice({
  filters: [{ vendorId: 0x045e, productId: 0x067d }],
});
await device.open();
// The OS does not always configure the device during enumeration.
if (device.configuration === null) await device.selectConfiguration(1);
await device.claimInterface(0);
```

Interface `0` is the vendor-specific interface carrying the two bulk
endpoints that the RPC protocol uses. The device also exposes a second,
endpoint-less vendor interface that exists only to carry the WebUSB
capability descriptor — do not claim it.

> [!NOTE]
>
> These descriptors are a convenience, not a gate. WebUSB can talk to
> any vendor-class device, so a browser could already reach Pico de
> Gallo without them. What they add is the landing-page notification
> and explicit signalling that the device is WebUSB-aware.

The same OS-level permissions still apply: a Linux udev rule is
required (see above), and on Windows the device must be bound to
WinUSB, which the Microsoft OS 2.0 descriptor handles automatically.

## Troubleshooting

- **`gallo: device not found`** — Is the device plugged in? Did
  you flash firmware? Try `gallo list`.
- **`Permission denied` on Linux** — udev rule missing or not
  reloaded. See above.
- **`gallo version` succeeds but `gallo i2c scan` hangs** — the
  bus has no pull-ups, or your peripheral is clock-stretching
  forever. Add 4.7 kΩ pull-ups (v1.0 boards lack them on-board).
- **Device disappears after a write** — likely a brown-out from
  trying to source too much current through the on-board 3.3 V
  rail. Power the peripheral externally.

See also: [Troubleshooting](../appendix/troubleshooting.md) for
the full list.
