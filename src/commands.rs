use embassy_futures::select::{select4, Either4};
use embassy_sync::blocking_mutex::raw::RawMutex;

use crate::{
    can::message::SteeringWheelButton,
    error::Error,
    signal::Sender,
    state::{
        bt::{AudioState, AudioTrackState, BtCommand, PhoneCallState},
        can::RadioState,
        BusSubscription,
    },
};

pub async fn process(
    bus: BusSubscription<'_>,
    button_commands: Sender<'_, impl RawMutex, BtCommand>,
) -> Result<(), Error> {
    loop {
        bus.service.starting();
        bus.service.started();

        let mut saudio = AudioState::Uninitialized;
        let mut strack = AudioTrackState::Uninitialized;
        let mut sphone = AudioState::Uninitialized;
        let mut scall = PhoneCallState::Idle;
        let mut sradio = RadioState::Unknown;

        loop {
            match select4(
                bus.service.wait_stop(),
                select4(
                    bus.audio.recv(),
                    bus.audio_track.recv(),
                    bus.phone.recv(),
                    bus.phone_call.recv(),
                ),
                bus.radio.recv(),
                bus.buttons.recv(),
            )
            .await
            {
                Either4::First(_) => break,
                Either4::Second(Either4::First(new)) => saudio = new,
                Either4::Second(Either4::Second(_)) => {
                    strack = bus.audio_track.state(|track| track.state)
                }
                Either4::Second(Either4::Third(new)) => sphone = new,
                Either4::Second(Either4::Fourth(_)) => {
                    scall = bus.phone_call.state(|call| call.state)
                }
                Either4::Third(new) => sradio = new,
                Either4::Fourth(buttons) => {
                    if matches!(scall, PhoneCallState::Ringing) {
                        if buttons.contains(SteeringWheelButton::Menu) {
                            button_commands.send(BtCommand::Answer);
                        } else if buttons.contains(SteeringWheelButton::Windows) {
                            button_commands.send(BtCommand::Reject);
                        }
                    } else if matches!(
                        scall,
                        PhoneCallState::CallActive
                            | PhoneCallState::Dialing
                            | PhoneCallState::DialingAlerting
                    ) {
                        if buttons.contains(SteeringWheelButton::Menu)
                            | buttons.contains(SteeringWheelButton::Windows)
                        {
                            button_commands.send(BtCommand::Hangup);
                        }
                    } else if sradio.is_bt_active() && !sphone.is_active() {
                        if saudio.is_connected() {
                            if buttons.contains(SteeringWheelButton::Mute) {
                                if matches!(saudio, AudioState::Streaming) {
                                    button_commands.send(BtCommand::Pause);
                                } else if matches!(
                                    saudio,
                                    AudioState::Connected | AudioState::Suspended
                                ) {
                                    button_commands.send(BtCommand::Resume);
                                }
                            } else if buttons.contains(SteeringWheelButton::Up)
                                && strack.is_connected()
                            {
                                button_commands.send(BtCommand::PreviousTrack);
                            } else if buttons.contains(SteeringWheelButton::Down)
                                && strack.is_connected()
                            {
                                button_commands.send(BtCommand::NextTrack);
                            }
                        }
                    }
                }
            }
        }

        bus.service.wait_start().await?;
    }
}
