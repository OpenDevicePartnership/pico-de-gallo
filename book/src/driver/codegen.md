# Describe the device for code generation

Hand-writing register boilerplate is dull, repetitive work. TMP102 is
small, but it still benefits from code generation.

Open `device.ddsl` in the crate root and replace the template's
placeholder device with the real TMP102 map. If you started from an
empty crate, create the file now.

DDSL is device-driver's own specification language. One file describes
the whole map, including the name of the device itself.

## 1. The device block

The `device` node names the generated low-level type and sets the
defaults that every object below it inherits.

```text
device Tmp102Registers {
    register-address-type: u8,
    default-byte-order: LE,
    default-access: RW,

    // registers and fieldsets go here
}
```

- `register-address-type` is the type of a register address. TMP102's
  pointer register is 8 bits, so `u8`.
- `default-byte-order` sets the byte order for fieldsets that do not
  declare one.
- `default-access` sets the access for objects that do not declare one.

The compiler works out word boundaries on its own, so `Tlow` becomes
`tlow()` and `SD` becomes `sd()`.

## 2. Register declarations

TMP102 has four registers. Three of them, the reading and the two alert
limits, share a single layout, so declare that layout once as a named
fieldset and point all three registers at it.

```text
    /// Left-justified temperature code, transmitted MSB first.
    ///
    /// 12 significant bits in normal mode, 13 in extended mode.
    fieldset TemperatureCode {
        size-bytes: 2,
        byte-order: BE,

        /// The raw 16-bit register value.
        field value 15:0 -> uint,
    },

    /// Temperature register.
    register Temperature {
        address: 0,
        access: RO,
        fields: TemperatureCode,
    },

    /// T-low register.
    register Tlow {
        address: 2,
        fields: TemperatureCode,
    },

    /// T-high register.
    register Thigh {
        address: 3,
        fields: TemperatureCode,
    },
```

`byte-order: BE` overrides the device default for these three registers
only, because TMP102 sends the temperature high byte first. `access: RO`
on `Temperature` leaves the generated operation without a `write`
method, so the datasheet's read-only rule shows up as a compile error.

## 3. Configuration bitfields

The configuration register is a pile of enums packed into bit ranges.
Its layout is used once, so define the fieldset inline; `fieldset _`
gives the generated type the name of the register it sits in.

```text
    /// Configuration register.
    register Configuration {
        address: 1,
        fields: fieldset _ {
            size-bytes: 2,

            /// Shutdown mode.
            field SD 0 -> _ as enum ShutdownMode {
                Running: 0,
                PowerOff: 1,
            },
            /// Thermostat mode of operation.
            field TM 1 -> _ as enum ThermostatMode {
                Comparator: 0,
                Interrupt: 1,
            },
            /// Alert pin polarity.
            field POL 2 -> _ as enum Polarity {
                ActiveLow: 0,
                ActiveHigh: 1,
            },
            /// Fault queue depth.
            field F 4:3 -> _ as enum FaultQueue {
                One: 0,
                Two: 1,
                Four: 2,
                Six: 3,
            },
            /// Converter resolution.
            field R 6:5 RO -> uint,
            /// One-shot conversion.
            field OS 7 -> bool,
            /// Extended mode.
            field EM 12 -> _ as enum ExtendedMode {
                Disable: 0,
                Enable: 1,
            },
            /// Alert.
            field AL 13 -> bool,
            /// Conversion rate.
            field CR 15:14 -> _ as enum ConversionRate {
                QuarterHz: 0,
                OneHz: 1,
                FourHz: 2,
                EightHz: 3,
            },
        },
    },
```

Bit ranges are written high-to-low and are inclusive, so `15:14` is the
two-bit conversion rate and a bare `0` is the single shutdown bit. The
`_` before `as` lets each field take its base type from the enum it
converts to.

That means:

- `SD` stops being a magic bit and becomes a `ShutdownMode`
- `CR` stops being `0b10` and becomes `ConversionRate::FourHz`
- `R` carries `RO`, so the fieldset gets `r()` but no `set_r()`
- every enum covers all bit patterns of its field, so `sd()` and `cr()`
  return the enum itself rather than a `Result`

> [!TIP]
> Keep the manifest focused on *register truth*, not ergonomic policy.
> It describes what the hardware is; the public driver API decides what
> is pleasant and safe to call.

## 4. Generate `src/registers.rs`

With the manifest in place, generate the low-level register interface:

```console
$ ddc build rust -s device.ddsl -o src/registers.rs --rust-defmt-feature=defmt
```

The `device` node supplies the name, so the generated low-level type is
`Tmp102Registers`.

The generated file is intentionally not the public API. We will treat it
as an implementation detail:

- `registers.rs` knows registers, fields, and access widths
- our hand-written wrapper will know addresses, conversions, and
  human-facing methods

Let the generator write the repetitive code and keep the policy
decisions in the part you maintain.

Next we connect that generated layer to a real I<sup>2</sup>C bus.
