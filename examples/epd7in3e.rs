//! Waveshare 7.3-inch (E) six-color e-paper examples, adapted from Waveshare's
//! EPD_7in3e_test.c, plus the existing six-block and partial-window examples.
//!
//! Raspberry Pi 3: /dev/gpiochip0 (pinctrl-bcm2835).
//! Run from the repository root to use pic/7in3e.bmp, or supply --bmp PATH.
//! You can run one test with --test NAME, or all tests with --test all.
//! The panel's physical partial-refresh support is not guaranteed.

use std::{error::Error, fs, path::Path};

use embedded_graphics::{
    mono_font::{ascii::{FONT_6X10, FONT_10X20}, MonoTextStyle},
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};
use embedded_hal::delay::DelayNs;
use epd_waveshare::{
    color::HexColor,
    epd7in3e::{Display7in3e, Epd7in3e, HEIGHT, WIDTH},
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
const HOLD_MS: u32 = 3_000;

const PARTIAL_X: u32 = 150;
const PARTIAL_Y: u32 = 50;
const PARTIAL_WIDTH: u32 = 100;
const PARTIAL_HEIGHT: u32 = 400;

type DisplayDriver = Epd7in3e<SpidevDevice, CdevPin, CdevPin, CdevPin, Delay>;
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

fn new_image() -> Box<Display7in3e> {
    let mut image = Box::new(Display7in3e::default());
    image.clear(HexColor::White).unwrap();
    image
}

fn show_image(
    epd: &mut DisplayDriver,
    spi: &mut SpidevDevice,
    delay: &mut Delay,
    image: &Display7in3e,
) -> TestResult {
    epd.update_and_display_frame(spi, image.buffer(), delay)?;
    delay.delay_ms(HOLD_MS);
    Ok(())
}

// Original test from commit f6a5e19.
fn test_six_blocks(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Six-color block test");
    epd.show_6block(spi, delay)?;
    delay.delay_ms(HOLD_MS);
    Ok(())
}

// Original partial-window test from commit f6a5e19. Sends only window bytes.
fn test_partial_window(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Partial red window at ({PARTIAL_X}, {PARTIAL_Y})");
    let red = HexColor::colors_byte(HexColor::Red, HexColor::Red);
    let partial = vec![red; (PARTIAL_WIDTH * PARTIAL_HEIGHT / 2) as usize];
    epd.update_partial_frame(spi, delay, &partial, PARTIAL_X, PARTIAL_Y, PARTIAL_WIDTH, PARTIAL_HEIGHT)?;
    epd.display_frame(spi, delay)?;
    delay.delay_ms(HOLD_MS);
    Ok(())
}

// Six-color striped framebuffer, retained from the earlier Rust demonstration.
fn test_color_stripes(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Six-color stripes");
    let mut image = new_image();
    let colors = [HexColor::Black, HexColor::White, HexColor::Yellow, HexColor::Red, HexColor::Blue, HexColor::Green];
    for (index, color) in colors.iter().enumerate() {
        let x = index as u32 * (WIDTH / colors.len() as u32);
        let width = if index + 1 == colors.len() { WIDTH - x } else { WIDTH / colors.len() as u32 };
        Rectangle::new(Point::new(x as i32, 0), Size::new(width, HEIGHT))
            .into_styled(PrimitiveStyle::with_fill(*color))
            .draw(&mut *image)?;
    }
    show_image(epd, spi, delay, &image)
}

// Waveshare's C example displays ./pic/7in3e.bmp. This decoder accepts
// uncompressed 24- or 32-bit Windows BMPs and maps RGB to the six EPD colors.
// Other BMP types can be converted with ImageMagick: magick input.bmp -type TrueColor BMP3:pic/7in3e.bmp
fn read_bmp(path: &Path, image: &mut Display7in3e) -> TestResult {
    let bytes = fs::read(path)?;
    if bytes.len() < 54 || &bytes[..2] != b"BM" {
        return Err("not a BMP file".into());
    }
    let get_u16 = |i: usize| -> u16 { u16::from_le_bytes([bytes[i], bytes[i + 1]]) };
    let get_u32 = |i: usize| -> u32 { u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) };
    let pixel_offset = get_u32(10) as usize;
    let dib_size = get_u32(14);
    let width = i32::from_le_bytes(bytes[18..22].try_into()?);
    let signed_height = i32::from_le_bytes(bytes[22..26].try_into()?);
    let planes = get_u16(26);
    let bpp = get_u16(28);
    let compression = get_u32(30);
    if dib_size < 40 || width <= 0 || signed_height == 0 || signed_height == i32::MIN
        || planes != 1 || !matches!(bpp, 24 | 32) || compression != 0 {
        return Err("BMP must be an uncompressed 24/32-bit Windows BMP".into());
    }
    let w = width as usize;
    let h = signed_height.unsigned_abs() as usize;
    let row_bytes = (w.checked_mul(bpp as usize).ok_or("BMP too wide")? + 31) / 32 * 4;
    let total = row_bytes.checked_mul(h).and_then(|n| n.checked_add(pixel_offset)).ok_or("BMP size overflow")?;
    if total > bytes.len() { return Err("truncated BMP pixel data".into()); }
    for y in 0..h.min(HEIGHT as usize) {
        let src_y = if signed_height > 0 { h - 1 - y } else { y };
        for x in 0..w.min(WIDTH as usize) {
            let i = pixel_offset + src_y * row_bytes + x * (bpp as usize / 8);
            let (b, g, r) = (bytes[i], bytes[i + 1], bytes[i + 2]);
            let color = nearest_color(r, g, b);
            Pixel(Point::new(x as i32, y as i32), color).draw(image)?;
        }
    }
    Ok(())
}

fn nearest_color(r: u8, g: u8, b: u8) -> HexColor {
    // Approximate RGB reference colors; the panel has its own color rendition.
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
    println!("BMP example: {}", path.display());
    if !path.exists() {
        println!("BMP not found: skipping (pass --bmp PATH to provide one)");
        return Ok(());
    }
    let mut image = new_image();
    read_bmp(path, &mut image)?;
    show_image(epd, spi, delay, &image)
}

fn test_points(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Points example");
    let mut image = new_image();
    for (x, y, size, color) in [(10, 80, 1, HexColor::Red), (10, 90, 2, HexColor::Blue), (10, 100, 3, HexColor::Green)] {
        Rectangle::new(Point::new(x, y), Size::new(size, size))
            .into_styled(PrimitiveStyle::with_fill(color)).draw(&mut *image)?;
    }
    show_image(epd, spi, delay, &image)
}

fn test_lines(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Solid and dotted lines example");
    let mut image = new_image();
    for (start, end) in [(Point::new(20, 70), Point::new(70, 120)), (Point::new(70, 70), Point::new(20, 120))] {
        Line::new(start, end).into_styled(PrimitiveStyle::with_stroke(HexColor::Yellow, 1)).draw(&mut *image)?;
    }
    // Dotted horizontal and vertical lines from the C demo.
    for x in (85..=125).step_by(4) {
        Pixel(Point::new(x, 95), HexColor::Yellow).draw(&mut *image)?;
    }
    for y in (75..=115).step_by(4) {
        Pixel(Point::new(105, y), HexColor::Yellow).draw(&mut *image)?;
    }
    show_image(epd, spi, delay, &image)
}

fn test_rectangles(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Outlined and filled rectangles example");
    let mut image = new_image();
    Rectangle::new(Point::new(20, 70), Size::new(51, 51))
        .into_styled(PrimitiveStyle::with_stroke(HexColor::Black, 1)).draw(&mut *image)?;
    Rectangle::new(Point::new(80, 70), Size::new(51, 51))
        .into_styled(PrimitiveStyle::with_fill(HexColor::Black)).draw(&mut *image)?;
    show_image(epd, spi, delay, &image)
}

fn test_circles(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Outlined and filled circles example");
    let mut image = new_image();
    Circle::new(Point::new(25, 75), 40)
        .into_styled(PrimitiveStyle::with_stroke(HexColor::Black, 1)).draw(&mut *image)?;
    Rectangle::new(Point::new(80, 70), Size::new(51, 51))
        .into_styled(PrimitiveStyle::with_fill(HexColor::Black)).draw(&mut *image)?;
    Circle::new(Point::new(85, 75), 40)
        .into_styled(PrimitiveStyle::with_fill(HexColor::White)).draw(&mut *image)?;
    show_image(epd, spi, delay, &image)
}

fn draw_text(image: &mut Display7in3e, x: i32, y: i32, text: &str, color: HexColor, large: bool) -> TestResult {
    if large {
        Text::with_baseline(text, Point::new(x, y), MonoTextStyle::new(&FONT_10X20, color), Baseline::Top).draw(image)?;
    } else {
        Text::with_baseline(text, Point::new(x, y), MonoTextStyle::new(&FONT_6X10, color), Baseline::Top).draw(image)?;
    }
    Ok(())
}

fn test_numbers(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("Numbers example");
    let mut image = new_image();
    draw_text(&mut image, 10, 33, "123456789", HexColor::Black, false)?;
    Rectangle::new(Point::new(10, 50), Size::new(120, 22))
        .into_styled(PrimitiveStyle::with_fill(HexColor::Black)).draw(&mut *image)?;
    draw_text(&mut image, 10, 50, "987654321", HexColor::White, false)?;
    show_image(epd, spi, delay, &image)
}

fn test_english_text(epd: &mut DisplayDriver, spi: &mut SpidevDevice, delay: &mut Delay) -> TestResult {
    println!("English text / color examples");
    let mut image = new_image();
    let colors = [HexColor::Black, HexColor::Green, HexColor::Blue, HexColor::Red, HexColor::Yellow];
    for (row, color) in colors.iter().enumerate() {
        draw_text(&mut image, 400, row as i32 * 20, "waveshare", *color, false)?;
    }
    // Colored text on black background, like the original Paint_DrawString_EN example.
    Rectangle::new(Point::new(150, 0), Size::new(230, 205))
        .into_styled(PrimitiveStyle::with_fill(HexColor::Black)).draw(&mut *image)?;
    for (row, color) in [HexColor::White, HexColor::Green, HexColor::Blue, HexColor::Red, HexColor::Yellow].iter().enumerate() {
        draw_text(&mut image, 150, row as i32 * 30, "hello world", *color, true)?;
    }
    Rectangle::new(Point::new(150, 180), Size::new(230, 22))
        .into_styled(PrimitiveStyle::with_fill(HexColor::Yellow)).draw(&mut *image)?;
    draw_text(&mut image, 150, 180, "hello world", HexColor::Black, true)?;
    show_image(epd, spi, delay, &image)
}

// The upstream C test uses Font12CN and Font24CN; this repository does not
// bundle those Chinese font bitmaps. Replace this function with a Chinese
// bitmap-font renderer when the assets are available.
fn test_chinese_text(_epd: &mut DisplayDriver, _spi: &mut SpidevDevice, _delay: &mut Delay) -> TestResult {
    println!("Chinese text example skipped: requires Font12CN / Font24CN assets");
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
    let mut bmp_path = "pic/7in3e.bmp".to_owned();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--test" => test = args.next().ok_or("--test requires a value")?,
            "--bmp" => bmp_path = args.next().ok_or("--bmp requires a path")?,
            _ => return Err(format!("unknown argument: {arg}; use --test NAME or --bmp PATH").into()),
        }
    }

    let mut spi = SpidevDevice::open(SPI_PATH)?;
    let options = SpidevOptions::new()
        .bits_per_word(8)
        .max_speed_hz(SPI_SPEED_HZ)
        .mode(spidev::SpiModeFlags::SPI_MODE_0)
        .build();
    spi.configure(&options)?;

    let mut chip = Chip::new(GPIO_CHIP)?;
    let busy = request_pin(&mut chip, BUSY_PIN, LineRequestFlags::INPUT, 0, "epd-busy")?;
    let dc = request_pin(&mut chip, DC_PIN, LineRequestFlags::OUTPUT, 0, "epd-dc")?;
    let rst = request_pin(&mut chip, RST_PIN, LineRequestFlags::OUTPUT, 1, "epd-rst")?;
    let mut delay = Delay {};
    let mut epd = Epd7in3e::new(&mut spi, busy, dc, rst, &mut delay, None)?;

    let run = |name: &str| test == "all" || test == name;
    let known = ["all", "blocks", "partial", "stripes", "bmp", "points", "lines", "rectangles", "circles", "numbers", "text", "chinese", "clear"];
    if !known.contains(&test.as_str()) {
        return Err(format!("unknown test '{test}'; choose from {}", known.join(", ")).into());
    }
    let result = (|| -> TestResult {
        if run("stripes") { test_color_stripes(&mut epd, &mut spi, &mut delay)?; }
        if run("bmp") { test_bmp(&mut epd, &mut spi, &mut delay, Path::new(&bmp_path))?; }
        if run("points") { test_points(&mut epd, &mut spi, &mut delay)?; }
        if run("lines") { test_lines(&mut epd, &mut spi, &mut delay)?; }
        if run("rectangles") { test_rectangles(&mut epd, &mut spi, &mut delay)?; }
        if run("circles") { test_circles(&mut epd, &mut spi, &mut delay)?; }
        if run("numbers") { test_numbers(&mut epd, &mut spi, &mut delay)?; }
        if run("text") { test_english_text(&mut epd, &mut spi, &mut delay)?; }
        if run("chinese") { test_chinese_text(&mut epd, &mut spi, &mut delay)?; }
        if run("clear") { test_clear(&mut epd, &mut spi, &mut delay)?; }
        if run("blocks") { test_six_blocks(&mut epd, &mut spi, &mut delay)?; }
        if run("partial") { test_partial_window(&mut epd, &mut spi, &mut delay)?; }
        Ok(())
    })();
    // Try to put the display into deep sleep even if a test failed.
    let sleep_result = epd.sleep(&mut spi, &mut delay);
    result?;
    sleep_result?;
    delay.delay_ms(2_000);
    Ok(())
}

