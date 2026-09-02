//! GPIO endpoint handlers.

use defmt::debug;
use embassy_rp::gpio::{Level, Pull};
use pico_de_gallo_internal::{
    GpioDirection, GpioError, GpioGetRequest, GpioGetResponse, GpioPull, GpioPutRequest, GpioPutResponse,
    GpioSetConfigurationRequest, GpioSetConfigurationResponse, GpioState, GpioSubscribeRequest, GpioSubscribeResponse,
    GpioUnsubscribeRequest, GpioUnsubscribeResponse, GpioWaitRequest, GpioWaitResponse,
    SystemResetSubscriptionsResponse,
};
use postcard_rpc::header::VarHeader;

use crate::context::{Context, NUM_GPIOS, PinMode};
use crate::{GPIO_MONITOR_ARMED, GPIO_MONITOR_RETURN, GPIO_MONITOR_START, GPIO_MONITOR_STOP};

/// Helper macro to get a GPIO pin by index for input operations.
/// In `LegacyAuto` mode, auto-switches to input. In `ExplicitInput` mode,
/// uses the pin as-is. In `ExplicitOutput` mode, returns `WrongDirection`.
/// Returns `PinMonitored` if the pin is currently subscribed for event monitoring.
macro_rules! gpio_for_input {
    ($context:expr, $pin:expr) => {{
        let idx = usize::from($pin);
        let mode = *$context.pin_modes.get(idx).ok_or(GpioError::InvalidPin)?;
        let gpio = $context
            .gpios
            .get_mut(idx)
            .ok_or(GpioError::InvalidPin)?
            .as_mut()
            .ok_or(GpioError::PinMonitored)?;
        match mode {
            PinMode::LegacyAuto => gpio.set_as_input(),
            PinMode::ExplicitInput => {}
            PinMode::ExplicitOutput => return Err(GpioError::WrongDirection),
        }
        gpio
    }};
}

/// Handler for `gpio/get` — reads the current logic level of a pin.
pub(crate) async fn gpio_get_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioGetRequest,
) -> GpioGetResponse {
    let gpio = gpio_for_input!(context, req.pin);
    debug!("gpio get: pin={=u8}", req.pin);
    match gpio.get_level() {
        Level::Low => Ok(GpioState::Low),
        Level::High => Ok(GpioState::High),
    }
}

/// Handler for `gpio/put` — sets a GPIO pin to the requested level.
pub(crate) async fn gpio_put_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioPutRequest,
) -> GpioPutResponse {
    let idx = usize::from(req.pin);
    let mode = *context.pin_modes.get(idx).ok_or(GpioError::InvalidPin)?;
    let gpio = context
        .gpios
        .get_mut(idx)
        .ok_or(GpioError::InvalidPin)?
        .as_mut()
        .ok_or(GpioError::PinMonitored)?;

    match mode {
        PinMode::LegacyAuto => gpio.set_as_output(),
        PinMode::ExplicitOutput => {}
        PinMode::ExplicitInput => return Err(GpioError::WrongDirection),
    }

    let level = match req.state {
        GpioState::Low => Level::Low,
        GpioState::High => Level::High,
    };

    debug!("gpio put: pin={=u8} level={=u8}", req.pin, level as u8);
    gpio.set_level(level);

    Ok(())
}

/// Handler for `gpio/wait-high` — blocks until the pin goes high.
///
/// `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT). A value of
/// `0` selects that ceiling; it no longer means "wait forever". Returns
/// [`GpioError::Timeout`] on expiry.
pub(crate) async fn gpio_wait_for_high_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioWaitRequest,
) -> GpioWaitResponse {
    let gpio = gpio_for_input!(context, req.pin);
    debug!(
        "gpio wait_for_high: pin={=u8} timeout_ms={=u32}",
        req.pin, req.timeout_ms
    );
    crate::progress::bounded(req.timeout_ms, gpio.wait_for_high())
        .await
        .map_err(|_| {
            defmt::warn!(
                "gpio_wait_for_high timeout (pin={=u8}, ms={=u32})",
                req.pin,
                req.timeout_ms
            );
            GpioError::Timeout
        })
}

/// Handler for `gpio/wait-low` — blocks until the pin goes low.
///
/// `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT). A value of
/// `0` selects that ceiling; it no longer means "wait forever". Returns
/// [`GpioError::Timeout`] on expiry.
pub(crate) async fn gpio_wait_for_low_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioWaitRequest,
) -> GpioWaitResponse {
    let gpio = gpio_for_input!(context, req.pin);
    debug!(
        "gpio wait_for_low: pin={=u8} timeout_ms={=u32}",
        req.pin, req.timeout_ms
    );
    crate::progress::bounded(req.timeout_ms, gpio.wait_for_low())
        .await
        .map_err(|_| {
            defmt::warn!(
                "gpio_wait_for_low timeout (pin={=u8}, ms={=u32})",
                req.pin,
                req.timeout_ms
            );
            GpioError::Timeout
        })
}

/// Handler for `gpio/wait-rising` — blocks until a rising edge.
///
/// `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT). A value of
/// `0` selects that ceiling; it no longer means "wait forever". Returns
/// [`GpioError::Timeout`] on expiry.
pub(crate) async fn gpio_wait_for_rising_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioWaitRequest,
) -> GpioWaitResponse {
    let gpio = gpio_for_input!(context, req.pin);
    debug!(
        "gpio wait_for_rising: pin={=u8} timeout_ms={=u32}",
        req.pin, req.timeout_ms
    );
    crate::progress::bounded(req.timeout_ms, gpio.wait_for_rising_edge())
        .await
        .map_err(|_| {
            defmt::warn!(
                "gpio_wait_for_rising timeout (pin={=u8}, ms={=u32})",
                req.pin,
                req.timeout_ms
            );
            GpioError::Timeout
        })
}

/// Handler for `gpio/wait-falling` — blocks until a falling edge.
///
/// `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT). A value of
/// `0` selects that ceiling; it no longer means "wait forever". Returns
/// [`GpioError::Timeout`] on expiry.
pub(crate) async fn gpio_wait_for_falling_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioWaitRequest,
) -> GpioWaitResponse {
    let gpio = gpio_for_input!(context, req.pin);
    debug!(
        "gpio wait_for_falling: pin={=u8} timeout_ms={=u32}",
        req.pin, req.timeout_ms
    );
    crate::progress::bounded(req.timeout_ms, gpio.wait_for_falling_edge())
        .await
        .map_err(|_| {
            defmt::warn!(
                "gpio_wait_for_falling timeout (pin={=u8}, ms={=u32})",
                req.pin,
                req.timeout_ms
            );
            GpioError::Timeout
        })
}

/// Handler for `gpio/wait-any` — blocks until any edge.
///
/// `req.timeout_ms` is clamped to
/// [`MAX_HANDLER_TIMEOUT`](crate::progress::MAX_HANDLER_TIMEOUT). A value of
/// `0` selects that ceiling; it no longer means "wait forever". Returns
/// [`GpioError::Timeout`] on expiry.
pub(crate) async fn gpio_wait_for_any_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioWaitRequest,
) -> GpioWaitResponse {
    let gpio = gpio_for_input!(context, req.pin);
    debug!(
        "gpio wait_for_any: pin={=u8} timeout_ms={=u32}",
        req.pin, req.timeout_ms
    );
    crate::progress::bounded(req.timeout_ms, gpio.wait_for_any_edge())
        .await
        .map_err(|_| {
            defmt::warn!(
                "gpio_wait_for_any timeout (pin={=u8}, ms={=u32})",
                req.pin,
                req.timeout_ms
            );
            GpioError::Timeout
        })
}

/// Handler for `gpio/set-config` — configures a pin's direction and pull resistor.
///
/// Once configured, the pin enters explicit mode and `gpio_get`/`gpio_put` will
/// no longer auto-switch direction. To restore auto-switching, reset the firmware.
pub(crate) async fn gpio_set_config_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioSetConfigurationRequest,
) -> GpioSetConfigurationResponse {
    let idx = usize::from(req.pin);
    let mode = context.pin_modes.get_mut(idx).ok_or(GpioError::InvalidPin)?;
    let gpio = context
        .gpios
        .get_mut(idx)
        .ok_or(GpioError::InvalidPin)?
        .as_mut()
        .ok_or(GpioError::PinMonitored)?;

    // Apply pull resistor setting
    gpio.set_pull(match req.pull {
        GpioPull::None => Pull::None,
        GpioPull::Up => Pull::Up,
        GpioPull::Down => Pull::Down,
    });

    // Apply direction and update tracked mode
    match req.direction {
        GpioDirection::Input => {
            gpio.set_as_input();
            *mode = PinMode::ExplicitInput;
        }
        GpioDirection::Output => {
            gpio.set_as_output();
            *mode = PinMode::ExplicitOutput;
        }
    }

    debug!(
        "gpio set_config: pin={=u8} dir={=u8} pull={=u8}",
        req.pin, req.direction as u8, req.pull as u8
    );
    Ok(())
}

/// Handler for `gpio/subscribe` — starts edge-event monitoring on a pin.
///
/// Takes ownership of the pin from Context, sends it to a background monitor
/// task, and waits for the armed acknowledgement before returning. While
/// subscribed, the pin cannot be used by other GPIO operations.
pub(crate) async fn gpio_subscribe_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioSubscribeRequest,
) -> GpioSubscribeResponse {
    let idx = usize::from(req.pin);
    if idx >= NUM_GPIOS {
        return Err(GpioError::InvalidPin);
    }

    let flex = context.gpios[idx].take().ok_or(GpioError::PinMonitored)?;

    debug!("gpio subscribe: pin={=u8} edge={=u8}", req.pin, req.edge as u8);

    GPIO_MONITOR_START[idx].send((flex, req.edge)).await;
    GPIO_MONITOR_ARMED[idx].receive().await;

    Ok(())
}

/// Handler for `gpio/unsubscribe` — stops edge-event monitoring on a pin.
///
/// Signals the monitor task to stop, waits for the pin to be returned, and
/// puts it back into Context so regular GPIO operations can resume.
pub(crate) async fn gpio_unsubscribe_handler(
    context: &mut Context,
    _header: VarHeader,
    req: GpioUnsubscribeRequest,
) -> GpioUnsubscribeResponse {
    let idx = usize::from(req.pin);
    if idx >= NUM_GPIOS {
        return Err(GpioError::InvalidPin);
    }

    if context.gpios[idx].is_some() {
        return Err(GpioError::PinNotMonitored);
    }

    debug!("gpio unsubscribe: pin={=u8}", req.pin);

    GPIO_MONITOR_STOP[idx].signal(());
    let flex = GPIO_MONITOR_RETURN[idx].receive().await;
    context.gpios[idx] = Some(flex);

    Ok(())
}

/// Handler for `system/reset-subscriptions` — tears down every active
/// GPIO subscription and returns the count of pins that were reset.
///
/// Hosts call this immediately after [`PicoDeGallo::validate`] when they
/// connect, so that any subscriptions left over from a previous host
/// process (one that crashed, was killed, or dropped its `nusb::Interface`
/// without sending `gpio/unsubscribe`) are cleaned up before the new host
/// starts using GPIO pins. This is idempotent: calling it on a fresh
/// firmware with no live subscriptions is cheap and returns `0`.
pub(crate) async fn system_reset_subscriptions_handler(
    context: &mut Context,
    _header: VarHeader,
    _req: (),
) -> SystemResetSubscriptionsResponse {
    let mut reset = 0u8;
    for idx in 0..NUM_GPIOS {
        // A `None` slot in `context.gpios` means the pin's `Flex` has been
        // handed to the monitor task — i.e. there is a live subscription.
        if context.gpios[idx].is_none() {
            debug!("system reset: tearing down gpio subscription on pin={=usize}", idx);
            GPIO_MONITOR_STOP[idx].signal(());
            let flex = GPIO_MONITOR_RETURN[idx].receive().await;
            context.gpios[idx] = Some(flex);
            reset = reset.saturating_add(1);
        }
    }
    reset
}
