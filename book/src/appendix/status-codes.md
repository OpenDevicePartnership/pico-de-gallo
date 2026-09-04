# Status Code Reference

Every `gallo_*` FFI function (except the lifecycle calls
`gallo_init`, `gallo_init_with_serial_number`, and `gallo_free`)
returns a `Status` value. For C11/C17, cbindgen emits
`typedef int32_t Status` together with integer constants, rather than making
`Status` a true C `enum` type. To catch appended statuses at compile time,
cast the scrutinee (`switch ((enum Status)x)`), omit any `default:` label
inside the switch, and build with `-Werror=switch`. A post-switch fallback
for an unknown numeric value is intentional and correct.

- **`Ok` (0)** — success.
- **Negative values** — errors, grouped roughly by peripheral.

## Stability

> [!IMPORTANT]
>
> **Status code numeric values are part of the C ABI** and never
> change once shipped. Existing codes are never renumbered or
> reused; new codes are only appended at the bottom of the enum.

If you compile against a header from an older release, codes you
don't recognize will be values your code has never seen. Treat
unknown negative values as "some error" — never assume a number
that doesn't appear in your header means success.

## Complete Status Table

| Name                       | Value | Description                                              |
|----------------------------|------:|----------------------------------------------------------|
| `Ok`                       |     0 | Operation successful                                     |
| `I2cReadFailed`            |    −1 | I²C read failed                                          |
| `I2cWriteFailed`           |    −2 | I²C write failed                                         |
| `InvalidResponse`          |    −3 | Firmware produced an invalid response                    |
| `Uninitialized`            |    −4 | Library was not initialised (NULL context)               |
| `InvalidArgument`          |    −5 | Caller passed an invalid argument                        |
| `PingFailed`               |    −6 | Ping round-trip failed                                   |
| `SpiReadFailed`            |    −7 | SPI read failed                                          |
| `SpiWriteFailed`           |    −8 | SPI write failed                                         |
| `SpiFlushFailed`           |    −9 | SPI flush failed                                         |
| `GpioGetFailed`            |   −10 | GPIO get failed                                          |
| `GpioPutFailed`            |   −11 | GPIO put failed                                          |
| `GpioWaitFailed`           |   −12 | GPIO wait failed                                         |
| `SetConfigFailed`          |   −13 | Set config failed (legacy)                               |
| `VersionFailed`            |   −14 | Version query failed                                     |
| `I2cWriteReadFailed`       |   −15 | I²C write-read failed                                    |
| `I2cSetConfigFailed`       |   −16 | I²C set config failed                                    |
| `SpiSetConfigFailed`       |   −17 | SPI set config failed                                    |
| `I2cNack`                  |   −18 | I²C target did not acknowledge                           |
| `I2cBusError`              |   −19 | I²C bus error                                            |
| `I2cArbitrationLoss`       |   −20 | I²C arbitration loss                                     |
| `I2cOverrun`               |   −21 | I²C data overrun                                         |
| `BufferTooLong`            |   −22 | Buffer exceeds firmware transfer limit                   |
| `I2cAddressOutOfRange`     |   −23 | I²C address out of valid range                           |
| `GpioInvalidPin`           |   −24 | Invalid GPIO pin number                                  |
| `CommsFailed`              |   −25 | USB communication failure                                |
| `I2cScanFailed`            |   −26 | I²C bus scan failed                                      |
| `GpioSetConfigFailed`      |   −27 | GPIO set config failed                                   |
| `GpioWrongDirection`       |   −28 | GPIO pin direction mismatch                              |
| `I2cGetConfigFailed`       |   −29 | I²C get config failed                                    |
| `SpiGetConfigFailed`       |   −30 | SPI get config failed                                    |
| `UartReadFailed`           |   −31 | UART read failed                                         |
| `UartWriteFailed`          |   −32 | UART write failed                                        |
| `UartFlushFailed`          |   −33 | UART flush failed                                        |
| `UartOverrun`              |   −34 | UART receiver overrun                                    |
| `UartBreak`                |   −35 | UART break condition                                     |
| `UartParity`               |   −36 | UART parity error                                        |
| `UartFraming`              |   −37 | UART framing error                                       |
| `UartInvalidBaudRate`      |   −38 | Invalid baud rate                                        |
| `UartSetConfigFailed`      |   −39 | UART set config failed                                   |
| `UartGetConfigFailed`      |   −40 | UART get config failed                                   |
| `PwmSetDutyCycleFailed`    |   −41 | PWM set duty cycle failed                                |
| `PwmGetDutyCycleFailed`    |   −42 | PWM get duty cycle failed                                |
| `PwmEnableFailed`          |   −43 | PWM enable failed                                        |
| `PwmDisableFailed`         |   −44 | PWM disable failed                                       |
| `PwmSetConfigFailed`       |   −45 | PWM set config failed                                    |
| `PwmGetConfigFailed`       |   −46 | PWM get config failed                                    |
| `PwmInvalidChannel`        |   −47 | Invalid PWM channel                                      |
| `PwmInvalidDutyCycle`      |   −48 | Invalid PWM duty cycle                                   |
| `PwmInvalidConfiguration`  |   −49 | Invalid PWM configuration                                |
| `AdcReadFailed`            |   −50 | ADC read failed                                          |
| `AdcGetConfigFailed`       |   −51 | ADC get config failed                                    |
| `AdcConversionFailed`      |   −52 | ADC conversion error                                     |
| `GpioPinMonitored`         |   −53 | Pin is currently subscribed                              |
| `GpioPinNotMonitored`      |   −54 | Pin is not subscribed                                    |
| `GpioSubscribeFailed`      |   −55 | GPIO subscribe failed                                    |
| `GpioUnsubscribeFailed`    |   −56 | GPIO unsubscribe failed                                  |
| `OneWireNoPresence`        |   −57 | 1-Wire: no device responded to reset                     |
| `OneWireBusError`          |   −58 | 1-Wire: bus communication error                          |
| `OneWireReadFailed`        |   −59 | 1-Wire: read failed                                      |
| `OneWireWriteFailed`       |   −60 | 1-Wire: write failed                                     |
| `OneWireSearchFailed`      |   −61 | 1-Wire: ROM search failed                                |
| `DeviceInfoFailed`         |   −62 | Device info query failed                                 |
| `SchemaMismatch`           |   −63 | Schema version mismatch between host and firmware        |
| `LegacyFirmware`           |   −64 | Firmware too old to support `device/info`                |
| `Unsupported`              |   −65 | Peripheral not available on this hardware revision       |
| `I2cBatchFailed`           |   −66 | I<sup>2</sup>C batch transaction failed                  |
| `SpiBatchFailed`           |   −67 | SPI batch transaction failed                             |
| `SpiTransferFailed`        |   −68 | SPI full-duplex transfer failed                          |
| `SystemResetSubscriptionsFailed` | −69 | `system/reset-subscriptions` call failed              |
| `GpioTimeout`              |   −70 | GPIO wait timed out                                      |
| `SpiInvalidCsPin`          |   −71 | SPI chip-select index is outside the reported GPIO range |
| `SpiCsPinUnavailable`      |   −72 | SPI chip-select pin is configured as an input            |
| `SpiCsPinMonitored`        |   −73 | SPI chip-select pin is monitored for GPIO events         |
| `SpiNoGpios`               |   −74 | Device reports zero GPIOs, so no chip-select pin exists  |
| `DeviceInfoTimeout`        |   −75 | `device/info` did not respond within 300 seconds         |
| `CallTimeout`              |   −76 | Device did not answer an RPC within its bound            |

`CallTimeout` (−76) means the request was transmitted but no reply arrived.
It is deliberately distinct from `CommsFailed` (−20): the transport is
healthy and the device very likely is too, so it is not a link failure. It
is also distinct from `DeviceInfoTimeout` (−75), which covers only the
validated metadata fetch and has a far longer bound. The handle stays
usable, so the call may simply be retried — except after a batch, whose
fate is unknown (it may have executed fully, partially, or not at all), so
a blind retry there can repeat side effects.

`SpiInvalidCsPin` (−71) and `SpiNoGpios` (−74) are host-side refusals: the
chip-select was rejected before anything was transmitted, and no pin was
driven. `DeviceInfoFailed` (−62), `SchemaMismatch` (−63),
`LegacyFirmware` (−64) and `DeviceInfoTimeout` (−75) mean the host could
not establish the device-reported GPIO count at all. Those four are never
reported as a chip-select error: a failure to learn the valid range is not
a complaint about the caller's pin.

## Source of Truth

The enum lives in
[`crates/pico-de-gallo-ffi/src/lib.rs`](https://github.com/OpenDevicePartnership/pico-de-gallo/blob/main/crates/pico-de-gallo-ffi/src/lib.rs)
and is mirrored into the generated `pico_de_gallo.h` by cbindgen.
If a code is missing from this table after a release, file an
issue — that's a documentation bug.
