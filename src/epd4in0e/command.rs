use crate::traits;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Command {
    PanelSetting = 0x00,
    PowerSetting = 0x01,
    PowerOff = 0x02,
    PowerOffSequenceSetting = 0x03,
    PowerOn = 0x04,
    BoosterSoftStart = 0x05,
    BoosterSoftStart2 = 0x06,
    DeepSleep = 0x07,
    BoosterSoftStart3 = 0x08,
    DataStartTransmission = 0x10,
    DisplayRefresh = 0x12,
    PllControl = 0x30,
    VcomAndDataIntervalSetting = 0x50,
    TconSetting = 0x60,
    ResolutionSetting = 0x61,
    PowerSaving = 0x84,
    Header = 0xAA,
    CascadeSetting = 0xE3,
}

impl traits::Command for Command {
    fn address(self) -> u8 {
        self as u8
    }
}
