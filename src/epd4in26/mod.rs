//! Driver for the Waveshare 4.26" (800 x 480) four-gray E-Ink display.
//!
//! The graphics buffer contains four 2-bit grayscale pixels per byte. The
//! driver converts it to the two 1-bit RAM planes used by this panel.

use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};

use crate::color::Gray2Color;
use crate::interface::DisplayInterface;
use crate::traits::{InternalWiAdditions, RefreshLut, WaveshareDisplay};

mod command;
mod constants;
use command::Command;
use constants::LUT_4GRAY;

/// Display width in pixels.
pub const WIDTH: u32 = 800;
/// Display height in pixels.
pub const HEIGHT: u32 = 480;
/// Default background color.
pub const DEFAULT_BACKGROUND_COLOR: Gray2Color = Gray2Color::White;
/// Two bits per pixel (four pixels per byte).
pub const BUFFER_SIZE: usize = (WIDTH * HEIGHT / 4) as usize;
const PLANE_ROW_BYTES: usize = (WIDTH / 8) as usize;
const IS_BUSY_LOW: bool = false; // BUSY is high while the panel is working.
const SINGLE_BYTE_WRITE: bool = false;

/// Full-size 2-bpp embedded-graphics buffer for this panel.
#[cfg(feature = "graphics")]
pub type Display4in26 = crate::graphics::Display<WIDTH, HEIGHT, false, BUFFER_SIZE, Gray2Color>;
/// Waveshare 4.26" four-gray display driver.
pub struct Epd4in26<SPI, BUSY, DC, RST, DELAY> {
    interface: DisplayInterface<SPI, BUSY, DC, RST, DELAY, SINGLE_BYTE_WRITE>,
    background_color: Gray2Color,
    partial_mode: bool,
}
// Convert four packed 2-bit pixels into four bits of one of the two RAM planes.
// Waveshare's encoding: 00 black, 01 dark gray, 10 light gray, 11 white.
// Plane 0 (0x24) is 1 for 00 or 10; plane 1 (0x26) for 00 or 01.
fn plane_byte(input: &[u8], plane: u8) -> u8 {
    let mut result = 0;
    for &byte in input {
        for shift in [6, 4, 2, 0] {
            let gray = (byte >> shift) & 0x03;
            result = (result << 1) | u8::from(gray & (1 << plane) == 0);
        }
    }
    result
}

// Monochrome partial updates use 1 = white and 0 = black. Only Gray2Color::White
// (11) is white; all other shades are treated as black.
fn partial_byte(input: &[u8]) -> u8 {
    let mut result = 0;
    for &byte in input {
        for shift in [6, 4, 2, 0] {
            result = (result << 1) | u8::from(((byte >> shift) & 0x03) == 0x03);
        }
    }
    result
}

impl<SPI, BUSY, DC, RST, DELAY> Epd4in26<SPI, BUSY, DC, RST, DELAY>
where
    SPI: SpiDevice,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayNs,
{
    fn set_cursor(&mut self, spi: &mut SPI) -> Result<(), SPI::Error> {
        self.interface
            .cmd_with_data(spi, Command::SetRamXCounter, &[0, 0])?;
        self.interface
            .cmd_with_data(spi, Command::SetRamYCounter, &[0, 0])
    }
    fn write_plane(&mut self, spi: &mut SPI, buffer: &[u8], plane: u8) -> Result<(), SPI::Error> {
        let command = if plane == 0 {
            Command::WriteRam
        } else {
            Command::WriteRam2
        };
        self.set_cursor(spi)?;
        self.interface.cmd(spi, command)?;
        let mut row = [0; PLANE_ROW_BYTES];
        for input in buffer.chunks_exact(PLANE_ROW_BYTES * 2) {
            for (out, pixels) in row.iter_mut().zip(input.chunks_exact(2)) {
                *out = plane_byte(pixels, plane);
            }
            self.interface.data(spi, &row)?;
        }
        Ok(())
    }

    // Establish a known monochrome base in both controller RAM planes before
    // activating the partial waveform. A preceding 4-gray frame leaves 0x26
    // containing grayscale plane data, which cannot serve as a mono base.
    // This intentionally clears the entire physical display to white on the
    // first partial update after a four-gray full refresh.
    fn begin_partial_mode(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.interface.reset(delay, 100_000, 2_000);
        self.interface
            .cmd_with_data(spi, Command::TemperatureSensor, &[0x80])?;
        self.interface
            .cmd_with_data(spi, Command::BorderWaveform, &[0x80])?;
        self.interface
            .cmd_with_data(spi, Command::DataEntryMode, &[0x01])?;
        self.set_partial_window(spi, 0, 0, WIDTH, HEIGHT)?;
        let white = [0xFFu8; PLANE_ROW_BYTES];
        for command in [Command::WriteRam, Command::WriteRam2] {
            self.set_partial_window(spi, 0, 0, WIDTH, HEIGHT)?;
            self.interface.cmd(spi, command)?;
            for _ in 0..HEIGHT {
                self.interface.data(spi, &white)?;
            }
        }
        // Full monochrome refresh establishes a clean visible base and
        // synchronized controller memory, as in Waveshare's Display_Base.
        self.interface
            .cmd_with_data(spi, Command::DisplayUpdateControl, &[0xF7])?;
        self.interface.cmd(spi, Command::MasterActivation)?;
        self.wait_until_idle(spi, delay)?;
        self.partial_mode = true;
        Ok(())
    }

    fn set_partial_window(
        &mut self,
        spi: &mut SPI,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), SPI::Error> {
        let x_end = x + width - 1;
        // Waveshare uses X+ / Y- RAM traversal. Start at the bottom of
        // the rectangle and stream rows in the same order as the 4-gray path.
        let y_start = y + height - 1;
        self.interface.cmd_with_data(
            spi,
            Command::SetRamX,
            &[x as u8, (x >> 8) as u8, x_end as u8, (x_end >> 8) as u8],
        )?;
        self.interface.cmd_with_data(
            spi,
            Command::SetRamY,
            &[y_start as u8, (y_start >> 8) as u8, y as u8, (y >> 8) as u8],
        )?;
        self.interface
            .cmd_with_data(spi, Command::SetRamXCounter, &[x as u8, (x >> 8) as u8])?;
        self.interface.cmd_with_data(
            spi,
            Command::SetRamYCounter,
            &[y_start as u8, (y_start >> 8) as u8],
        )
    }
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
        self.partial_mode = false;
        self.interface.reset(delay, 100_000, 2_000);
        delay.delay_ms(100);
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
        self.interface.cmd_with_data(
            spi,
            Command::DriverOutputControl,
            &[(HEIGHT - 1) as u8, ((HEIGHT - 1) >> 8) as u8, 0x02],
        )?;
        self.interface
            .cmd_with_data(spi, Command::BorderWaveform, &[0x01])?;
        self.interface
            .cmd_with_data(spi, Command::DataEntryMode, &[0x01])?;
        self.interface.cmd_with_data(
            spi,
            Command::SetRamX,
            &[0x00, 0x00, (WIDTH - 1) as u8, ((WIDTH - 1) >> 8) as u8],
        )?;
        self.interface.cmd_with_data(
            spi,
            Command::SetRamY,
            &[(HEIGHT - 1) as u8, ((HEIGHT - 1) >> 8) as u8, 0x00, 0x00],
        )?;
        self.set_cursor(spi)?;
        self.wait_until_idle(spi, delay)?;
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
            interface: DisplayInterface::new(busy, dc, rst, delay_us),
            background_color: DEFAULT_BACKGROUND_COLOR,
            partial_mode: false,
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
    fn update_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        assert_eq!(buffer.len(), BUFFER_SIZE);
        // Partial updates reset the controller and use the monochrome waveform.
        // Restore the four-gray setup before sending another full frame.
        if self.partial_mode {
            self.init(spi, delay)?;
        }
        self.wait_until_idle(spi, delay)?;
        self.write_plane(spi, buffer, 0)?;
        self.write_plane(spi, buffer, 1)
    }
    /// Write a rectangle of packed 2-bpp pixels using the monochrome partial
    /// waveform. Only `Gray2Color::White` remains white; all grays become black.
    /// `x` and `width` must be multiples of 8, and `buffer` must contain exactly
    /// `width * height / 4` bytes, packed row-by-row (four pixels per byte).
    /// Call `display_frame` to activate the partial refresh.
    /// The first partial update after full grayscale refresh establishes a
    /// white monochrome base across the panel; draw any retained content
    /// again using partial updates after that transition.
    fn update_partial_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        buffer: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), SPI::Error> {
        assert!(width > 0 && height > 0, "partial region must be nonempty");
        assert!(
            x <= WIDTH && width <= WIDTH - x,
            "partial region exceeds display width"
        );
        assert!(
            y <= HEIGHT && height <= HEIGHT - y,
            "partial region exceeds display height"
        );
        assert_eq!(x % 8, 0, "partial x must be 8-pixel aligned");
        assert_eq!(width % 8, 0, "partial width must be 8-pixel aligned");
        assert_eq!(buffer.len(), (width * height / 4) as usize);

        self.wait_until_idle(spi, delay)?;
        if !self.partial_mode {
            self.begin_partial_mode(spi, delay)?;
        }
        // Match the Waveshare X+/Y- orientation used by four-gray refresh.
        // Using X+/Y+ mirrored the partial image vertically.
        self.interface
            .cmd_with_data(spi, Command::DataEntryMode, &[0x01])?;
        self.interface
            .cmd_with_data(spi, Command::BorderWaveform, &[0x80])?;
        self.set_partial_window(spi, x, y, width, height)?;

        self.interface.cmd(spi, Command::WriteRam)?;
        let bytes_per_row = (width / 4) as usize;
        let mut row = [0u8; PLANE_ROW_BYTES];
        for input in buffer.chunks_exact(bytes_per_row) {
            for (out, pixels) in row[..(width / 8) as usize]
                .iter_mut()
                .zip(input.chunks_exact(2))
            {
                *out = partial_byte(pixels);
            }
            self.interface.data(spi, &row[..(width / 8) as usize])?;
        }
        self.partial_mode = true;
        Ok(())
    }
    fn display_frame(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.interface.cmd_with_data(
            spi,
            Command::DisplayUpdateControl,
            &[if self.partial_mode { 0xFF } else { 0xC7 }],
        )?;
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
        if self.partial_mode {
            self.init(spi, delay)?;
        }
        self.wait_until_idle(spi, delay)?;
        let gray = self.background_color as u8;
        for (command, bit) in [(Command::WriteRam, 0), (Command::WriteRam2, 1)] {
            self.set_cursor(spi)?;
            self.interface.cmd(spi, command)?;
            let value = if gray & (1 << bit) == 0 { 0xFF } else { 0x00 };
            let row = [value; PLANE_ROW_BYTES];
            for _ in 0..HEIGHT {
                self.interface.data(spi, &row)?;
            }
        }
        Ok(())
    }
    fn set_lut(
        &mut self,
        spi: &mut SPI,
        _delay: &mut DELAY,
        _refresh_rate: Option<RefreshLut>,
    ) -> Result<(), SPI::Error> {
        // Only the 4-gray LUT is available for this panel mode.
        self.interface
            .cmd_with_data(spi, Command::WriteLut, &LUT_4GRAY[..105])?;
        self.interface
            .cmd_with_data(spi, Command::GateDrivingVoltage, &LUT_4GRAY[105..106])?;
        self.interface
            .cmd_with_data(spi, Command::SourceDrivingVoltage, &LUT_4GRAY[106..109])?;
        self.interface
            .cmd_with_data(spi, Command::WriteVcom, &LUT_4GRAY[109..110])
    }
    fn wait_until_idle(&mut self, _spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.interface.wait_until_idle(delay, IS_BUSY_LOW);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_gray_plane_encoding() {
        // Black, dark gray, light gray, white, repeated.
        assert_eq!(plane_byte(&[0x1B, 0x1B], 0), 0xAA);
        assert_eq!(plane_byte(&[0x1B, 0x1B], 1), 0xCC);
        assert_eq!(BUFFER_SIZE, 96_000);
    }

    #[test]
    fn partial_conversion_only_white_stays_white() {
        // 00 black, 01 dark gray, 10 light gray, 11 white, repeated.
        assert_eq!(partial_byte(&[0x1B, 0x1B]), 0x11);
        assert_eq!(partial_byte(&[0xFF, 0xFF]), 0xFF);
        assert_eq!(partial_byte(&[0x00, 0x00]), 0x00);
        assert_eq!(partial_byte(&[0xAA, 0x55]), 0x00);
    }
}
