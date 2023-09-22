use std::cell::{Cell, RefCell};

use embassy_futures::select::{select, select4, Either, Either4};
use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_time::{Duration, Timer};
use enumset::EnumSet;

use crate::{
    bus::{
        bt::{AudioState, AudioTrackState, BtCommand, PhoneCallInfo, PhoneCallState, TrackInfo},
        can::RadioState,
        BusSubscription,
    },
    can::message::SteeringWheelButton,
    error::Error,
    flash_mode::FlashMode,
    signal::{Receiver, Sender, StatefulReceiver},
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
    mut flash_mode: FlashMode<'_>,
    button_commands: Sender<'_, impl RawMutex, BtCommand>,
) -> Result<(), Error> {
    let flash_mode_period = Cell::new(true);

    loop {
        bus.service.starting();
        bus.service.started();

        let status = RefCell::new(Status::new());

        loop {
            match select4(
                bus.service.wait_disabled(),
                process_flash_mode_period(&flash_mode_period),
                process_buttons(
                    &bus.buttons,
                    &status,
                    &mut flash_mode,
                    &flash_mode_period,
                    &button_commands,
                ),
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
                Either4::First(_) => break,
                _ => unreachable!(),
            }
        }

        bus.service.wait_enabled().await?;
    }
}

async fn process_flash_mode_period(flash_mode_period: &Cell<bool>) -> Result<(), Error> {
    Timer::after(Duration::from_secs(3)).await;

    flash_mode_period.set(false);

    core::future::pending().await
}

async fn process_buttons(
    buttons: &Receiver<'_, impl RawMutex, EnumSet<SteeringWheelButton>>,
    status: &RefCell<Status>,
    flash_mode: &mut FlashMode<'_>,
    flash_mode_period: &Cell<bool>,
    button_commands: &Sender<'_, impl RawMutex, BtCommand>,
) -> Result<(), Error> {
    let mut sbuttons = EnumSet::EMPTY;
    let mut conf = false;
    let mut menu = false;

    loop {
        let buttons = buttons.recv().await;
        let just_pressed = sbuttons.intersection(buttons);

        sbuttons = buttons;

        let status = status.borrow();

        if status.phone.is_active() {
            conf = false;
        } else {
            if just_pressed.contains(SteeringWheelButton::Windows) {
                if flash_mode_period.get() && sbuttons.contains(SteeringWheelButton::Mute) {
                    flash_mode.enter().await?;
                } else {
                    conf = !conf;
                }
            }
        }

        if conf {
            handle_conf(just_pressed, &status, button_commands);
        } else {
            handle_run(just_pressed, &mut menu, &status, button_commands);
        }
    }
}

fn handle_conf(
    just_pressed: EnumSet<SteeringWheelButton>,
    status: &Status,
    button_commands: &Sender<'_, impl RawMutex, BtCommand>,
) {
    // TODO
}

fn handle_run(
    just_pressed: EnumSet<SteeringWheelButton>,
    menu: &mut bool,
    status: &Status,
    button_commands: &Sender<'_, impl RawMutex, BtCommand>,
) {
    if status.phone.is_active() {
        *menu = false;
    }

    if *menu {
        handle_phone_menu(just_pressed, menu, status, button_commands);
    } else {
        handle_shortcuts(just_pressed, menu, status, button_commands);
    }
}

fn handle_phone_menu(
    just_pressed: EnumSet<SteeringWheelButton>,
    menu: &mut bool,
    status: &Status,
    button_commands: &Sender<'_, impl RawMutex, BtCommand>,
) {
    // TODO
    if just_pressed.contains(SteeringWheelButton::Up) {
        *menu = false;
    }
}

fn handle_shortcuts(
    just_pressed: EnumSet<SteeringWheelButton>,
    menu: &mut bool,
    status: &Status,
    button_commands: &Sender<'_, impl RawMutex, BtCommand>,
) {
    match status.call {
        PhoneCallState::Dialing | PhoneCallState::DialingAlerting | PhoneCallState::CallActive => {
            if just_pressed.contains(SteeringWheelButton::Menu) {
                button_commands.send(BtCommand::Hangup);
            }
        }
        PhoneCallState::Ringing => {
            if just_pressed.contains(SteeringWheelButton::Menu) {
                button_commands.send(BtCommand::Answer);
            } else if just_pressed.contains(SteeringWheelButton::Down) {
                button_commands.send(BtCommand::Reject);
            }
        }
        PhoneCallState::Idle => {
            if just_pressed.contains(SteeringWheelButton::Menu) {
                *menu = true;
            } else if status.radio.is_bt_active() {
                if status.audio.is_connected() {
                    if just_pressed.contains(SteeringWheelButton::Mute) {
                        if matches!(status.audio, AudioState::Streaming) {
                            button_commands.send(BtCommand::Pause);
                        } else if matches!(
                            status.audio,
                            AudioState::Connected | AudioState::Suspended
                        ) {
                            button_commands.send(BtCommand::Resume);
                        }
                    } else if just_pressed.contains(SteeringWheelButton::Up)
                        && status.track.is_connected()
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
    }
}

async fn process_status(
    audio: &Receiver<'_, impl RawMutex, AudioState>,
    audio_track: &StatefulReceiver<'_, impl RawMutex, TrackInfo>,
    phone: &Receiver<'_, impl RawMutex, AudioState>,
    phone_call: &StatefulReceiver<'_, impl RawMutex, PhoneCallInfo>,
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
