//! Driver for the Waveshare 4.26inch e-Paper HAT Display via SPI
//!
//! # References
//!
//! - [Datasheet](https://www.waveshare.com/wiki/4.26inch_e-Paper_HAT)
//! - [Waveshare C driver](https://github.com/waveshareteam/e-Paper/blob/master/RaspberryPi_JetsonNano/c/lib/e-Paper/EPD_4in26.c)
//! - [Waveshare Python driver](https://github.com/waveshareteam/e-Paper/blob/master/RaspberryPi_JetsonNano/python/lib/waveshare_epd/epd4in26.py)
use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};

use crate::{
    color::Gray2Color,
    interface::DisplayInterface,
    traits::{InternalWiAdditions, RefreshLut, WaveshareDisplay},
};
use command::Command;
use constants::LUT_4GRAY;

mod command;
mod constants;

/// Display width in pixels.
pub const WIDTH: u32 = 800;
/// Display height in pixels.
pub const HEIGHT: u32 = 480;
/// Number of bytes in a packed two-bit grayscale frame.
pub const FRAME_BUFFER_SIZE: usize = WIDTH as usize * HEIGHT as usize / 4;
/// Default background used by `clear_frame`.
pub const DEFAULT_BACKGROUND_COLOR: Gray2Color = Gray2Color::White;

const PLANE_ROW_SIZE: usize = WIDTH as usize / 8;

/// Full-size packed grayscale buffer for use with embedded-graphics.
#[cfg(feature = "graphics")]
pub type Display4in26 =
    crate::graphics::Display<WIDTH, HEIGHT, false, FRAME_BUFFER_SIZE, Gray2Color>;

/// Driver for the Waveshare 4.26-inch display in four-level grayscale mode.
pub struct Epd4in26<SPI, BUSY, DC, RST, DELAY> {
    interface: DisplayInterface<SPI, BUSY, DC, RST, DELAY, false>,
    background_color: Gray2Color,
}

impl<SPI, BUSY, DC, RST, DELAY> InternalWiAdditions<SPI, BUSY, DC, RST, DELAY>
    for Epd4in26<SPI, BUSY, DC, RST, DELAY>
where
    SPI: SpiDevice,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayNs,
{
    fn init(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        // The interface's 200 ms post-reset delay covers the reference's two
        // consecutive 100 ms delays after raising RST.
        self.interface.reset(delay, 100_000, 2_000);
        self.wait_until_idle(spi, delay)?;
        self.interface.cmd(spi, Command::SwReset)?;
        self.wait_until_idle(spi, delay)?;
        self.interface
            .cmd_with_data(spi, Command::TemperatureSensor, &[0x80])?;
        self.interface.cmd_with_data(
            spi,
            Command::BoosterSoftStart,
            &[0xAE, 0xC7, 0xC3, 0xC0, 0x80],
        )?;
        // The controller's gate count follows the panel height.
        let gates = (HEIGHT as u16 - 1).to_le_bytes();
        self.interface.cmd_with_data(
            spi,
            Command::DriverOutputControl,
            &[gates[0], gates[1], 0x02],
        )?;
        self.interface
            .cmd_with_data(spi, Command::BorderWaveform, &[0x01])?;
        self.interface
            .cmd_with_data(spi, Command::DataEntryMode, &[0x01])?;
        self.interface
            .cmd_with_data(spi, Command::RamXWindow, &[0x00, 0x00, 0x1F, 0x03])?;
        self.interface
            .cmd_with_data(spi, Command::RamYWindow, &[0xDF, 0x01, 0x00, 0x00])?;
        self.reset_cursor(spi)?;
        self.set_lut(spi, delay, None)
    }
}

impl<SPI, BUSY, DC, RST, DELAY> WaveshareDisplay<SPI, BUSY, DC, RST, DELAY>
    for Epd4in26<SPI, BUSY, DC, RST, DELAY>
where
    SPI: SpiDevice,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayNs,
{
    type DisplayColor = Gray2Color;

    fn new(
        spi: &mut SPI,
        busy: BUSY,
        dc: DC,
        rst: RST,
        delay: &mut DELAY,
        delay_us: Option<u32>,
    ) -> Result<Self, SPI::Error> {
        let mut epd = Self {
            interface: DisplayInterface::new(busy, dc, rst, Some(delay_us.unwrap_or(20_000))),
            background_color: DEFAULT_BACKGROUND_COLOR,
        };
        epd.init(spi, delay)?;
        Ok(epd)
    }

    fn sleep(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;
        self.interface
            .cmd_with_data(spi, Command::DeepSleep, &[0x03])?;
        delay.delay_ms(100);
        Ok(())
    }

    fn wake_up(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.init(spi, delay)
    }

    fn set_background_color(&mut self, color: Gray2Color) {
        self.background_color = color;
    }

    fn background_color(&self) -> &Gray2Color {
        &self.background_color
    }

    fn width(&self) -> u32 {
        WIDTH
    }
    fn height(&self) -> u32 {
        HEIGHT
    }

    /// Upload a packed grayscale frame without refreshing the panel.
    ///
    /// # Panics
    /// Panics unless `buffer` contains exactly [`FRAME_BUFFER_SIZE`] bytes.
    fn update_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        assert_eq!(
            buffer.len(),
            FRAME_BUFFER_SIZE,
            "expected a full packed grayscale frame"
        );
        self.wait_until_idle(spi, delay)?;
        let mut row = [0u8; PLANE_ROW_SIZE];
        for (command, bit) in [(Command::WriteRam, 0), (Command::WriteRam2, 1)] {
            self.reset_cursor(spi)?;
            self.interface.cmd(spi, command)?;
            for packed_row in buffer.chunks_exact(PLANE_ROW_SIZE * 2) {
                for (out, pixels) in row.iter_mut().zip(packed_row.chunks_exact(2)) {
                    *out = plane_byte([pixels[0], pixels[1]], bit);
                }
                self.interface.data(spi, &row)?;
            }
        }
        Ok(())
    }

    /// Partial grayscale updates are unsupported.
    ///
    /// # Panics
    /// Always panics; use `update_frame` with a full grayscale frame.
    fn update_partial_frame(
        &mut self,
        _spi: &mut SPI,
        _delay: &mut DELAY,
        _buffer: &[u8],
        _x: u32,
        _y: u32,
        _width: u32,
        _height: u32,
    ) -> Result<(), SPI::Error> {
        panic!("partial updates are not supported in 4.26-inch grayscale mode")
    }

    fn display_frame(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;
        self.interface
            .cmd_with_data(spi, Command::DisplayUpdateControl, &[0xC7])?;
        self.interface.cmd(spi, Command::MasterActivation)?;
        self.wait_until_idle(spi, delay)
    }

    fn update_and_display_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        self.update_frame(spi, buffer, delay)?;
        self.display_frame(spi, delay)
    }

    fn clear_frame(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;
        let packed = self.background_color.get_byte_value();
        for (command, bit) in [(Command::WriteRam, 0), (Command::WriteRam2, 1)] {
            self.reset_cursor(spi)?;
            self.interface.cmd(spi, command)?;
            let row = [plane_byte([packed; 2], bit); PLANE_ROW_SIZE];
            for _ in 0..HEIGHT {
                self.interface.data(spi, &row)?;
            }
        }
        Ok(())
    }

    /// Reload the full grayscale LUT and its voltage settings.
    ///
    /// # Panics
    /// Panics for `Some(RefreshLut::Quick)`, which is not a grayscale waveform.
    fn set_lut(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        refresh_rate: Option<RefreshLut>,
    ) -> Result<(), SPI::Error> {
        assert_ne!(
            refresh_rate,
            Some(RefreshLut::Quick),
            "quick grayscale refresh is unsupported"
        );
        self.wait_until_idle(spi, delay)?;
        self.interface
            .cmd_with_data(spi, Command::WriteLut, &LUT_4GRAY[..105])?;
        self.interface
            .cmd_with_data(spi, Command::GateVoltage, &LUT_4GRAY[105..106])?;
        self.interface
            .cmd_with_data(spi, Command::SourceVoltage, &LUT_4GRAY[106..109])?;
        self.interface
            .cmd_with_data(spi, Command::VcomVoltage, &LUT_4GRAY[109..110])
    }

    fn wait_until_idle(&mut self, _spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.interface.wait_until_idle(delay, false);
        // The reference also waits 20 ms after BUSY is released.
        delay.delay_ms(20);
        Ok(())
    }
}

impl<SPI, BUSY, DC, RST, DELAY> Epd4in26<SPI, BUSY, DC, RST, DELAY>
where
    SPI: SpiDevice,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayNs,
{
    fn reset_cursor(&mut self, spi: &mut SPI) -> Result<(), SPI::Error> {
        self.interface
            .cmd_with_data(spi, Command::RamXCounter, &[0, 0])?;
        self.interface
            .cmd_with_data(spi, Command::RamYCounter, &[0, 0])
    }
}

// The reference maps black/dark/light/white to (RAM 0x24, RAM 0x26):
// (1,1), (0,1), (1,0), (0,0). These are the inverted low/high pixel bits.
fn plane_byte(packed: [u8; 2], bit: u8) -> u8 {
    let mut result = 0;
    for byte in packed {
        for shift in [6, 4, 2, 0] {
            result = (result << 1) | ((!byte >> (shift + bit)) & 1);
        }
    }
    result
}
