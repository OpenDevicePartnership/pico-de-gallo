# Implementing `RegisterInterface` and `AsyncRegisterInterface`

At this point we have generated code, but it still has no idea how to
reach real hardware. That is our job.

`Tmp102Registers` wants a tiny transport object that knows how to read
and write a register by address. For TMP102 that transport is just
I<sup>2</sup>C plus the selected device address.

## Encode the legal addresses

TMP102 does not live at an arbitrary address. Datasheet table 6-4 gives
us four legal values, so we should model exactly those four values.

```rust,noplayground
/// Logic level wired onto the TMP102 A0 pin.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum A0 {
    /// A0 tied to ground (`0x48`).
    #[default]
    Gnd,
    /// A0 tied to V+ (`0x49`).
    Vplus,
    /// A0 tied to SDA (`0x4a`).
    Sda,
    /// A0 tied to SCL (`0x4b`).
    Scl,
}

impl From<A0> for u8 {
    fn from(a0: A0) -> Self {
        match a0 {
            A0::Gnd => 0x48,
            A0::Vplus => 0x49,
            A0::Sda => 0x4a,
            A0::Scl => 0x4b,
        }
    }
}
```

That alone eliminates a whole class of mistakes. The caller can no
longer accidentally pass `0x52` and then wonder why the driver never
sees an ACK.

## Wrap the bus

Now create the transport type that the generated layer will sit on top
of:

```rust,noplayground
use embedded_hal::i2c::I2c;
use embedded_hal_async::i2c::I2c as AsyncI2c;

struct Interface<I2C> {
    i2c: I2C,
    addr: u8,
}

impl<I2C> Interface<I2C> {
    fn new(i2c: I2C, a0: A0) -> Self {
        Self {
            i2c,
            addr: a0.into(),
        }
    }
}
```

`RegisterInterfaceBase` declares the two associated types that the
blocking and async traits are both built on:

```rust,noplayground
use device_driver::RegisterInterfaceBase;
use embedded_hal::i2c::ErrorKind;

impl<I2C> RegisterInterfaceBase for Interface<I2C> {
    type Error = ErrorKind;
    type AddressType = u8;
}
```

`AddressType` has to match the `register-address-type` we set in the
manifest. `ErrorKind` comes from `embedded-hal`, and
`embedded-hal-async` re-exports that same type, so one impl covers both
sides and needs no bound on `I2C`.

The job of `Interface` is deliberately boring: take register operations
from generated code and translate them into real I<sup>2</sup>C
transactions.

## Blocking register access

For blocking `embedded-hal`, that translation looks like this:

```rust,noplayground
use device_driver::{FieldsetMetadata, RegisterInterface};
use embedded_hal::i2c::{Error, I2c};

impl<I2C: I2c> RegisterInterface for Interface<I2C> {
    fn write_register(
        &mut self,
        address: Self::AddressType,
        data: &mut [u8],
        _metadata: &FieldsetMetadata,
    ) -> Result<(), Self::Error> {
        let mut buf = [0u8; 3];
        buf[0] = address;
        buf[1..].copy_from_slice(data);

        self.i2c.write(self.addr, &buf).map_err(|e| e.kind())
    }

    fn read_register(
        &mut self,
        address: Self::AddressType,
        data: &mut [u8],
        _metadata: &FieldsetMetadata,
    ) -> Result<(), Self::Error> {
        self.i2c
            .write_read(self.addr, &[address], data)
            .map_err(|e| e.kind())
    }
}
```

TMP102 keeps this pleasantly simple: one pointer byte, then two bytes of
payload.

Both methods take `data` as `&mut [u8]`. The write side gets a mutable
buffer so that an interface can rearrange bytes in place before sending,
which we do not need here. `_metadata` carries the fieldset's byte
order, which we can also ignore because the framing is the same for
every TMP102 register.

> [!NOTE]
> The fixed `[u8; 3]` buffer is TMP102-specific. For a bigger device with
> wider registers, size the stack buffer to your largest write or switch
> to a small growable buffer type.

## Async register access

The async version is the same idea with `.await` in the obvious places.
Because the template resolved `both` at generation time, this impl sits
next to the blocking one with no `cfg` between them:

```rust,noplayground
use device_driver::AsyncRegisterInterface;

impl<I2C: AsyncI2c> AsyncRegisterInterface for Interface<I2C> {
    async fn write_register(
        &mut self,
        address: Self::AddressType,
        data: &mut [u8],
        _metadata: &FieldsetMetadata,
    ) -> Result<(), Self::Error> {
        let mut buf = [0u8; 3];
        buf[0] = address;
        buf[1..].copy_from_slice(data);

        self.i2c.write(self.addr, &buf).await.map_err(|e| e.kind())
    }

    async fn read_register(
        &mut self,
        address: Self::AddressType,
        data: &mut [u8],
        _metadata: &FieldsetMetadata,
    ) -> Result<(), Self::Error> {
        self.i2c
            .write_read(self.addr, &[address], data)
            .await
            .map_err(|e| e.kind())
    }
}
```

So far, so good. The generated code can finally talk to the sensor.

## Put the generated layer behind a real driver type

Build this as it stands and the compiler will report that fields and
methods on `Tmp102Registers` are never used. It is right: we generated a
low-level API and nothing calls it yet.

A minimal wrapper clears those warnings and gives the chapter a place to
keep growing:

```rust,noplayground
mod registers;

use registers::{ShutdownMode, Tmp102Registers};

pub struct Tmp102<I2C> {
    inner: Tmp102Registers<Interface<I2C>>,
    extended_mode: bool,
}

impl<I2C> Tmp102<I2C> {
    pub fn new(i2c: I2C, a0: A0) -> Self {
        Self {
            inner: Tmp102Registers::new(Interface::new(i2c, a0)),
            extended_mode: false,
        }
    }
}

impl<I2C: I2c> Tmp102<I2C> {
    pub fn raw_temperature_register(&mut self) -> Result<[u8; 2], ErrorKind> {
        Ok(self.inner.temperature().read()?.into())
    }

    pub fn configure_shutdown_bit(&mut self, shutdown: bool) -> Result<(), ErrorKind> {
        self.inner.configuration().modify(|reg| {
            reg.set_sd(if shutdown {
                ShutdownMode::PowerOff
            } else {
                ShutdownMode::Running
            })
        })
    }

    pub fn set_low_limit_raw(&mut self, raw: [u8; 2]) -> Result<(), ErrorKind> {
        self.inner.tlow().write(|reg| *reg = raw.into())
    }

    pub fn set_high_limit_raw(&mut self, raw: [u8; 2]) -> Result<(), ErrorKind> {
        self.inner.thigh().write(|reg| *reg = raw.into())
    }
}

impl<I2C: AsyncI2c> Tmp102<I2C> {
    pub async fn raw_temperature_register_async(&mut self) -> Result<[u8; 2], ErrorKind> {
        Ok(self.inner.temperature().read_async().await?.into())
    }
}
```

Now `inner.temperature()`, `inner.configuration()`, `inner.tlow()`, and
`inner.thigh()` are all exercised through `Tmp102`, so those dead-code
warnings disappear for the right reason: the generated layer is no
longer orphaned.

This wrapper is still too raw for real use, but that is fine. The next
step is where the driver starts feeling like a crate you would actually
publish.
