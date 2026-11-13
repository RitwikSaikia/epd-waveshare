//! Run on a Raspberry Pi with SPI enabled:
//! `cargo run --example epd4in26 --features graphics,linux-dev`

use embedded_graphics::{
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use embedded_hal::delay::DelayNs;
use epd_waveshare::{
    epd4in26::{Display4in26, Epd4in26, HEIGHT, WIDTH},
    prelude::*,
};
use linux_embedded_hal::{
    gpio_cdev::{Chip, LineRequestFlags},
    spidev::{self, SpidevOptions},
    CdevPin, Delay, SPIError, SpidevDevice,
};

const SPI_PATH: &str = "/dev/spidev0.0";
const GPIO_CHIP_PATH: &str = "/dev/gpiochip0";
const SPI_SPEED_HZ: u32 = 2_000_000;

// Raspberry Pi BCM GPIO numbers used by the Waveshare HAT.
const BUSY_PIN: u32 = 24;
const DC_PIN: u32 = 25;
const RST_PIN: u32 = 17;
const POWER_PIN: u32 = 18;

fn request_pin(
    chip: &mut Chip,
    number: u32,
    flags: LineRequestFlags,
    initial_value: u8,
) -> CdevPin {
    let handle = chip
        .get_line(number)
        .expect("getting GPIO line failed")
        .request(flags, initial_value, "epd4in26")
        .expect("requesting GPIO line failed");
    CdevPin::new(handle).expect("reading GPIO line info failed")
}

fn main() -> Result<(), SPIError> {
    let mut spi = SpidevDevice::open(SPI_PATH)?;
    let options = SpidevOptions::new()
        .bits_per_word(8)
        .max_speed_hz(SPI_SPEED_HZ)
        .mode(spidev::SpiModeFlags::SPI_MODE_0)
        .build();
    spi.configure(&options)?;

    let mut gpio = Chip::new(GPIO_CHIP_PATH).expect("opening GPIO chip failed");
    let _power = request_pin(&mut gpio, POWER_PIN, LineRequestFlags::OUTPUT, 1);
    let busy = request_pin(&mut gpio, BUSY_PIN, LineRequestFlags::INPUT, 0);
    let dc = request_pin(&mut gpio, DC_PIN, LineRequestFlags::OUTPUT, 0);
    let rst = request_pin(&mut gpio, RST_PIN, LineRequestFlags::OUTPUT, 0);
    let mut delay = Delay {};

    let mut epd = Epd4in26::new(&mut spi, busy, dc, rst, &mut delay, None)?;
    let mut display = Box::new(Display4in26::default());
    display.clear(Gray2Color::White).unwrap();

    let colors = [
        Gray2Color::Black,
        Gray2Color::DarkGray,
        Gray2Color::LightGray,
        Gray2Color::White,
    ];
    let stripe_width = WIDTH / colors.len() as u32;

    for (index, color) in colors.iter().enumerate() {
        let x = index as u32 * stripe_width;
        let width = if index + 1 == colors.len() {
            WIDTH - x
        } else {
            stripe_width
        };
        Rectangle::new(Point::new(x as i32, 0), Size::new(width, HEIGHT))
            .into_styled(PrimitiveStyle::with_fill(*color))
            .draw(&mut *display)
            .unwrap();
    }

    println!("Displaying four-level grayscale test pattern");
    epd.update_and_display_frame(&mut spi, display.buffer(), &mut delay)?;
    delay.delay_ms(5_000);

    println!("Putting display to sleep");
    epd.sleep(&mut spi, &mut delay)
}
