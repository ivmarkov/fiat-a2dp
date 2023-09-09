use embassy_sync::{blocking_mutex::raw::RawMutex, signal::Signal};

use enumset::EnumSetType;

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

#[derive(Debug, EnumSetType)]
pub enum Service {
    Can,
    AudioOutgoing,
    AudioIncoming,
    AudioState,
    Bt,
}

pub struct State<M, T>([Signal<M, T>; 4])
where
    M: RawMutex;

impl<M, T> State<M, T>
where
    M: RawMutex,
{
    const INIT: Signal<M, T> = Signal::new();

    pub const fn new() -> Self {
        Self([Self::INIT; 4])
    }

    pub fn receiver(&self, service: Service) -> Receiver<'_, M, T> {
        let index = service as usize;

        Receiver(&self.0[index])
    }

    pub fn sender(&self) -> Sender<'_, M, T> {
        Sender(&self.0)
    }
}

pub struct Receiver<'a, M, T>(&'a Signal<M, T>)
where
    M: RawMutex;

impl<'a, M, T> Receiver<'a, M, T>
where
    M: RawMutex,
    T: Send,
{
    pub async fn recv(&self) -> T {
        self.0.wait().await
    }
}

pub struct Sender<'a, M, T>(&'a [Signal<M, T>])
where
    M: RawMutex;

impl<'a, M, T> Sender<'a, M, T>
where
    M: RawMutex,
    T: Send + Clone,
{
    pub fn send(&self, value: T) {
        for signal in self.0 {
            signal.signal(value.clone());
        }
    }
}
