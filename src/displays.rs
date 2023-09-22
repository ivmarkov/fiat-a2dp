use embassy_futures::select::{select4, Either4};
use embassy_sync::blocking_mutex::raw::RawMutex;

use crate::{
    bus::{
        bt::{AudioTrackState, PhoneCallState},
        can::{DisplayText, RadioState},
        BusSubscription,
    },
    error::Error,
    signal::StatefulSender,
};

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
    radio_display: StatefulSender<'_, impl RawMutex, DisplayText>,
) -> Result<(), Error> {
    loop {
        bus.service.starting();
        bus.service.started();

        let mut sradio = RadioState::Unknown;
        let mut sphone = PhoneCallState::Idle;
        let mut saudio = AudioTrackState::Uninitialized;

        loop {
            let ret = select4(
                bus.service.wait_disabled(),
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

        bus.service.wait_enabled().await?;
    }
}
