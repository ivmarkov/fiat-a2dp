use embassy_futures::select::{select4, Either4};
use embassy_sync::blocking_mutex::raw::RawMutex;
use enumset::EnumSet;

use crate::{
    can::message::SteeringWheelButton,
    error::Error,
    start::ServiceLifecycle,
    state::{AudioState, AudioTrackState, Command, PhoneCallState, RadioState, Receiver, Sender},
};

pub async fn process(
    service: ServiceLifecycle<'_, impl RawMutex>,
    audio: Receiver<'_, impl RawMutex, AudioState>,
    audio_track: Receiver<'_, impl RawMutex, AudioTrackState>,
    phone: Receiver<'_, impl RawMutex, AudioState>,
    phone_call: Receiver<'_, impl RawMutex, PhoneCallState>,
    radio: Receiver<'_, impl RawMutex, RadioState>,
    buttons: Receiver<'_, impl RawMutex, EnumSet<SteeringWheelButton>>,
    command: Sender<'_, impl RawMutex, Command>,
) -> Result<(), Error> {
    loop {
        service.starting();
        service.started();

        let mut saudio = AudioState::Uninitialized;
        let mut strack = AudioTrackState::Uninitialized;
        let mut sphone = AudioState::Uninitialized;
        let mut scall = PhoneCallState::Idle;
        let mut sradio = RadioState::Unknown;

        loop {
            match select4(
                service.wait_stop(),
                select4(
                    audio.recv(),
                    audio_track.recv(),
                    phone.recv(),
                    phone_call.recv(),
                ),
                radio.recv(),
                buttons.recv(),
            )
            .await
            {
                Either4::First(_) => break,
                Either4::Second(Either4::First(new)) => saudio = new,
                Either4::Second(Either4::Second(new)) => strack = new,
                Either4::Second(Either4::Third(new)) => sphone = new,
                Either4::Second(Either4::Fourth(new)) => scall = new,
                Either4::Third(new) => {
                    if sradio != new {
                        if saudio.is_active() && !sphone.is_active() {
                            match new {
                                RadioState::BtActive => command.send(Command::Resume),
                                _ => command.send(Command::Pause),
                            }
                        }

                        sradio = new;
                    }
                }
                Either4::Fourth(buttons) => {
                    if matches!(scall, PhoneCallState::Ringing) {
                        if buttons.contains(SteeringWheelButton::Menu) {
                            command.send(Command::Answer);
                        } else if buttons.contains(SteeringWheelButton::Windows) {
                            command.send(Command::Reject);
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
                            command.send(Command::Hangup);
                        }
                    } else if sradio.is_bt_active() && !sphone.is_active() {
                        if saudio.is_connected() {
                            if buttons.contains(SteeringWheelButton::Mute) {
                                if matches!(saudio, AudioState::Streaming) {
                                    command.send(Command::Pause);
                                } else if matches!(
                                    saudio,
                                    AudioState::Connected | AudioState::Suspended
                                ) {
                                    command.send(Command::Resume);
                                }
                            } else if buttons.contains(SteeringWheelButton::Up)
                                && strack.is_connected()
                            {
                                command.send(Command::PreviousTrack);
                            } else if buttons.contains(SteeringWheelButton::Down)
                                && strack.is_connected()
                            {
                                command.send(Command::NextTrack);
                            }
                        }
                    }
                }
            }
        }

        service.wait_start().await?;
    }
}
