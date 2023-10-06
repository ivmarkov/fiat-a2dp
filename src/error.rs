use core::fmt::{Display, Formatter, Result};

//use futures::task::SpawnError;
use esp_idf_svc::sys::EspError;

#[derive(Debug)]
pub enum Error {
    EspError(EspError),
    //SpawnError(SpawnError),
}

impl From<EspError> for Error {
    fn from(error: EspError) -> Self {
        Self::EspError(error)
    }
}

// impl From<SpawnError> for Error {
//     fn from(error: SpawnError) -> Self {
//         Self::SpawnError(error)
//     }
// }

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::EspError(error) => error.fmt(f),
            //Self::SpawnError(error) => error.fmt(f),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}
