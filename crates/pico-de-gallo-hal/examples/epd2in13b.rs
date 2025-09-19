use embedded_graphics::{
    mono_font::MonoTextStyleBuilder,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle},
    text::{Baseline, Text, TextStyleBuilder},
};
use embedded_hal::delay::DelayNs;
use embedded_hal_bus::spi::ExclusiveDevice;
use epd_waveshare::{
    color::*,
    epd2in13_v2::{Display2in13, Epd2in13},
    graphics::DisplayRotation,
    prelude::*,
};
use pico_de_gallo_hal::{Hal, SpiPhase, SpiPolarity};

fn main() {
    let mut hal = Hal::new();

    hal.set_config(
        100_000,
        10_000_000,
        SpiPhase::CaptureOnFirstTransition,
        SpiPolarity::IdleLow,
    );

    let cs = hal.gpio(0);
    let dc = hal.gpio(1);
    let rst = hal.gpio(2);
    let busy = hal.gpio(3);
    let delay = hal.delay();
    let spi = hal.spi();

    // One SPI device only on the SPI bus
    let mut spi = ExclusiveDevice::new(spi, cs, delay).unwrap();

    let mut delay = hal.delay();
    let mut epd2in13 =
        Epd2in13::new(&mut spi, busy, dc, rst, &mut delay, None).expect("eink initalize error");

    // Rotations
    let mut display = Display2in13::default();

    display.set_rotation(DisplayRotation::Rotate0);
    draw_text(&mut display, "Rotate 0!", 5, 50);

    display.set_rotation(DisplayRotation::Rotate90);
    draw_text(&mut display, "Rotate 90!", 5, 50);

    display.set_rotation(DisplayRotation::Rotate180);
    draw_text(&mut display, "Rotate 180!", 5, 50);

    display.set_rotation(DisplayRotation::Rotate270);
    draw_text(&mut display, "Rotate 270!", 5, 50);

    epd2in13
        .update_frame(&mut spi, display.buffer(), &mut delay)
        .expect("update frame");
    epd2in13
        .display_frame(&mut spi, &mut delay)
        .expect("display frame new graphics");
    delay.delay_ms(5_000);

    display.clear(Color::White).ok();

    // draw a analog clock
    let _ = Circle::with_center(Point::new(64, 64), 80)
        .into_styled(PrimitiveStyle::with_stroke(Color::Black, 1))
        .draw(&mut display);
    let _ = Line::new(Point::new(64, 64), Point::new(30, 40))
        .into_styled(PrimitiveStyle::with_stroke(Color::Black, 4))
        .draw(&mut display);
    let _ = Line::new(Point::new(64, 64), Point::new(80, 40))
        .into_styled(PrimitiveStyle::with_stroke(Color::Black, 1))
        .draw(&mut display);

    epd2in13
        .update_frame(&mut spi, display.buffer(), &mut delay)
        .expect("update frame");
    epd2in13
        .display_frame(&mut spi, &mut delay)
        .expect("display frame new graphics");

    delay.delay_ms(5_000);

    display.clear(Color::White).ok();

    // draw white on black background
    let style = MonoTextStyleBuilder::new()
        .font(&embedded_graphics::mono_font::ascii::FONT_6X10)
        .text_color(Color::White)
        .background_color(Color::Black)
        .build();
    let text_style = TextStyleBuilder::new().baseline(Baseline::Top).build();

    let _ = Text::with_text_style("It's working-WoB!", Point::new(90, 10), style, text_style)
        .draw(&mut display);

    // use bigger/different font
    let style = MonoTextStyleBuilder::new()
        .font(&embedded_graphics::mono_font::ascii::FONT_10X20)
        .text_color(Color::White)
        .background_color(Color::Black)
        .build();

    let _ = Text::with_text_style("It's working\nWoB!", Point::new(90, 40), style, text_style)
        .draw(&mut display);

    epd2in13
        .update_frame(&mut spi, display.buffer(), &mut delay)
        .expect("update frame");
    epd2in13
        .display_frame(&mut spi, &mut delay)
        .expect("display frame new graphics");

    delay.delay_ms(5_000);

    display.clear(Color::White).ok();

    // Demonstrating how to use the partial refresh feature of the screen.
    // Real animations can be used.
    epd2in13
        .set_refresh(&mut spi, &mut delay, RefreshLut::Quick)
        .unwrap();
    epd2in13.clear_frame(&mut spi, &mut delay).unwrap();

    // a moving `Hello World!`
    let limit = 10;
    for i in 0..limit {
        draw_text(&mut display, "  Hello World! ", 5 + i * 12, 50);

        epd2in13
            .update_and_display_frame(&mut spi, display.buffer(), &mut delay)
            .expect("display frame new graphics");
    }
    delay.delay_ms(5_000);

    // Show a spinning bar without any delay between frames. Shows how «fast»
    // the screen can refresh for this kind of change (small single character)
    display.clear(Color::White).ok();
    epd2in13
        .update_and_display_frame(&mut spi, display.buffer(), &mut delay)
        .unwrap();

    let spinner = ["|", "/", "-", "\\"];
    for i in 0..10 {
        display.clear(Color::White).ok();
        draw_text(&mut display, spinner[i % spinner.len()], 10, 100);
        epd2in13
            .update_and_display_frame(&mut spi, display.buffer(), &mut delay)
            .unwrap();
    }

    println!("Finished tests - going to sleep");
    epd2in13.sleep(&mut spi, &mut delay).expect("sleep");
}

fn draw_text(display: &mut Display2in13, text: &str, x: i32, y: i32) {
    let style = MonoTextStyleBuilder::new()
        .font(&embedded_graphics::mono_font::ascii::FONT_6X10)
        .text_color(Color::White)
        .background_color(Color::Black)
        .build();

    let text_style = TextStyleBuilder::new().baseline(Baseline::Top).build();

    let _ = Text::with_text_style(text, Point::new(x, y), style, text_style).draw(display);
}
