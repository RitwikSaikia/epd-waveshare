//! Waveshare 4-inch (E) six-color e-paper example, based on EPD_4in0e_test.c.
//!
//! The driver in this branch is named `epd4in0e`, despite this example's filename.
//! Run on a Raspberry Pi with `--test all` or `--test bmp --bmp pic/02.bmp`.
//! The panel does not support partial refresh.

use std::{error::Error, fs, path::Path};

use embedded_graphics::{
    mono_font::{ascii::{FONT_6X10, FONT_10X20}, MonoTextStyle},
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};
use embedded_hal::{delay::DelayNs, digital::OutputPin};
use epd_waveshare::{
    color::HexColor,
    epd4in0e::{Display4in0e, Epd4in0e, HEIGHT, WIDTH},
    prelude::*,
};
use linux_embedded_hal::{
    gpio_cdev::{Chip, LineRequestFlags},
    spidev::{self, SpidevOptions},
    CdevPin, Delay, SpidevDevice,
};

const SPI_PATH: &str = "/dev/spidev0.0";
const GPIO_CHIP: &str = "/dev/gpiochip0";
const SPI_SPEED_HZ: u32 = 2_000_000;
const BUSY_PIN: u32 = 24;
const DC_PIN: u32 = 25;
const RST_PIN: u32 = 17;
const POWER_PIN: u32 = 18;
const HOLD_MS: u32 = 3_000;

type DisplayDriver = Epd4in0e<SpidevDevice, CdevPin, CdevPin, CdevPin, Delay>;
type TestResult = Result<(), Box<dyn Error>>;

fn request_pin(
    chip: &mut Chip,
    number: u32,
    flags: LineRequestFlags,
    initial_value: u8,
    name: &str,
) -> Result<CdevPin, Box<dyn Error>> {
    let handle = chip.get_line(number)?.request(flags, initial_value, name)?;
    Ok(CdevPin::new(handle)?)
}

fn new_image() -> Box<Display4in0e> {
    let mut image = Box::new(Display4in0e::default());
    image.clear(HexColor::White).unwrap();
    image
}

fn show_image(
    epd: &mut DisplayDriver,
    spi: &mut SpidevDevice,
    delay: &mut Delay,
    image: &Display4in0e,
) -> TestResult {
    epd.update_and_display_frame(spi, image.buffer(), delay)?;
    delay.delay_ms(HOLD_MS);
    Ok(())
}

fn test_blocks(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Six-color block test");
    epd.show_6block(spi, delay)?;
    delay.delay_ms(HOLD_MS);
    Ok(())
}

fn test_stripes(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Six-color stripes");
    let mut image = new_image();
    let colors = [HexColor::Black, HexColor::White, HexColor::Yellow, HexColor::Red, HexColor::Blue, HexColor::Green];
    for (index, color) in colors.iter().enumerate() {
        let x = index as u32 * (WIDTH / colors.len() as u32);
        let width = if index + 1 == colors.len() { WIDTH - x } else { WIDTH / colors.len() as u32 };
        Rectangle::new(Point::new(x as i32, 0), Size::new(width, HEIGHT))
            .into_styled(PrimitiveStyle::with_fill(*color)).draw(&mut *image)?;
    }
    show_image(epd, spi, delay, &image)
}

// Decode uncompressed 24/32-bit Windows BMP files and quantize to panel colors.
fn read_bmp(path: &Path, image: &mut Display4in0e) -> TestResult {
    let bytes = fs::read(path)?;
    if bytes.len() < 54 || &bytes[..2] != b"BM" {
        return Err("not a BMP file".into());
    }
    let get_u16 = |i: usize| -> u16 { u16::from_le_bytes([bytes[i], bytes[i + 1]]) };
    let get_u32 = |i: usize| -> u32 { u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) };
    let pixel_offset = get_u32(10) as usize;
    let dib_size = get_u32(14) as usize;
    let width = i32::from_le_bytes(bytes[18..22].try_into()?);
    let signed_height = i32::from_le_bytes(bytes[22..26].try_into()?);
    let bpp = get_u16(28);
    if dib_size < 40 || 14_usize.checked_add(dib_size).map_or(true, |end| end > bytes.len())
        || width <= 0 || signed_height == 0 || signed_height == i32::MIN
        || get_u16(26) != 1 || !matches!(bpp, 24 | 32) || get_u32(30) != 0 {
        return Err("BMP must be uncompressed 24/32-bit Windows BMP".into());
    }
    let w = width as usize;
    let h = signed_height.unsigned_abs() as usize;
    let row_bytes = w.checked_mul(bpp as usize).and_then(|n| n.checked_add(31))
        .map(|n| (n / 32) * 4).ok_or("BMP width overflow")?;
    let end = row_bytes.checked_mul(h).and_then(|n| n.checked_add(pixel_offset)).ok_or("BMP size overflow")?;
    if pixel_offset < 14 + dib_size || end > bytes.len() {
        return Err("invalid or truncated BMP pixel data".into());
    }
    for y in 0..h.min(HEIGHT as usize) {
        let source_y = if signed_height > 0 { h - 1 - y } else { y };
        for x in 0..w.min(WIDTH as usize) {
            let offset = pixel_offset + source_y * row_bytes + x * (bpp as usize / 8);
            let (b, g, r) = (bytes[offset], bytes[offset + 1], bytes[offset + 2]);
            Pixel(Point::new(x as i32, y as i32), nearest_color(r, g, b)).draw(image)?;
        }
    }
    Ok(())
}

fn nearest_color(r: u8, g: u8, b: u8) -> HexColor {
    let palette = [
        (HexColor::Black, [0_i32, 0, 0]),
        (HexColor::White, [255, 255, 255]),
        (HexColor::Yellow, [255, 255, 0]),
        (HexColor::Red, [255, 0, 0]),
        (HexColor::Blue, [0, 0, 255]),
        (HexColor::Green, [0, 255, 0]),
    ];
    palette.iter().min_by_key(|(_, rgb)| {
        (rgb[0] - i32::from(r)).pow(2)
            + (rgb[1] - i32::from(g)).pow(2)
            + (rgb[2] - i32::from(b)).pow(2)
    }).unwrap().0
}

fn test_bmp(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay, path: &Path) -> TestResult {
    println!("BMP test: {}", path.display());
    if !path.exists() {
        println!("BMP not found; skipping (use --bmp PATH)");
        return Ok(());
    }
    let mut image = new_image();
    read_bmp(path, &mut image)?;
    show_image(epd, spi, delay, &image)
}

fn test_points(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Points test");
    let mut image = new_image();
    for (x, y, size, color) in [(10, 80, 1, HexColor::Red), (10, 90, 2, HexColor::Blue), (10, 100, 3, HexColor::Green)] {
        Rectangle::new(Point::new(x, y), Size::new(size, size))
            .into_styled(PrimitiveStyle::with_fill(color)).draw(&mut *image)?;
    }
    show_image(epd, spi, delay, &image)
}

fn test_lines(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Solid and dotted lines test");
    let mut image = new_image();
    for (a, b) in [(Point::new(20, 70), Point::new(70, 120)), (Point::new(70, 70), Point::new(20, 120))] {
        Line::new(a, b).into_styled(PrimitiveStyle::with_stroke(HexColor::Yellow, 1)).draw(&mut *image)?;
    }
    for x in (85..=125).step_by(4) { Pixel(Point::new(x, 95), HexColor::Yellow).draw(&mut *image)?; }
    for y in (75..=115).step_by(4) { Pixel(Point::new(105, y), HexColor::Yellow).draw(&mut *image)?; }
    show_image(epd, spi, delay, &image)
}

fn test_rectangles(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Rectangles test");
    let mut image = new_image();
    Rectangle::new(Point::new(20, 70), Size::new(51, 51))
        .into_styled(PrimitiveStyle::with_stroke(HexColor::Black, 1)).draw(&mut *image)?;
    Rectangle::new(Point::new(80, 70), Size::new(51, 51))
        .into_styled(PrimitiveStyle::with_fill(HexColor::Black)).draw(&mut *image)?;
    show_image(epd, spi, delay, &image)
}

fn test_circles(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Circles test");
    let mut image = new_image();
    Circle::new(Point::new(25, 75), 40)
        .into_styled(PrimitiveStyle::with_stroke(HexColor::Black, 1)).draw(&mut *image)?;
    Rectangle::new(Point::new(80, 70), Size::new(51, 51))
        .into_styled(PrimitiveStyle::with_fill(HexColor::Black)).draw(&mut *image)?;
    Circle::new(Point::new(85, 75), 40)
        .into_styled(PrimitiveStyle::with_fill(HexColor::White)).draw(&mut *image)?;
    show_image(epd, spi, delay, &image)
}

fn draw_text(image: &mut Display4in0e, x: i32, y: i32, value: &str, color: HexColor, large: bool) -> TestResult {
    if large {
        Text::with_baseline(value, Point::new(x, y), MonoTextStyle::new(&FONT_10X20, color), Baseline::Top).draw(image)?;
    } else {
        Text::with_baseline(value, Point::new(x, y), MonoTextStyle::new(&FONT_6X10, color), Baseline::Top).draw(image)?;
    }
    Ok(())
}

fn test_numbers(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Numbers test");
    let mut image = new_image();
    draw_text(&mut image, 10, 33, "123456789", HexColor::Black, false)?;
    Rectangle::new(Point::new(10, 50), Size::new(80, 15))
        .into_styled(PrimitiveStyle::with_fill(HexColor::Black)).draw(&mut *image)?;
    draw_text(&mut image, 10, 50, "987654321", HexColor::White, false)?;
    show_image(epd, spi, delay, &image)
}

fn test_text(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("English text and colors test");
    let mut image = new_image();
    let colors = [HexColor::Black, HexColor::Green, HexColor::Blue, HexColor::Red, HexColor::Yellow];
    // On this portrait panel, shift the original x=400 text into the visible area.
    for (row, color) in colors.iter().enumerate() {
        draw_text(&mut image, 285, row as i32 * 20, "waveshare", *color, false)?;
    }
    Rectangle::new(Point::new(150, 0), Size::new(130, 205))
        .into_styled(PrimitiveStyle::with_fill(HexColor::Black)).draw(&mut *image)?;
    for (row, color) in [HexColor::White, HexColor::Green, HexColor::Blue, HexColor::Red, HexColor::Yellow].iter().enumerate() {
        draw_text(&mut image, 150, row as i32 * 30, "hello world", *color, true)?;
    }
    Rectangle::new(Point::new(150, 180), Size::new(130, 22))
        .into_styled(PrimitiveStyle::with_fill(HexColor::Yellow)).draw(&mut *image)?;
    draw_text(&mut image, 150, 180, "hello world", HexColor::Black, true)?;
    show_image(epd, spi, delay, &image)
}

fn test_chinese(_epd: &mut DisplayDriver, _spi: &mut SpidevDevice, _delay: &mut Delay) -> TestResult {
    println!("Chinese font test skipped: Font12CN/Font24CN assets are not bundled");
    Ok(())
}

fn test_clear(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Clear display to white");
    epd.set_background_color(HexColor::White);
    epd.clear_frame(spi, delay)?;
    delay.delay_ms(1_000);
    Ok(())
}

fn main() -> TestResult {
    let mut test = "all".to_owned();
    let mut bmp_path = "examples/assets/epd4in0e.bmp".to_owned();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--test" => test = args.next().ok_or("--test requires a value")?,
            "--bmp" => bmp_path = args.next().ok_or("--bmp requires a path")?,
            _ => return Err(format!("unknown argument: {arg}; use --test NAME or --bmp PATH").into()),
        }
    }
    let known = ["all", "blocks", "stripes", "bmp", "points", "lines", "rectangles", "circles", "numbers", "text", "chinese", "clear"];
    if !known.contains(&test.as_str()) {
        return Err(format!("unknown test '{test}'; choose from {}", known.join(", ")).into());
    }

    let mut spi = SpidevDevice::open(SPI_PATH)?;
    spi.configure(&SpidevOptions::new().bits_per_word(8).max_speed_hz(SPI_SPEED_HZ)
        .mode(spidev::SpiModeFlags::SPI_MODE_0).build())?;
    let mut chip = Chip::new(GPIO_CHIP)?;
    let mut power = request_pin(&mut chip, POWER_PIN, LineRequestFlags::OUTPUT, 1, "epd-power")?;
    let busy = request_pin(&mut chip, BUSY_PIN, LineRequestFlags::INPUT, 0, "epd-busy")?;
    let dc = request_pin(&mut chip, DC_PIN, LineRequestFlags::OUTPUT, 0, "epd-dc")?;
    let rst = request_pin(&mut chip, RST_PIN, LineRequestFlags::OUTPUT, 0, "epd-rst")?;
    let mut delay = Delay {};
    let mut epd = Epd4in0e::new(&mut spi, busy, dc, rst, &mut delay, None)?;
    let run = |name: &str| test == "all" || test == name;

    let result = (|| -> TestResult {
        // The Waveshare C demo clears the panel before the test images.
        if test == "all" { test_clear(&mut epd, &mut spi, &mut delay)?; }
        if run("bmp") { test_bmp(&mut epd, &mut spi, &mut delay, Path::new(&bmp_path))?; }
        if run("points") { test_points(&mut epd, &mut spi, &mut delay)?; }
        if run("lines") { test_lines(&mut epd, &mut spi, &mut delay)?; }
        if run("rectangles") { test_rectangles(&mut epd, &mut spi, &mut delay)?; }
        if run("circles") { test_circles(&mut epd, &mut spi, &mut delay)?; }
        if run("numbers") { test_numbers(&mut epd, &mut spi, &mut delay)?; }
        if run("text") { test_text(&mut epd, &mut spi, &mut delay)?; }
        if run("chinese") { test_chinese(&mut epd, &mut spi, &mut delay)?; }
        if run("stripes") { test_stripes(&mut epd, &mut spi, &mut delay)?; }
        if run("blocks") { test_blocks(&mut epd, &mut spi, &mut delay)?; }
        if run("clear") || test == "all" { test_clear(&mut epd, &mut spi, &mut delay)?; }
        Ok(())
    })();
    let sleep_result = epd.sleep(&mut spi, &mut delay);
    delay.delay_ms(2_000);
    power.set_low()?;
    result?;
    sleep_result?;
    Ok(())
}

