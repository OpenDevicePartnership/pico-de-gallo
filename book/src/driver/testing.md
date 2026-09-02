# Testing with `pico-de-gallo-hal`

This is the whole payoff.

Because the driver is written against `embedded-hal`, and because
`pico-de-gallo-hal` implements those traits on top of a USB-connected
Pico de Gallo board, we can run the driver on a host machine against a
*real* TMP102.

No mocks. No firmware flashing loop. No sacrificial example binary that
only exists so you can manually test one register read.

## A hardware-in-the-loop test

Our public API is async, so the test is too:

```rust,no_run
#[cfg(feature = "hil")]
#[tokio::test]
async fn tmp102_reports_a_plausible_temperature() {
    use pico_de_gallo_hal::Hal;
    use tmp102::{Celsius, Tmp102, A0};

    let hal = Hal::new();
    let i2c = hal.i2c();
    let mut sensor = Tmp102::new(i2c, A0::Gnd);

    let Celsius(temp) = sensor.temperature().await.unwrap();

    assert!((-40.0..=125.0).contains(&temp));
}
```

That range is intentionally broad: it matches the device's operating
range and keeps the test robust across different lab environments.

The blocking siblings we add in the next chapter work the same way with
a plain `#[test]` and no `.await`.

## Why this is special

Most embedded-driver test setups force you into one of two extremes:

- pure mocks, which are fast but can only prove your expectations about
  bus traffic
- target-hardware tests, which are realistic but slow and usually drag
  in flashing, runners, probes, and target-specific setup

Pico de Gallo sits in a very productive middle ground:

- the sensor is real
- the electrical path is real
- the I<sup>2</sup>C transactions are real
- the test still runs from your normal host-side Rust test harness

That means you can put a USB-connected board on a CI runner and execute
real hardware-in-the-loop tests with `cargo test`.

## Gate HIL tests behind a feature

Not every CI environment has hardware attached, so gate the test behind
an opt-in feature:

```rust,noplayground
#[cfg(feature = "hil")]
#[tokio::test]
async fn tmp102_reports_a_plausible_temperature() {
    // ...
}
```

And in `Cargo.toml`:

```toml
[features]
default = []
defmt = ["dep:defmt", "device-driver/defmt"]
hil = []
```

Now normal CI can run `cargo test`, while the hardware-backed runner can
run:

```console
$ cargo test --features hil
```

> [!TIP]
> Keep `pico-de-gallo-hal` in `[dev-dependencies]`. End users of your
> published crate do not need the host transport layer just because you
> used it for tests.

## Mocks still have a place

`pico-de-gallo-hal` is not a replacement for `embedded-hal-mock`.
Instead, the two complement each other nicely:

- use `embedded-hal-mock` for pure logic tests, edge cases, and exact
  bus-sequence assertions
- use Pico de Gallo for integration tests against a real sensor

That combination is hard to beat:

- mocks keep your fast inner loop fast
- HIL tests catch the mistakes that only show up with actual hardware

For driver authors, that is exactly why `pico-de-gallo-hal` exists.
