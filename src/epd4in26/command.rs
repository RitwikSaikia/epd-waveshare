//! Commands used by the 4.26-inch grayscale driver.

#[derive(Clone, Copy)]
pub(crate) enum Command {
    DriverOutputControl = 0x01,
    GateVoltage = 0x03,
    SourceVoltage = 0x04,
    BoosterSoftStart = 0x0C,
    DeepSleep = 0x10,
    DataEntryMode = 0x11,
    SwReset = 0x12,
    TemperatureSensor = 0x18,
    MasterActivation = 0x20,
    DisplayUpdateControl = 0x22,
    WriteRam = 0x24,
    WriteRam2 = 0x26,
    VcomVoltage = 0x2C,
    WriteLut = 0x32,
    BorderWaveform = 0x3C,
    RamXWindow = 0x44,
    RamYWindow = 0x45,
    RamXCounter = 0x4E,
    RamYCounter = 0x4F,
}

impl crate::traits::Command for Command {
    fn address(self) -> u8 {
        self as u8
    }
}
