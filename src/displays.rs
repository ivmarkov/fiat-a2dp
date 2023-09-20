use core::fmt::Write;

use embassy_futures::select::{select4, Either4};
use embassy_sync::blocking_mutex::raw::RawMutex;

use crate::{
    error::Error,
    signal::SharedStateSender,
    state::{
        AudioTrackState, BusSubscription, PhoneCallInfo, PhoneCallState, RadioState, TrackInfo,
    },
};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DisplayText {
    pub version: u32,
    pub text: heapless::String<32>,
}

impl DisplayText {
    pub const fn new() -> Self {
        Self {
            version: 0,
            text: heapless::String::new(),
        }
    }

    // pub fn reset(&mut self) {
    //     self.version += 1;
    //     self.text.clear();
    // }

    pub fn update_phone_info(&mut self, phone: &PhoneCallInfo) {
        self.version += 1;
        self.text.clear();

        let secs = phone.duration.as_secs();

        let mins = secs / 60;
        let secs = secs % 60;

        write!(&mut self.text, "{} {}:{}", phone.phone, mins, secs).unwrap();
    }

    pub fn update_track_info(&mut self, track: &TrackInfo) {
        self.version += 1;
        self.text.clear();

        let secs = track.offset.as_secs();

        let mins = secs / 60;
        let secs = secs % 60;

        write!(
            &mut self.text,
            "{} {} {}:{}",
            track.album, track.artist, mins, secs
        )
        .unwrap();
    }
}

// async fn process_cockpit(
//     audio: Receiver<'_, impl RawMutex, AudioState>,
//     audio_track: Receiver<'_, impl RawMutex, AudioTrackState>,
//     track_info: &Mutex<impl RawMutex, RefCell<TrackInfo>>,
//     phone: Receiver<'_, impl RawMutex, AudioState>,
//     phone_call: Receiver<'_, impl RawMutex, PhoneCallState>,
//     call_info: &Mutex<impl RawMutex, RefCell<PhoneCallInfo>>,
//     radio: Receiver<'_, impl RawMutex, RadioState>,
//     radio_display: &Mutex<impl RawMutex, RefCell<DisplayText>>,
//     radio_display_out: &Signal<impl RawMutex, ()>,
// ) -> Result<(), Error> {
//     todo!()
// }

pub async fn process_radio(
    bus: BusSubscription<'_>,
    radio_display: SharedStateSender<'_, impl RawMutex, DisplayText>,
) -> Result<(), Error> {
    loop {
        bus.service.starting();
        bus.service.started();

        let mut sradio = RadioState::Unknown;
        let mut sphone = PhoneCallState::Idle;
        let mut saudio = AudioTrackState::Uninitialized;

        loop {
            let ret = select4(
                bus.service.wait_stop(),
                bus.radio.recv(),
                bus.phone_call.recv(),
                bus.audio_track.recv(),
            )
            .await;

            match ret {
                Either4::First(_) => break,
                Either4::Second(new) => sradio = new,
                Either4::Third(_) => sphone = bus.phone_call.state(|call| call.state),
                Either4::Fourth(_) => saudio = bus.audio_track.state(|track| track.state),
            }

            if sradio.is_bt_active() {
                if sphone.is_active() {
                    bus.phone_call.state(|call| {
                        radio_display.modify(|display| {
                            display.update_phone_info(&call);
                            true
                        });
                    });
                } else if saudio.is_active() {
                    bus.audio_track.state(|track| {
                        radio_display.modify(|display| {
                            display.update_track_info(&track);
                            true
                        });
                    });
                }
            }
        }

        bus.service.wait_start().await?;
    }
}
