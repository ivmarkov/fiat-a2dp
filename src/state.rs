use embassy_sync::{blocking_mutex::raw::RawMutex, signal::Signal};

use enumset::EnumSetType;

pub type DisplayString = heapless::String<32>;

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BtState {
    Uninitialized,
    Initialized,
    Paired,
    Connected,
}

impl BtState {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AudioState {
    Uninitialized,
    Initialized,
    Connected,
    Streaming,
    Suspended,
}

impl AudioState {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected) || self.is_active()
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Streaming)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TrackInfo {
    pub version: u32,
    pub artist: DisplayString,
    pub album: DisplayString,
    pub song: DisplayString,
    pub offset: core::time::Duration,
    pub duration: core::time::Duration,
    pub paused: bool,
}

impl TrackInfo {
    pub const fn new() -> Self {
        Self {
            version: 0,
            artist: DisplayString::new(),
            album: DisplayString::new(),
            song: DisplayString::new(),
            offset: core::time::Duration::from_secs(0),
            duration: core::time::Duration::from_secs(0),
            paused: false,
        }
    }

    pub fn reset(&mut self) {
        self.artist.clear();
        self.album.clear();
        self.song.clear();
        self.offset = core::time::Duration::from_secs(0);
        self.duration = core::time::Duration::from_secs(0);
        self.paused = false;
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AudioTrackState {
    Uninitialized,
    Initialized,
    Connected,
    Playing,
    Paused,
}

impl AudioTrackState {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected) || self.is_active()
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Playing | Self::Paused)
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PhoneCallInfo {
    pub version: u32,
    pub phone: DisplayString,
    pub duration: core::time::Duration,
}

impl PhoneCallInfo {
    pub const fn new() -> Self {
        Self {
            version: 0,
            phone: DisplayString::new(),
            duration: core::time::Duration::from_secs(0),
        }
    }

    pub fn reset(&mut self) {
        self.phone.clear();
        self.duration = core::time::Duration::from_secs(0);
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum PhoneCallState {
    Idle,
    Dialing,
    DialingAlerting,
    Ringing,
    CallActive,
}

impl PhoneCallState {
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
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
    CockpitDisplay,
    RadioDisplay,
    Commands,
}

pub struct State<M, T>([Signal<M, T>; 8])
where
    M: RawMutex;

impl<M, T> State<M, T>
where
    M: RawMutex,
{
    const INIT: Signal<M, T> = Signal::new();

    pub const fn new() -> Self {
        Self([Self::INIT; 8])
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

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Command {
    Answer,
    Reject,
    Hangup,
    Pause,
    Resume,
    NextTrack,
    PreviousTrack,
}
