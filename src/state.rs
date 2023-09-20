use std::cell::Cell;

use embassy_sync::blocking_mutex::{raw::NoopRawMutex, Mutex};
use enumset::{EnumSet, EnumSetType};
use esp_idf_svc::hal::task::embassy_sync::EspRawMutex;

use crate::{
    can::message::SteeringWheelButton,
    displays::DisplayText,
    service::ServiceLifecycle,
    signal::{Receiver, SharedStateReceiver, SharedStateSpmcSignal, SpmcSignal},
};

pub type DisplayString = heapless::String<32>;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
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

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
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

#[derive(Debug, Eq, PartialEq)]
pub struct TrackInfo {
    pub version: u32,
    pub state: AudioTrackState,
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
            state: AudioTrackState::Uninitialized,
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

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
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

#[derive(Debug, Eq, PartialEq)]
pub struct PhoneCallInfo {
    pub version: u32,
    pub state: PhoneCallState,
    pub phone: DisplayString,
    pub duration: core::time::Duration,
}

impl PhoneCallInfo {
    pub const fn new() -> Self {
        Self {
            version: 0,
            state: PhoneCallState::Idle,
            phone: DisplayString::new(),
            duration: core::time::Duration::from_secs(0),
        }
    }

    pub fn reset(&mut self) {
        self.phone.clear();
        self.duration = core::time::Duration::from_secs(0);
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
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

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum BtCommand {
    Answer,
    Reject,
    Hangup,
    Pause,
    Resume,
    NextTrack,
    PreviousTrack,
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
    Microphone,
    Speakers,
    AudioMux,
    Bt,
    CockpitDisplay,
    RadioDisplay,
    Commands,
}

pub struct State {
    pub started_services: Mutex<NoopRawMutex, Cell<EnumSet<Service>>>,
    pub start: SpmcSignal<NoopRawMutex, bool>,
    pub bt: SpmcSignal<EspRawMutex, BtState>,
    pub audio: SpmcSignal<EspRawMutex, AudioState>,
    pub audio_track: SharedStateSpmcSignal<EspRawMutex, TrackInfo>,
    pub phone: SpmcSignal<EspRawMutex, AudioState>,
    pub phone_call: SharedStateSpmcSignal<EspRawMutex, PhoneCallInfo>,
    pub button_commands: SpmcSignal<NoopRawMutex, BtCommand>,
    pub radio_commands: SpmcSignal<NoopRawMutex, BtCommand>,
    pub radio: SpmcSignal<NoopRawMutex, RadioState>,
    pub buttons: SpmcSignal<NoopRawMutex, EnumSet<SteeringWheelButton>>,
    pub cockpit_display: SharedStateSpmcSignal<NoopRawMutex, DisplayText>,
    pub radio_display: SharedStateSpmcSignal<NoopRawMutex, DisplayText>,
}

impl State {
    pub const fn new() -> Self {
        Self {
            started_services: Mutex::new(Cell::new(EnumSet::EMPTY)),
            start: SpmcSignal::new(),
            bt: SpmcSignal::new(),
            audio: SpmcSignal::new(),
            audio_track: SharedStateSpmcSignal::new(TrackInfo::new()),
            phone: SpmcSignal::new(),
            phone_call: SharedStateSpmcSignal::new(PhoneCallInfo::new()),
            button_commands: SpmcSignal::new(),
            radio_commands: SpmcSignal::new(),
            radio: SpmcSignal::new(),
            buttons: SpmcSignal::new(),
            cockpit_display: SharedStateSpmcSignal::new(DisplayText::new()),
            radio_display: SharedStateSpmcSignal::new(DisplayText::new()),
        }
    }

    pub fn subscription(&self, service: Service) -> BusSubscription<'_> {
        BusSubscription {
            service: ServiceLifecycle::new(service, &self.start, &self.started_services),
            bt: self.bt.receiver(service),
            audio: self.audio.receiver(service),
            audio_track: self.audio_track.receiver(service),
            phone: self.phone.receiver(service),
            phone_call: self.phone_call.receiver(service),
            button_commands: self.button_commands.receiver(service),
            radio_commands: self.radio_commands.receiver(service),
            radio: self.radio.receiver(service),
            buttons: self.buttons.receiver(service),
            cockpit_display: self.cockpit_display.receiver(service),
            radio_display: self.radio_display.receiver(service),
        }
    }
}

pub struct BusSubscription<'a> {
    pub service: ServiceLifecycle<'a, NoopRawMutex>,
    pub bt: Receiver<'a, EspRawMutex, BtState>,
    pub audio: Receiver<'a, EspRawMutex, AudioState>,
    pub audio_track: SharedStateReceiver<'a, EspRawMutex, TrackInfo>,
    pub phone: Receiver<'a, EspRawMutex, AudioState>,
    pub phone_call: SharedStateReceiver<'a, EspRawMutex, PhoneCallInfo>,
    pub button_commands: Receiver<'a, NoopRawMutex, BtCommand>,
    pub radio_commands: Receiver<'a, NoopRawMutex, BtCommand>,
    pub radio: Receiver<'a, NoopRawMutex, RadioState>,
    pub buttons: Receiver<'a, NoopRawMutex, EnumSet<SteeringWheelButton>>,
    pub cockpit_display: SharedStateReceiver<'a, NoopRawMutex, DisplayText>,
    pub radio_display: SharedStateReceiver<'a, NoopRawMutex, DisplayText>,
}
