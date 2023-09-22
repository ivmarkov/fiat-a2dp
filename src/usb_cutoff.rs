use esp_idf_svc::hal::{
    gpio::{AnyOutputPin, Output, OutputPin, PinDriver},
    into_ref,
    peripheral::Peripheral,
};

use crate::error::Error;

pub struct UsbCutoff<'d>(PinDriver<'d, AnyOutputPin, Output>);

impl<'d> UsbCutoff<'d> {
    pub fn new(cutoff: impl Peripheral<P = impl OutputPin> + 'd) -> Result<Self, Error> {
        into_ref!(cutoff);

        Ok(Self(PinDriver::output(cutoff.map_into())?))
    }

    pub fn cutoff(&mut self) -> Result<(), Error> {
        self.0.set_high()?;

        Ok(())
    }
}
