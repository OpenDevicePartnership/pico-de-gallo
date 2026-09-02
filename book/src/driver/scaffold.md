# Scaffolding the crate

This driver is an ordinary Rust library crate. It needs no board support
package, cross toolchain, or custom linker setup.

The recommended starting point is
[drive-rs](https://github.com/OpenDevicePartnership/drive-rs), a
[`cargo-generate`](https://cargo-generate.github.io/cargo-generate/)
template that scaffolds a `no_std`, `embedded-hal` driver crate on top
of the [`device-driver`](https://docs.rs/device-driver) toolkit.
After three prompts, it generates a compiling crate with a register map,
a transport layer, a high-level API, hardware-free mock tests, and a
Pico de Gallo example. No TMP102-specific code is required yet.

To assemble the crate by hand, skip to
[starting from an empty crate](#optional-starting-from-an-empty-crate)
at the end of this chapter.

## Prerequisites

Install `cargo-generate` to run the template. The template's
post-generation hook also uses the `ddc` compiler from
`device-driver-cli` to turn the register manifest into Rust:

```console
$ cargo install cargo-generate
$ cargo install device-driver-cli
```

## Running the template

```console
$ cargo generate --git https://github.com/OpenDevicePartnership/drive-rs template --allow-commands
```

The template asks three questions:

1. Enter the project name in kebab-case. It becomes the crate name, and
    its PascalCase form becomes the driver struct name.
2. Select one or more interfaces from `gpio`, `i2c`, `spi`, and `uart`.
3. Choose `sync`, `async`, or `both`. The answer is resolved at
    generation time, not behind a Cargo feature: `sync` emits only the
    blocking surface, `async` only the async one, and `both` emits the
    two side by side.

TMP102 is a plain I<sup>2</sup>C part with no discrete control lines, so
select `i2c` as the only interface. Select `both` to support blocking and
async users:

```text
🤷   Project Name: tmp102
🤷   Which bus/interface(s) does the device use? [i2c]
🤷   Generate blocking, async, or both APIs? both
```

## Generated files

```text
tmp102/
├── Cargo.toml            # embedded-hal + embedded-hal-async + device-driver runtime
├── README.md
├── device.ddsl           # device-driver manifest, the source of truth
├── src/
│   ├── lib.rs            # #![no_std] crate root
│   ├── error.rs          # generic Error<E> that preserves the underlying bus error
│   ├── registers.rs      # generated from device.ddsl by ddc (don't edit by hand)
│   ├── interface.rs      # I2cInterface: bridges embedded-hal to device-driver
│   └── driver.rs         # the high-level Tmp102 type and its constructors
├── tests/
│   └── integration.rs    # mock-based tests, no hardware required
└── examples/
    └── pico.rs           # runs against real hardware over a Pico de Gallo bridge
```

Because this configuration selects only `i2c`, the template does not
emit code for `gpio`, `spi`, or `uart`. It omits unused interfaces
instead of placing them behind feature flags.

The generated `Cargo.toml` contains the driver dependencies and the
host-side test harness:

```toml
[package]
name = "tmp102"
version = "0.1.0"
edition = "2024"
rust-version = "1.94"
license = "MIT OR Apache-2.0"

[dependencies]
embedded-hal = "1.0"
embedded-hal-async = "1.0"
device-driver = { version = "2.1.0", default-features = false }
defmt = { version = "1", optional = true }

[dev-dependencies]
pico-de-gallo-hal = { version = "0.7", default-features = false }
tokio = { version = "1", features = ["macros", "rt", "rt-multi-thread"] }

[features]
default = []
defmt = ["dep:defmt", "device-driver/defmt"]
```

`embedded-hal` provides the blocking driver surface, while
`embedded-hal-async` provides the async API. Both are ordinary,
non-optional dependencies: the `both` answer was already resolved when
the crate was generated, so there is no `async` feature to turn on or
off. Answering `sync` would have emitted neither `embedded-hal-async`
nor any async code at all. `device-driver` supplies the generated
register accessors. The optional `defmt` dependency supports logging on
the target.

The development dependencies serve the host-side code:
`pico-de-gallo-hal` lets tests and examples communicate with real
hardware, and `tokio` runs async examples and hardware-in-the-loop
tests.

`device-driver` uses `default-features = false`.
The register map is compiled ahead of time by `ddc`, so the crate only
needs the runtime. Users do not compile its procedural macro or manifest
parser.

> [!TIP]
> Keep `pico-de-gallo-hal` in `[dev-dependencies]`, not in normal
> `[dependencies]`. Your end users should depend on your driver crate,
> not on the host-side test harness you used while writing it.

## Build checks

The generated crate compiles and passes its tests before it contains any
TMP102-specific code:

```console
$ cd tmp102
$ cargo test
$ cargo clippy -- -D warnings
```

It's `no_std`, so it builds for a bare-metal target too:

```console
$ rustup target add thumbv7em-none-eabihf
$ cargo build --target thumbv7em-none-eabihf
```

Next, replace the template's placeholder register map with the one from
the TMP102 datasheet, then build the driver API on top of it.

`device.ddsl` is the single source of truth for that map. It is written
in DDSL, device-driver's own specification language, and it names the
generated device itself. There is no separate device-name flag:

```text
device Tmp102Registers {
    default-byte-order: LE,
    register-address-type: u8,
    ...
}
```

After editing it to match the datasheet, regenerate `src/registers.rs`,
which is committed to the crate:

```console
$ ddc build rust -s device.ddsl -o src/registers.rs --rust-defmt-feature=defmt
```

## Optional: starting from an empty crate

The template is optional. To add each piece by hand, start with a fresh
library:

```console
$ cargo new --lib tmp102
    Creating library `tmp102` package
$ cd tmp102
```

Then add the dependencies we know we will need:

```console
$ cargo add embedded-hal
$ cargo add embedded-hal-async
$ cargo add device-driver --no-default-features
$ cargo add --dev pico-de-gallo-hal
$ cargo add --dev tokio -F rt-multi-thread,time,macros
$ cargo install device-driver-cli
```

This produces the same dependency set as the generated crate, but leaves
`src/lib.rs` empty. It does not create `error.rs`, `interface.rs`,
`driver.rs`, mock tests, or `examples/pico.rs`. The following chapters
write those files from scratch, so this route involves more typing and
teaches more of the process.

Both routes produce a library-first, `embedded-hal`-based driver with an
async API and host-side hardware testing. The next chapter describes the
TMP102 registers in the format the generator accepts.
