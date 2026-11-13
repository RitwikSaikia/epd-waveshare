//! Commands for the Waveshare 4.26" E-Ink Display.

use crate::traits;

#[derive(Clone, Copy)]
pub(crate) enum Command {
    DriverOutputControl = 0x01,
    GateDrivingVoltage = 0x03,
    SourceDrivingVoltage = 0x04,
    DeepSleep = 0x10,
    DataEntryMode = 0x11,
    SwReset = 0x12,
    TemperatureSensor = 0x18,
    DisplayUpdateControl = 0x22,
    MasterActivation = 0x20,
    WriteRam = 0x24,
    WriteRam2 = 0x26,
    WriteVcom = 0x2C,
    WriteLut = 0x32,
    BorderWaveform = 0x3C,
    SetRamX = 0x44,
    SetRamY = 0x45,
    SetRamXCounter = 0x4E,
    SetRamYCounter = 0x4F,
    BoosterSoftStart = 0x0C,
}

impl traits::Command for Command {
    fn address(self) -> u8 {
        self as u8
    }
}
