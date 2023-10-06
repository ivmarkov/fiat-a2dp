use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use enumset::{EnumSet, EnumSetType};
use esp_idf_svc::hal::task::embassy_sync::EspRawMutex;

use crate::{
    can::message::SteeringWheelButton,
    service::{ServiceLifecycle, System},
    signal::{BroadcastSignal, Receiver, StatefulBroadcastSignal, StatefulReceiver},
};

use self::{
    bt::{AudioState, BtCommand, BtState, PhoneCallInfo, TrackInfo},
    can::{DisplayText, RadioState},
};

pub type DisplayString = heapless::String<32>;

pub mod bt {
    use super::DisplayString;

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
}

pub mod can {
    use core::fmt::Write;

    use super::{
        bt::{PhoneCallInfo, TrackInfo},
        DisplayString,
    };

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

    #[derive(Debug, Clone, Eq, PartialEq)]
    pub struct DisplayText {
        pub version: u32,
        pub menu: bool,
        pub text: DisplayString,
    }

    impl DisplayText {
        pub const fn new() -> Self {
            Self {
                version: 0,
                menu: false,
                text: heapless::String::new(),
            }
        }

        pub fn reset(&mut self) {
            self.version += 1;
            self.menu = false;
            self.text.clear();
        }

        pub fn update_phone_info(&mut self, phone: &PhoneCallInfo) {
            self.version += 1;
            self.text.clear();

            let secs = phone.duration.as_secs();

            let mins = secs / 60;
            let secs = secs % 60;

            write!(&mut self.text, "{} {:02}:{:02}", phone.phone, mins, secs).unwrap();
        }

        pub fn update_track_info(&mut self, track: &TrackInfo) {
            self.version += 1;
            self.text.clear();

            let secs = track.offset.as_secs();

            let mins = secs / 60;
            let secs = secs % 60;

            write!(
                &mut self.text,
                "{};{};{:02}:{:02}",
                track.album, track.artist, mins, secs
            )
            .unwrap();
        }
    }
}

#[derive(Debug, EnumSetType)]
pub enum Service {
    Bt,
    AudioMux,
    Microphone,
    Speakers,
    Can,
    RadioDisplay,
    CockpitDisplay,
    Commands,
    Wifi,
}

pub struct Bus {
    pub system: StatefulBroadcastSignal<NoopRawMutex, System>,
    pub bt: BroadcastSignal<EspRawMutex, BtState>,
    pub audio: BroadcastSignal<EspRawMutex, AudioState>,
    pub audio_track: StatefulBroadcastSignal<EspRawMutex, TrackInfo>,
    pub phone: BroadcastSignal<EspRawMutex, AudioState>,
    pub phone_call: StatefulBroadcastSignal<EspRawMutex, PhoneCallInfo>,
    pub button_commands: BroadcastSignal<NoopRawMutex, BtCommand>,
    pub radio_commands: BroadcastSignal<NoopRawMutex, BtCommand>,
    pub radio: BroadcastSignal<NoopRawMutex, RadioState>,
    pub buttons: BroadcastSignal<NoopRawMutex, EnumSet<SteeringWheelButton>>,
    pub cockpit_display: StatefulBroadcastSignal<NoopRawMutex, DisplayText>,
    pub radio_display: StatefulBroadcastSignal<NoopRawMutex, DisplayText>,
    pub update: BroadcastSignal<NoopRawMutex, ()>,
}

impl Bus {
    pub const fn new() -> Self {
        Self {
            system: StatefulBroadcastSignal::new(System::new()),
            bt: BroadcastSignal::new(),
            audio: BroadcastSignal::new(),
            audio_track: StatefulBroadcastSignal::new(TrackInfo::new()),
            phone: BroadcastSignal::new(),
            phone_call: StatefulBroadcastSignal::new(PhoneCallInfo::new()),
            button_commands: BroadcastSignal::new(),
            radio_commands: BroadcastSignal::new(),
            radio: BroadcastSignal::new(),
            buttons: BroadcastSignal::new(),
            cockpit_display: StatefulBroadcastSignal::new(DisplayText::new()),
            radio_display: StatefulBroadcastSignal::new(DisplayText::new()),
            update: BroadcastSignal::new(),
        }
    }

    pub fn subscription(&self, service: Service) -> BusSubscription<'_> {
        BusSubscription {
            service: ServiceLifecycle::new(service, &self.system),
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
            update: self.update.receiver(service),
        }
    }
}

pub struct BusSubscription<'a> {
    pub service: ServiceLifecycle<'a, NoopRawMutex>,
    pub bt: Receiver<'a, EspRawMutex, BtState>,
    pub audio: Receiver<'a, EspRawMutex, AudioState>,
    pub audio_track: StatefulReceiver<'a, EspRawMutex, TrackInfo>,
    pub phone: Receiver<'a, EspRawMutex, AudioState>,
    pub phone_call: StatefulReceiver<'a, EspRawMutex, PhoneCallInfo>,
    pub button_commands: Receiver<'a, NoopRawMutex, BtCommand>,
    pub radio_commands: Receiver<'a, NoopRawMutex, BtCommand>,
    pub radio: Receiver<'a, NoopRawMutex, RadioState>,
    pub buttons: Receiver<'a, NoopRawMutex, EnumSet<SteeringWheelButton>>,
    pub cockpit_display: StatefulReceiver<'a, NoopRawMutex, DisplayText>,
    pub radio_display: StatefulReceiver<'a, NoopRawMutex, DisplayText>,
    pub update: Receiver<'a, NoopRawMutex, ()>,
}
