use std::cell::RefCell;

use embassy_futures::select::{select3, Either3};
use embassy_sync::{
    blocking_mutex::{raw::RawMutex, Mutex},
    signal::Signal,
};
use esp_idf_svc::sys::EspError;

use crate::{
    can::DisplayText,
    state::{AudioTrackState, PhoneCallInfo, PhoneCallState, RadioState, Receiver, TrackInfo},
};

pub async fn process(
    audio_track: Receiver<'_, impl RawMutex, AudioTrackState>,
    track_info: &Mutex<impl RawMutex, RefCell<TrackInfo>>,
    phone_call: Receiver<'_, impl RawMutex, PhoneCallState>,
    call_info: &Mutex<impl RawMutex, RefCell<PhoneCallInfo>>,
    radio: Receiver<'_, impl RawMutex, RadioState>,
    radio_display: &Mutex<impl RawMutex, RefCell<DisplayText>>,
    radio_display_out: &Signal<impl RawMutex, ()>,
) -> Result<(), EspError> {
    process_radio_display(
        audio_track,
        track_info,
        phone_call,
        call_info,
        radio,
        radio_display,
        radio_display_out,
    )
    .await
}

async fn process_radio_display(
    audio_track: Receiver<'_, impl RawMutex, AudioTrackState>,
    track_info: &Mutex<impl RawMutex, RefCell<TrackInfo>>,
    phone_call: Receiver<'_, impl RawMutex, PhoneCallState>,
    call_info: &Mutex<impl RawMutex, RefCell<PhoneCallInfo>>,
    radio: Receiver<'_, impl RawMutex, RadioState>,
    radio_display: &Mutex<impl RawMutex, RefCell<DisplayText>>,
    radio_display_out: &Signal<impl RawMutex, ()>,
) -> Result<(), EspError> {
    let mut sradio = RadioState::Unknown;
    let mut sphone = PhoneCallState::Idle;
    let mut saudio = AudioTrackState::Uninitialized;

    loop {
        let ret = select3(radio.recv(), phone_call.recv(), audio_track.recv()).await;

        match ret {
            Either3::First(new) => sradio = new,
            Either3::Second(new) => sphone = new,
            Either3::Third(new) => saudio = new,
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
}
