use std::cell::RefCell;

use embassy_futures::select::{select, select3, select4, Either, Either3, Either4};
use embassy_sync::blocking_mutex::raw::RawMutex;
use enumset::EnumSet;

use crate::{
    can::message::SteeringWheelButton,
    error::Error,
    signal::{Receiver, Sender, SharedStateReceiver},
    state::{
        bt::{AudioState, AudioTrackState, BtCommand, PhoneCallInfo, PhoneCallState, TrackInfo},
        can::RadioState,
        BusSubscription,
    },
};

struct Status {
    audio: AudioState,
    track: AudioTrackState,
    phone: AudioState,
    call: PhoneCallState,
    radio: RadioState,
}

impl Status {
    pub const fn new() -> Self {
        Self {
            audio: AudioState::Uninitialized,
            track: AudioTrackState::Uninitialized,
            phone: AudioState::Uninitialized,
            call: PhoneCallState::Idle,
            radio: RadioState::Unknown,
        }
    }
}

pub async fn process(
    bus: BusSubscription<'_>,
    button_commands: Sender<'_, impl RawMutex, BtCommand>,
) -> Result<(), Error> {
    loop {
        bus.service.starting();
        bus.service.started();

        let status = RefCell::new(Status::new());

        loop {
            match select3(
                bus.service.wait_stop(),
                process_buttons(&bus.buttons, &status, &button_commands),
                process_status(
                    &bus.audio,
                    &bus.audio_track,
                    &bus.phone,
                    &bus.phone_call,
                    &bus.radio,
                    &status,
                ),
            )
            .await
            {
                Either3::First(_) => break,
                _ => unreachable!(),
            }
        }

        bus.service.wait_start().await?;
    }
}

async fn process_buttons(
    buttons: &Receiver<'_, impl RawMutex, EnumSet<SteeringWheelButton>>,
    status: &RefCell<Status>,
    button_commands: &Sender<'_, impl RawMutex, BtCommand>,
) -> Result<(), Error> {
    let mut sbuttons = EnumSet::EMPTY;
    let mut conf = false;

    loop {
        let buttons = buttons.recv().await;
        let just_pressed = sbuttons.intersection(buttons);

        sbuttons = buttons;

        if just_pressed.contains(SteeringWheelButton::Menu) {
            conf = !conf;
        } else {
            if conf {
                handle_bt_conf(just_pressed, &status.borrow(), button_commands);
            } else {
                handle_bt_runtime(just_pressed, &status.borrow(), button_commands);
            }
        }
    }
}

fn handle_bt_conf(
    just_pressed: EnumSet<SteeringWheelButton>,
    status: &Status,
    button_commands: &Sender<'_, impl RawMutex, BtCommand>,
) {
    // TODO
}

fn handle_bt_runtime(
    just_pressed: EnumSet<SteeringWheelButton>,
    status: &Status,
    button_commands: &Sender<'_, impl RawMutex, BtCommand>,
) {
    if matches!(status.call, PhoneCallState::Ringing) {
        if just_pressed.contains(SteeringWheelButton::Menu) {
            button_commands.send(BtCommand::Answer);
        } else if just_pressed.contains(SteeringWheelButton::Windows) {
            button_commands.send(BtCommand::Reject);
        }
    } else if matches!(
        status.call,
        PhoneCallState::CallActive | PhoneCallState::Dialing | PhoneCallState::DialingAlerting
    ) {
        if just_pressed.contains(SteeringWheelButton::Windows) {
            button_commands.send(BtCommand::Hangup);
        }
    } else if status.radio.is_bt_active() && !status.phone.is_active() {
        if status.audio.is_connected() {
            if just_pressed.contains(SteeringWheelButton::Mute) {
                if matches!(status.audio, AudioState::Streaming) {
                    button_commands.send(BtCommand::Pause);
                } else if matches!(status.audio, AudioState::Connected | AudioState::Suspended) {
                    button_commands.send(BtCommand::Resume);
                }
            } else if just_pressed.contains(SteeringWheelButton::Up) && status.track.is_connected()
            {
                button_commands.send(BtCommand::PreviousTrack);
            } else if just_pressed.contains(SteeringWheelButton::Down)
                && status.track.is_connected()
            {
                button_commands.send(BtCommand::NextTrack);
            }
        }
    }
}

async fn process_status(
    audio: &Receiver<'_, impl RawMutex, AudioState>,
    audio_track: &SharedStateReceiver<'_, impl RawMutex, TrackInfo>,
    phone: &Receiver<'_, impl RawMutex, AudioState>,
    phone_call: &SharedStateReceiver<'_, impl RawMutex, PhoneCallInfo>,
    radio: &Receiver<'_, impl RawMutex, RadioState>,
    status: &RefCell<Status>,
) -> Result<(), Error> {
    loop {
        match select(
            radio.recv(),
            select4(
                audio.recv(),
                audio_track.recv(),
                phone.recv(),
                phone_call.recv(),
            ),
        )
        .await
        {
            Either::First(new) => status.borrow_mut().radio = new,
            Either::Second(Either4::First(new)) => status.borrow_mut().audio = new,
            Either::Second(Either4::Second(_)) => {
                status.borrow_mut().track = audio_track.state(|track| track.state)
            }
            Either::Second(Either4::Third(new)) => status.borrow_mut().phone = new,
            Either::Second(Either4::Fourth(_)) => {
                status.borrow_mut().call = phone_call.state(|call| call.state)
            }
        }
    }
}
