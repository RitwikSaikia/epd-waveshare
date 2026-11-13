//! 4.26-inch (800 x 480) four-grayscale e-paper example.
//!
//! Full-screen demos use four-gray refresh. The final two demos use real
//! monochrome hardware partial refresh: White stays white, and every other
//! Gray2Color shade is displayed as black.
use embedded_graphics::{
    mono_font::{ascii::FONT_10X20, MonoTextStyle},
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    text::Text,
};
use embedded_hal::delay::DelayNs;
use epd_waveshare::{
    epd4in26::{Display4in26, Epd4in26},
    graphics::{Display, DisplayRotation},
    prelude::*,
};
use linux_embedded_hal::{
    gpio_cdev::{Chip, LineRequestFlags},
    spidev::{self, SpidevOptions},
    CdevPin, Delay, SPIError, SpidevDevice,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type Panel = Epd4in26<SpidevDevice, CdevPin, CdevPin, CdevPin, Delay>;

const WHITE: Gray2Color = Gray2Color::White;
const BLACK: Gray2Color = Gray2Color::Black;
const PAUSE_MS: u32 = 2_000;

// Both partial update windows have an 8-pixel-aligned X and width.
// Partial buffers contain packed 2-bpp pixels (four pixels per byte).
const REGION_X: u32 = 56;
const REGION_Y: u32 = 96;
const REGION_W: u32 = 256;
const REGION_H: u32 = 144;
const REGION_BYTES: usize = (REGION_W * REGION_H / 4) as usize;
type RegionDisplay = Display<REGION_W, REGION_H, false, REGION_BYTES, Gray2Color>;

const COUNTER_X: u32 = 16;
const COUNTER_Y: u32 = 96;
const COUNTER_W: u32 = 224;
const COUNTER_H: u32 = 64;
const COUNTER_BYTES: usize = (COUNTER_W * COUNTER_H / 4) as usize;
type CounterDisplay = Display<COUNTER_W, COUNTER_H, false, COUNTER_BYTES, Gray2Color>;

// Dedicated packed 2-bpp framebuffer for a monochrome partial-refresh clock.
const CLOCK_X: u32 = 200;
const CLOCK_Y: u32 = 184;
const CLOCK_W: u32 = 400;
const CLOCK_H: u32 = 112;
const CLOCK_BYTES: usize = (CLOCK_W * CLOCK_H / 4) as usize;
type ClockDisplay = Display<CLOCK_W, CLOCK_H, false, CLOCK_BYTES, Gray2Color>;

// BCM GPIO offsets are relative to the SoC controller, not sysfs GPIO numbers.
// On the Raspberry Pi 4 this is normally /dev/gpiochip0.
const GPIO_CHIP: &str = "/dev/gpiochip0";

fn pin(chip: &mut Chip, number: u32, flags: LineRequestFlags, initial: u8) -> CdevPin {
    let handle = chip
        .get_line(number)
        .unwrap_or_else(|err| panic!("BCM GPIO {number}: {err}"))
        .request(flags, initial, "epd4in26")
        .unwrap_or_else(|err| panic!("Request BCM GPIO {number}: {err}"));
    CdevPin::new(handle).expect("Create GPIO pin")
}

fn setup_spi() -> SpidevDevice {
    let mut spi = SpidevDevice::open("/dev/spidev0.0").expect("SPI device");
    spi.configure(
        &SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(4_000_000)
            .mode(spidev::SpiModeFlags::SPI_MODE_0)
            .build(),
    )
    .expect("SPI configuration");
    spi
}

fn show(
    epd: &mut Panel,
    spi: &mut SpidevDevice,
    delay: &mut Delay,
    display: &Display4in26,
) -> Result<(), SPIError> {
    epd.update_and_display_frame(spi, display.buffer(), delay)?;
    delay.delay_ms(PAUSE_MS);
    Ok(())
}

fn show_partial(
    epd: &mut Panel,
    spi: &mut SpidevDevice,
    delay: &mut Delay,
    buffer: &[u8],
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<(), SPIError> {
    epd.update_partial_frame(spi, delay, buffer, x, y, width, height)?;
    epd.display_frame(spi, delay)?;
    delay.delay_ms(PAUSE_MS);
    Ok(())
}

fn text<T: DrawTarget<Color = Gray2Color>>(
    display: &mut T,
    label: &str,
    x: i32,
    y: i32,
    color: Gray2Color,
) {
    let _ = Text::new(
        label,
        Point::new(x, y),
        MonoTextStyle::new(&FONT_10X20, color),
    )
    .draw(display);
}

fn reset_canvas(display: &mut Display4in26) {
    display.set_rotation(DisplayRotation::Rotate0);
    let _ = display.clear(WHITE);
}

fn clear_panel(epd: &mut Panel, spi: &mut SpidevDevice, delay: &mut Delay) -> Result<(), SPIError> {
    println!("Clear panel");
    epd.clear_frame(spi, delay)?;
    epd.display_frame(spi, delay)
}

fn draw_shapes(display: &mut Display4in26, x: i32, y: i32, color: Gray2Color) {
    let outline = PrimitiveStyle::with_stroke(color, 2);
    let fill = PrimitiveStyle::with_fill(color);
    let _ = Line::new(Point::new(x, y), Point::new(x + 80, y + 80))
        .into_styled(outline)
        .draw(display);
    let _ = Line::new(Point::new(x + 80, y), Point::new(x, y + 80))
        .into_styled(outline)
        .draw(display);
    let _ = Rectangle::new(Point::new(x, y), Size::new(80, 80))
        .into_styled(outline)
        .draw(display);
    let _ = Circle::new(Point::new(x + 110, y), 80)
        .into_styled(outline)
        .draw(display);
    let _ = Rectangle::new(Point::new(x + 220, y), Size::new(80, 80))
        .into_styled(fill)
        .draw(display);
}

fn horizontal_demo(
    epd: &mut Panel,
    spi: &mut SpidevDevice,
    delay: &mut Delay,
    display: &mut Display4in26,
) -> Result<(), SPIError> {
    println!("Horizontal text and graphics");
    reset_canvas(display);
    text(display, "Hello world!", 20, 35, BLACK);
    text(display, "4.26 inch e-Paper", 20, 65, BLACK);
    draw_shapes(display, 30, 100, BLACK);
    show(epd, spi, delay, display)
}

fn vertical_demo(
    epd: &mut Panel,
    spi: &mut SpidevDevice,
    delay: &mut Delay,
    display: &mut Display4in26,
) -> Result<(), SPIError> {
    println!("Vertical text and graphics");
    reset_canvas(display);
    display.set_rotation(DisplayRotation::Rotate90);
    text(display, "Hello world!", 10, 30, BLACK);
    text(display, "4.26 inch e-Paper", 10, 60, BLACK);
    draw_shapes(display, 20, 100, BLACK);
    show(epd, spi, delay, display)?;
    display.set_rotation(DisplayRotation::Rotate0);
    Ok(())
}

fn grayscale_demo(
    epd: &mut Panel,
    spi: &mut SpidevDevice,
    delay: &mut Delay,
    display: &mut Display4in26,
) -> Result<(), SPIError> {
    println!("Four shades of gray");
    reset_canvas(display);
    text(display, "Four grayscale levels", 20, 40, BLACK);
    let shades = [
        (Gray2Color::Black, "Black"),
        (Gray2Color::DarkGray, "Dark gray"),
        (Gray2Color::LightGray, "Light gray"),
        (Gray2Color::White, "White"),
    ];
    for (i, &(shade, label)) in shades.iter().enumerate() {
        let x = 20 + i as i32 * 195;
        let _ = Rectangle::new(Point::new(x, 85), Size::new(180, 140))
            .into_styled(PrimitiveStyle::with_fill(shade))
            .draw(display);
        let _ = Rectangle::new(Point::new(x, 85), Size::new(180, 140))
            .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
            .draw(display);
        text(display, label, x, 255, BLACK);
    }
    for (i, &(shade, _)) in shades.iter().enumerate() {
        let _ = Rectangle::new(Point::new(20 + i as i32 * 190, 320), Size::new(190, 90))
            .into_styled(PrimitiveStyle::with_fill(shade))
            .draw(display);
    }
    show(epd, spi, delay, display)
}

fn patterned_demo(
    epd: &mut Panel,
    spi: &mut SpidevDevice,
    delay: &mut Delay,
    display: &mut Display4in26,
) -> Result<(), SPIError> {
    println!("Grayscale pattern (no external bitmap required)");
    reset_canvas(display);
    text(display, "Grayscale pattern", 20, 40, BLACK);
    let shades = [BLACK, Gray2Color::DarkGray, Gray2Color::LightGray, WHITE];
    for row in 0..8 {
        for col in 0..16 {
            let shade = shades[((row + col) % 4) as usize];
            let _ = Rectangle::new(Point::new(col * 48 + 16, row * 44 + 75), Size::new(48, 44))
                .into_styled(PrimitiveStyle::with_fill(shade))
                .draw(display);
        }
    }
    show(epd, spi, delay, display)
}

fn region_update_demo(
    epd: &mut Panel,
    spi: &mut SpidevDevice,
    delay: &mut Delay,
    _display: &mut Display4in26,
) -> Result<(), SPIError> {
    println!("Region redraw (hardware partial refresh)");
    // First partial refresh establishes a clean monochrome white base.
    // The previous four-gray image will not be preserved outside this window.

    // These coordinates are relative to the partial window, not the screen.
    let mut region = RegionDisplay::default();
    for (index, shade) in [BLACK, Gray2Color::DarkGray, Gray2Color::LightGray, WHITE]
        .iter()
        .enumerate()
    {
        let _ = region.clear(WHITE);
        let _ = Rectangle::new(Point::new(0, 0), Size::new(REGION_W, REGION_H))
            .into_styled(PrimitiveStyle::with_fill(*shade))
            .draw(&mut region);
        // White letters on all black-equivalent shades; black on white.
        let label_color = if index == 3 { BLACK } else { WHITE };
        text(&mut region, "Updated region", 8, 80, label_color);
        println!("Partial region update {}/4", index + 1);
        show_partial(
            epd,
            spi,
            delay,
            region.buffer(),
            REGION_X,
            REGION_Y,
            REGION_W,
            REGION_H,
        )?;
    }
    Ok(())
}

fn counter_update_demo(
    epd: &mut Panel,
    spi: &mut SpidevDevice,
    delay: &mut Delay,
    _display: &mut Display4in26,
) -> Result<(), SPIError> {
    println!("Counter redraw (hardware partial refresh)");
    // A full grayscale update here would require another monochrome base
    // initialization; keep this demo entirely in partial mode.

    let mut counter = CounterDisplay::default();
    for count in 0..10 {
        let _ = counter.clear(WHITE);
        let label = format!("Count: {count}");
        text(&mut counter, &label, 8, 44, BLACK);
        println!("Partial counter update: {count}");
        show_partial(
            epd,
            spi,
            delay,
            counter.buffer(),
            COUNTER_X,
            COUNTER_Y,
            COUNTER_W,
            COUNTER_H,
        )?;
    }
    Ok(())
}

// Seven-segment glyphs are a custom font implemented using graphics primitives.
// Bit order: top, top-right, bottom-right, bottom, bottom-left,
// top-left, middle. Each glyph is 42 x 82 pixels.
const DIGIT_SEGMENTS: [u8; 10] = [
    0b011_1111, // 0
    0b000_0110, // 1
    0b101_1011, // 2
    0b100_1111, // 3
    0b110_0110, // 4
    0b110_1101, // 5
    0b111_1101, // 6
    0b000_0111, // 7
    0b111_1111, // 8
    0b110_1111, // 9
];

fn clock_digit<T: DrawTarget<Color = Gray2Color>>(target: &mut T, digit: u8, x: i32, y: i32) {
    let bits = DIGIT_SEGMENTS[digit as usize];
    let stroke = PrimitiveStyle::with_stroke(BLACK, 12);
    let segments = [
        ((x + 7, y + 4), (x + 35, y + 4)),    // top
        ((x + 38, y + 9), (x + 38, y + 39)),  // upper right
        ((x + 38, y + 46), (x + 38, y + 76)), // lower right
        ((x + 7, y + 81), (x + 35, y + 81)),  // bottom
        ((x + 4, y + 46), (x + 4, y + 76)),   // lower left
        ((x + 4, y + 9), (x + 4, y + 39)),    // upper left
        ((x + 7, y + 42), (x + 35, y + 42)),  // center
    ];
    for (index, &((x0, y0), (x1, y1))) in segments.iter().enumerate() {
        if bits & (1 << index) != 0 {
            let _ = Line::new(Point::new(x0, y0), Point::new(x1, y1))
                .into_styled(stroke)
                .draw(target);
        }
    }
}

fn clock_colon<T: DrawTarget<Color = Gray2Color>>(target: &mut T, x: i32, y: i32) {
    for offset in [26, 60] {
        let _ = Circle::new(Point::new(x + 1, y + offset - 4), 16)
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(target);
    }
}

fn draw_clock(target: &mut ClockDisplay, seconds_since_epoch: u64) {
    let seconds = seconds_since_epoch % 86_400;
    let hours = (seconds / 3_600) as u8;
    let minutes = ((seconds / 60) % 60) as u8;
    let seconds = (seconds % 60) as u8;
    let digits = [
        hours / 10,
        hours % 10,
        minutes / 10,
        minutes % 10,
        seconds / 10,
        seconds % 10,
    ];
    let mut x = 18;
    let y = 10;
    for (i, digit) in digits.iter().enumerate() {
        if i == 2 || i == 4 {
            clock_colon(target, x, y);
            x += 22;
        }
        clock_digit(target, *digit, x, y);
        x += 54;
    }
}

/// Display the current UTC clock for ten seconds, refreshing only its region.
/// The deadline is measured with Instant, so panel refresh time counts toward
/// the ten seconds instead of adding ten fixed sleeps after each refresh.
fn clock_update_demo(
    epd: &mut Panel,
    spi: &mut SpidevDevice,
    delay: &mut Delay,
    seconds: Option<u64>,
) -> Result<(), SPIError> {
    println!("Clock demo: custom seven-segment font, (UTC)");
    let mut clock = ClockDisplay::default();
    let start = Instant::now();
    let mut previous_second = None;
    let seconds = seconds.unwrap_or(30);

    while start.elapsed() < Duration::from_secs(seconds) {
        let utc_second = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System clock must be after 1970")
            .as_secs();
        if previous_second != Some(utc_second) {
            clock.clear(WHITE).ok();
            draw_clock(&mut clock, utc_second);
            epd.update_partial_frame(
                spi,
                delay,
                clock.buffer(),
                CLOCK_X,
                CLOCK_Y,
                CLOCK_W,
                CLOCK_H,
            )?;
            epd.display_frame(spi, delay)?;
            previous_second = Some(utc_second);
        } else {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    Ok(())
}

fn main() -> Result<(), SPIError> {
    let mut spi = setup_spi();
    // Waveshare Raspberry Pi HAT defaults: BUSY=24, DC=25, RST=17, PWR=18.
    // SPI chip select (CE0, BCM 8) is managed by /dev/spidev0.0.
    // Change these BCM numbers if using custom wiring.
    let mut chip = Chip::new(GPIO_CHIP).expect("Open GPIO chip (check /dev/gpiochip* permissions)");
    println!("Using {GPIO_CHIP}");
    let _power = pin(&mut chip, 18, LineRequestFlags::OUTPUT, 1);
    let busy = pin(&mut chip, 24, LineRequestFlags::INPUT, 0);
    let dc = pin(&mut chip, 25, LineRequestFlags::OUTPUT, 1);
    let rst = pin(&mut chip, 17, LineRequestFlags::OUTPUT, 1);
    println!(
        "BUSY (BCM 24) before init: {}",
        busy.get_value().expect("Read BUSY")
    );
    let mut delay = Delay {};
    println!("Initializing 4.26-inch panel...");
    let mut epd = Epd4in26::new(&mut spi, busy, dc, rst, &mut delay, None)?;
    println!("Panel initialized");
    let mut display = Display4in26::default();
    clear_panel(&mut epd, &mut spi, &mut delay)?;
    horizontal_demo(&mut epd, &mut spi, &mut delay, &mut display)?;
    vertical_demo(&mut epd, &mut spi, &mut delay, &mut display)?;
    grayscale_demo(&mut epd, &mut spi, &mut delay, &mut display)?;
    patterned_demo(&mut epd, &mut spi, &mut delay, &mut display)?;

    clear_panel(&mut epd, &mut spi, &mut delay)?;
    region_update_demo(&mut epd, &mut spi, &mut delay, &mut display)?;

    clear_panel(&mut epd, &mut spi, &mut delay)?;
    counter_update_demo(&mut epd, &mut spi, &mut delay, &mut display)?;

    clear_panel(&mut epd, &mut spi, &mut delay)?;
    clock_update_demo(&mut epd, &mut spi, &mut delay, Some(u64::MAX))?;

    epd.sleep(&mut spi, &mut delay)
}
