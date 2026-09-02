# Publishing the driver

Once the driver feels good locally, spend ten extra minutes making it a
crate other people will trust.

## Cargo metadata

Start with the basics in `Cargo.toml`:

```toml
[package]
name = "tmp102"
version = "0.1.0"
edition = "2024"
description = "embedded-hal driver for the TMP102 I2C temperature sensor"
license = "MIT OR Apache-2.0"
repository = "https://github.com/OpenDevicePartnership/pico-de-gallo"
categories = ["embedded", "hardware-support", "no-std"]
keywords = ["tmp102", "temperature", "sensor", "i2c", "embedded-hal"]
```

If the crate is `no_std`, say so clearly in the README and crate docs.
The generated crate resolves blocking versus async when it is scaffolded
rather than behind a Cargo feature, so state which of the two surfaces it
exposes right next to the first example.

## docs.rs

If your docs need specific features enabled, tell docs.rs explicitly.
Here the only optional features are `defmt` and `hil`, so turning
everything on is the simplest correct answer:

```toml
[package.metadata.docs.rs]
all-features = true
rustdoc-args = ["--cfg", "docsrs"]
```

That avoids the common "works locally, missing items on docs.rs" trap.

## README essentials

At minimum, include:

- what the device is
- which traits the crate implements or expects
- a blocking example
- an async example if you support one
- feature flags (`defmt`, and the `hil` gate from the previous chapter)
- wiring or address-selection notes for `A0`

For this driver, also mention that `pico-de-gallo-hal` is only used for
examples and tests; it should stay in `[dev-dependencies]` so downstream
users do not pay for it.

## `defmt` support

The template already wired this up, and it spans two places that have to
agree. `Cargo.toml` keeps the dependency optional and forwards the
feature to the runtime:

```toml
[dependencies]
defmt = { version = "1", optional = true }

[features]
defmt = ["dep:defmt", "device-driver/defmt"]
```

The other half is the `--rust-defmt-feature=defmt` we passed to `ddc`.
That is what puts `#[cfg(feature = "defmt")]` and
`#[cfg_attr(feature = "defmt", derive(defmt::Format))]` in
`registers.rs`. Rename the Cargo feature without regenerating and the
generated code silently stops matching it.

## Release hygiene

Three last bits of boring professionalism matter a lot:

- keep a `CHANGELOG.md`
- follow semver when you change the public API
- commit `src/registers.rs` and let it into the package

Small drivers live a long time. A clean README, useful crate metadata,
and predictable releases do more for adoption than one more clever type.
