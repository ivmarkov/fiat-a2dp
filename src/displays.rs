use core::{cell::RefCell, fmt::Write};

use embassy_futures::select::{select4, Either4};
use embassy_sync::{
    blocking_mutex::{raw::RawMutex, Mutex},
    signal::Signal,
};

use crate::{
    error::Error,
    start::ServiceLifecycle,
    state::{AudioTrackState, PhoneCallInfo, PhoneCallState, RadioState, Receiver, TrackInfo},
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
    service: ServiceLifecycle<'_, impl RawMutex>,
    audio_track: Receiver<'_, impl RawMutex, AudioTrackState>,
    track_info: &Mutex<impl RawMutex, RefCell<TrackInfo>>,
    phone_call: Receiver<'_, impl RawMutex, PhoneCallState>,
    call_info: &Mutex<impl RawMutex, RefCell<PhoneCallInfo>>,
    radio: Receiver<'_, impl RawMutex, RadioState>,
    radio_display: &Mutex<impl RawMutex, RefCell<DisplayText>>,
    radio_display_out: &Signal<impl RawMutex, ()>,
) -> Result<(), Error> {
    loop {
        service.starting();
        service.started();

        let mut sradio = RadioState::Unknown;
        let mut sphone = PhoneCallState::Idle;
        let mut saudio = AudioTrackState::Uninitialized;

        loop {
            let ret = select4(
                service.wait_stop(),
                radio.recv(),
                phone_call.recv(),
                audio_track.recv(),
            )
            .await;

            match ret {
                Either4::First(_) => break,
                Either4::Second(new) => sradio = new,
                Either4::Third(new) => sphone = new,
                Either4::Fourth(new) => saudio = new,
            }

            if sradio.is_bt_active() {
                if sphone.is_active() {
                    call_info.lock(|ci| {
                        radio_display
                            .lock(|display| display.borrow_mut().update_phone_info(&ci.borrow()));
                        radio_display_out.signal(());
                    });
                } else if saudio.is_active() {
                    track_info.lock(|ti| {
                        radio_display
                            .lock(|display| display.borrow_mut().update_track_info(&ti.borrow()));
                        radio_display_out.signal(());
                    });
                }
            }
        }

        service.wait_start().await?;
    }
}
