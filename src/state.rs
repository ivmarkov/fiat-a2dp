use embassy_sync::{blocking_mutex::raw::RawMutex, signal::Signal};
use esp_idf_svc::hal::task::embassy_sync::EspRawMutex;

pub type DisplayString = heapless::String<32>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BtState {
    Uninitialized,
    Initialized,
    Paired,
    Connected(DisplayString),
}

impl BtState {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TrackInfo {
    pub artist: DisplayString,
    pub album: DisplayString,
    pub song: DisplayString,
    pub offset: core::time::Duration,
    pub duration: core::time::Duration,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AudioState {
    Uninitialized,
    Initialized,
    Connected,
    Playing(TrackInfo),
    Paused(TrackInfo),
}

impl AudioState {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected) || self.is_active()
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Playing(_) | Self::Paused(_))
    }

    pub fn track_info(&self) -> Option<&TrackInfo> {
        match self {
            Self::Playing(track_info) | Self::Paused(track_info) => Some(track_info),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PhoneCallInfo {
    pub phone: DisplayString,
    pub duration: core::time::Duration,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PhoneState {
    Uninitialized,
    Initialized,
    Connected,
    Dialing(DisplayString),
    Ringing(DisplayString),
    CallActive(PhoneCallInfo),
}

impl PhoneState {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected) || self.is_active()
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Dialing(_) | Self::Ringing(_) | Self::CallActive(_)
        )
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RadioState {
    Unknown,
    Fm,
    BtActive,
    BtMuted,
}

impl RadioState {
    pub fn is_bt_active(&self) -> bool {
        matches!(self, Self::BtActive)
    }
}

pub type StateSignal<T> = Signal<EspRawMutex, T>;

pub fn signal_all<M, T>(signals: &[&Signal<M, T>], data: T)
where
    M: RawMutex,
    T: Clone + Send,
{
    for signal in signals {
        signal.signal(data.clone());
    }
}
