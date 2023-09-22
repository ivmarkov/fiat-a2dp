use embassy_time::{Duration, Timer};
use esp_idf_svc::{
    hal::{
        gpio::{Gpio12, Gpio13, Output, PinDriver},
        peripheral::Peripheral,
    },
    sys::{EspError, ESP_FAIL},
};

use crate::error::Error;

pub struct FlashMode<'d> {
    flash: PinDriver<'d, Gpio12, Output>,
    reset: PinDriver<'d, Gpio13, Output>,
}

impl<'d> FlashMode<'d> {
    pub fn new(
        flash: impl Peripheral<P = Gpio12> + 'd,
        reset: impl Peripheral<P = Gpio13> + 'd,
    ) -> Result<Self, Error> {
        Ok(Self {
            flash: PinDriver::output(flash)?,
            reset: PinDriver::output(reset)?,
        })
    }

    pub async fn enter(&mut self) -> Result<(), Error> {
        self.flash.set_low()?;

        // Give some time to the capacitor to charge itself
        Timer::after(Duration::from_millis(500)).await;

        self.reset.set_low()?;

        // Give some time to reset to trigger
        Timer::after(Duration::from_millis(500)).await;

        Err(Error::EspError(EspError::from_infallible::<ESP_FAIL>()))
    }
}
