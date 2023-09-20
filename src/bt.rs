use core::cell::RefCell;

use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::blocking_mutex::Mutex;

use esp_idf_svc::bt::a2dp::{AudioStatus, ConnectionStatus};
use esp_idf_svc::bt::avrc::{KeyCode, Notification, PlaybackStatus};
use esp_idf_svc::bt::hfp::client::{self, CallSetupStatus};
use esp_idf_svc::{
    bt::{
        a2dp::{A2dpEvent, EspA2dp, SinkEnabled},
        avrc::controller::{AvrccEvent, EspAvrcc},
        avrc::{MetadataId, NotificationType},
        gap::{
            Cod, CodMajorDeviceType, CodMode, CodServiceClass, DiscoveryMode, EspGap, GapEvent,
            IOCapabilities,
        },
        hfp::client::{EspHfpc, HfpcEvent},
        BtClassic, BtClassicEnabled, BtDriver,
    },
    nvs::EspDefaultNvsPartition,
};

use esp_idf_svc::hal::{modem::BluetoothModemPeripheral, peripheral::Peripheral};

use log::*;

use crate::audio::AUDIO_BUFFERS;

use crate::error::Error;
use crate::select_spawn::SelectSpawn;
use crate::start::ServiceLifecycle;
use crate::state::{
    AudioState, AudioTrackState, BtState, Command, PhoneCallInfo, PhoneCallState, Receiver, Sender,
    TrackInfo,
};

pub async fn process(
    service: ServiceLifecycle<'_, impl RawMutex>,
    mut modem: impl Peripheral<P = impl BluetoothModemPeripheral>,
    nvs: EspDefaultNvsPartition,
    bt: Sender<'_, impl RawMutex + Sync, BtState>,
    audio_state: Sender<'_, impl RawMutex + Sync, AudioState>,
    audio_track_state: Sender<'_, impl RawMutex + Sync, AudioTrackState>,
    track_info: &Mutex<impl RawMutex + Sync, RefCell<TrackInfo>>,
    phone_state: Sender<'_, impl RawMutex + Sync, AudioState>,
    phone_call_state: Sender<'_, impl RawMutex + Sync, PhoneCallState>,
    phone_call_info: &Mutex<impl RawMutex + Sync, RefCell<PhoneCallInfo>>,
    commands: Receiver<'_, impl RawMutex, Command>,
) -> Result<(), Error> {
    loop {
        {
            service.starting();

            let driver = BtDriver::<BtClassic>::new(&mut modem, Some(nvs.clone()))?;

            driver.set_device_name("Fiat")?;

            info!("Bluetooth initialized");

            let gap = EspGap::new(&driver)?;

            info!("GAP created");

            let avrcc = EspAvrcc::new(&driver)?;

            info!("AVRCC created");

            let a2dp = EspA2dp::new_sink(&driver)?;

            info!("A2DP created");

            let hfpc = EspHfpc::new(&driver, None)?;

            info!("HFPC created");

            gap.initialize(|event| handle_gap(&gap, &bt, event))?;

            gap.set_cod(
                Cod::new(
                    CodMajorDeviceType::AudioVideo,
                    0,
                    CodServiceClass::Audio | CodServiceClass::Telephony,
                ),
                CodMode::Init,
            )?;

            gap.set_ssp_io_cap(IOCapabilities::None)?;
            gap.set_pin("1234")?;
            gap.set_scan_mode(true, DiscoveryMode::Discoverable)?;

            info!("GAP initialized");

            audio_track_state.send(AudioTrackState::Initialized);
            avrcc
                .initialize(|event| handle_avrcc(&avrcc, &audio_track_state, track_info, event))?;

            info!("AVRCC initialized");

            a2dp.initialize(|event| handle_a2dp(&a2dp, &audio_state, event))?;

            info!("A2DP initialized");

            hfpc.initialize(|event| {
                handle_hfpc(
                    &hfpc,
                    &phone_state,
                    &phone_call_state,
                    phone_call_info,
                    event,
                )
            })?;

            info!("HFPC initialized");

            a2dp.set_delay(core::time::Duration::from_millis(150))?;

            service.started();

            SelectSpawn::run(service.wait_stop())
                .chain(process_commands(&commands, &a2dp, &avrcc, &hfpc))
                .await?;
        }

        service.wait_start().await?;
    }
}

async fn process_commands<'d, M>(
    commands: &Receiver<'_, impl RawMutex, Command>,
    _a2dp: &EspA2dp<'d, M, &BtDriver<'d, M>, impl SinkEnabled>,
    avrcc: &EspAvrcc<'d, M, &BtDriver<'d, M>>,
    hfpc: &EspHfpc<'d, M, &BtDriver<'d, M>>,
) -> Result<(), Error>
where
    M: BtClassicEnabled,
{
    loop {
        match commands.recv().await {
            Command::Answer => hfpc.answer()?,
            Command::Reject => hfpc.reject()?,
            Command::Hangup => hfpc.reject()?,
            Command::Pause => avrcc.send_passthrough(0, KeyCode::Pause, true)?,
            Command::Resume => avrcc.send_passthrough(0, KeyCode::Play, true)?,
            Command::NextTrack => avrcc.send_passthrough(0, KeyCode::ChannelUp, true)?,
            Command::PreviousTrack => avrcc.send_passthrough(0, KeyCode::ChannelDown, true)?,
        }
    }
}

fn handle_gap<'d, M>(
    gap: &EspGap<'d, M, &BtDriver<'d, M>>,
    _bt: &Sender<'_, impl RawMutex, BtState>,
    event: GapEvent<'_>,
) where
    M: BtClassicEnabled,
{
    match event {
        GapEvent::DeviceDiscovered { bd_addr, props } => {
            info!("Found device: {:?}", bd_addr);

            for prop in props {
                info!("Prop: {:?}", prop.prop());
            }

            //let _ = gap.stop_discovery();
        }
        GapEvent::PairingUserConfirmationRequest { bd_addr, .. } => {
            gap.reply_ssp_confirm(&bd_addr, true).unwrap();
        }
        _ => (),
    }
}

fn handle_a2dp<'d, M>(
    _a2dp: &EspA2dp<'d, M, &BtDriver<'d, M>, impl SinkEnabled>,
    audio_state: &Sender<'_, impl RawMutex, AudioState>,
    event: A2dpEvent<'_>,
) where
    M: BtClassicEnabled,
{
    match event {
        A2dpEvent::Initialized => audio_state.send(AudioState::Initialized),
        A2dpEvent::Deinitialized => audio_state.send(AudioState::Uninitialized),
        A2dpEvent::ConnectionState { status, .. } => match status {
            ConnectionStatus::Connected => audio_state.send(AudioState::Connected),
            ConnectionStatus::Disconnected => audio_state.send(AudioState::Initialized),
            _ => (),
        },
        A2dpEvent::AudioState { status, .. } => match status {
            AudioStatus::Started => audio_state.send(AudioState::Streaming),
            AudioStatus::SuspendedByRemote => audio_state.send(AudioState::Suspended),
            AudioStatus::Stopped => audio_state.send(AudioState::Connected),
        },
        A2dpEvent::SinkData(data) => {
            AUDIO_BUFFERS.lock(|buffers| {
                buffers.borrow_mut().push_incoming(data, true, || {});
            });
        }
        _ => (),
    }
}

fn handle_avrcc<'d, M>(
    avrcc: &EspAvrcc<'d, M, &BtDriver<'d, M>>,
    audio_track_state: &Sender<'_, impl RawMutex, AudioTrackState>,
    track_info: &Mutex<impl RawMutex, RefCell<TrackInfo>>,
    event: AvrccEvent<'_>,
) where
    M: BtClassicEnabled,
{
    match &event {
        AvrccEvent::Connected(_) => {
            audio_track_state.send(AudioTrackState::Connected);
            avrcc.request_capabilities(0).unwrap();
        }
        AvrccEvent::Disconnected(_) => audio_track_state.send(AudioTrackState::Initialized),
        AvrccEvent::NotificationCapabilities { .. } => {
            request_info(avrcc);
        }
        AvrccEvent::Notification(notification) => {
            request_info(avrcc); // TODO: Necessary?

            match notification {
                Notification::Playback(status) => match status {
                    PlaybackStatus::Stopped => {
                        update_track_info(audio_track_state, track_info, |ti| {
                            ti.reset();
                        })
                    }
                    PlaybackStatus::Playing
                    | PlaybackStatus::SeekForward
                    | PlaybackStatus::SeekBackward
                    | PlaybackStatus::Paused => {
                        update_track_info(audio_track_state, track_info, |ti| {
                            ti.paused = matches!(status, PlaybackStatus::Paused);
                        })
                    }
                    _ => (),
                },
                Notification::TrackChanged
                | Notification::TrackStarted
                | Notification::TrackEnded => {
                    update_track_info(audio_track_state, track_info, |ti| {
                        ti.reset();
                    })
                }
                Notification::PlaybackPosition(position) => {
                    update_track_info(audio_track_state, track_info, |ti| {
                        ti.offset = core::time::Duration::from_secs(*position as _);
                    })
                }
                _ => (),
            }
        }
        AvrccEvent::Metadata { id, text } => match id {
            MetadataId::Title => update_track_info(audio_track_state, track_info, |ti| {
                ti.song = (*text).into();
            }),
            MetadataId::Artist => update_track_info(audio_track_state, track_info, |ti| {
                ti.artist = (*text).into();
            }),
            MetadataId::Album => update_track_info(audio_track_state, track_info, |ti| {
                ti.album = (*text).into();
            }),
            MetadataId::PlayingTime => update_track_info(audio_track_state, track_info, |ti| {
                ti.duration = core::time::Duration::from_secs(0); // TODO (*text).into();
            }),
            _ => (),
        },
        _ => (),
    }
}

fn handle_hfpc<'d, M>(
    hfpc: &EspHfpc<'d, M, &BtDriver<'d, M>>,
    audio_state: &Sender<'_, impl RawMutex, AudioState>,
    phone_call_state: &Sender<'_, impl RawMutex, PhoneCallState>,
    phone_call_info: &Mutex<impl RawMutex, RefCell<PhoneCallInfo>>,
    event: HfpcEvent<'_>,
) -> usize
where
    M: BtClassicEnabled,
{
    match event {
        HfpcEvent::ConnectionState { status, .. } => {
            match status {
                client::ConnectionStatus::Connected | client::ConnectionStatus::SlcConnected => {
                    audio_state.send(AudioState::Connected)
                }
                client::ConnectionStatus::Disconnected => audio_state.send(AudioState::Initialized),
                _ => (),
            }

            0
        }
        HfpcEvent::AudioState { status, .. } => {
            match status {
                client::AudioStatus::Connected | client::AudioStatus::ConnectedMsbc => {
                    audio_state.send(AudioState::Streaming)
                }
                client::AudioStatus::Disconnected => audio_state.send(AudioState::Suspended),
                _ => (),
            }

            0
        }
        HfpcEvent::CallSetupState(state) if state != CallSetupStatus::Idle => {
            hfpc.request_current_calls().unwrap();

            update_call_info(phone_call_state, phone_call_info, move |_ci| match state {
                CallSetupStatus::Idle => unreachable!(),
                CallSetupStatus::Incoming => PhoneCallState::Ringing,
                CallSetupStatus::OutgoingDialing => PhoneCallState::Dialing,
                CallSetupStatus::OutgoingAlerting => PhoneCallState::DialingAlerting,
            });

            0
        }
        HfpcEvent::CallState(active) => {
            if active {
                hfpc.request_current_calls().unwrap();
            }

            update_call_info(phone_call_state, phone_call_info, |ci| {
                if active {
                    PhoneCallState::CallActive
                } else {
                    ci.reset();
                    PhoneCallState::Idle
                }
            });

            0
        }
        // HfpcEvent::CurrentCall { outgoing, status, number, .. } => {
        //     match status {
        //         CurrentCallStatus::Active => todo!(),
        //         CurrentCallStatus::Held => todo!(),
        //         CurrentCallStatus::Dialing => todo!(),
        //         CurrentCallStatus::Alerting => todo!(),
        //         CurrentCallStatus::Incoming => todo!(),
        //         CurrentCallStatus::Waiting => todo!(),
        //         CurrentCallStatus::HeldByResponseAndHold => todo!(),
        //     }

        //     0
        // }
        HfpcEvent::RecvData(data) => {
            AUDIO_BUFFERS.lock(|buffers| {
                buffers.borrow_mut().push_incoming(data, false, || {
                    hfpc.request_outgoing_data_ready();
                })
            });

            0
        }
        HfpcEvent::SendData(data) => {
            AUDIO_BUFFERS.lock(|buffers| buffers.borrow_mut().pop_outgoing(data, false))
        }
        _ => 0,
    }
}

fn update_track_info(
    audio_track_state: &Sender<'_, impl RawMutex, AudioTrackState>,
    track_info: &Mutex<impl RawMutex, RefCell<TrackInfo>>,
    mut f: impl FnMut(&mut TrackInfo),
) {
    track_info.lock(|ti| {
        let mut ti = ti.borrow_mut();

        f(&mut ti);

        ti.version += 1;

        audio_track_state.send(if ti.paused {
            AudioTrackState::Paused
        } else {
            AudioTrackState::Playing
        })
    });
}

fn request_info<'d, M>(avrcc: &EspAvrcc<'d, M, &BtDriver<'d, M>>)
where
    M: BtClassicEnabled,
{
    // TODO: Do it based on available capabilities

    avrcc
        .register_notification(1, NotificationType::PlaybackPosition, 1000)
        .unwrap();
    avrcc
        .register_notification(2, NotificationType::Playback, 0)
        .unwrap();
    avrcc
        .register_notification(3, NotificationType::TrackChanged, 0)
        .unwrap();
    avrcc
        .request_metadata(
            4,
            MetadataId::Title | MetadataId::Artist | MetadataId::Album | MetadataId::PlayingTime,
        )
        .unwrap();
}

fn update_call_info(
    phone_call_state: &Sender<'_, impl RawMutex, PhoneCallState>,
    call_info: &Mutex<impl RawMutex, RefCell<PhoneCallInfo>>,
    mut f: impl FnMut(&mut PhoneCallInfo) -> PhoneCallState,
) {
    call_info.lock(|ci| {
        let mut ci = ci.borrow_mut();

        ci.version += 1;

        let state = f(&mut ci);

        phone_call_state.send(state)
    });
}
