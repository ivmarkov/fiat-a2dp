use core::cell::RefCell;

use embassy_sync::blocking_mutex::raw::RawMutex;

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

use crate::audio::SharedAudioBuffers;
use crate::bus::{
    bt::{
        AudioState, AudioTrackState, BtCommand, BtState, PhoneCallInfo, PhoneCallState, TrackInfo,
    },
    BusSubscription,
};
use crate::error::Error;
use crate::select_spawn::SelectSpawn;
use crate::signal::{Receiver, Sender, StatefulSender};

pub async fn process(
    modem: &RefCell<impl Peripheral<P = impl BluetoothModemPeripheral>>,
    nvs: EspDefaultNvsPartition,
    bus: BusSubscription<'_>,
    bt: Sender<'_, impl RawMutex + Sync, BtState>,
    audio: Sender<'_, impl RawMutex + Sync, AudioState>,
    audio_track: StatefulSender<'_, impl RawMutex + Sync, TrackInfo>,
    phone: Sender<'_, impl RawMutex + Sync, AudioState>,
    phone_call: StatefulSender<'_, impl RawMutex + Sync, PhoneCallInfo>,
    audio_buffers: &SharedAudioBuffers<'_>,
) -> Result<(), Error> {
    loop {
        bus.service.wait_enabled().await?;

        {
            bus.service.starting();

            let mut modem = modem.borrow_mut();

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

            audio_track.modify(|track| {
                track.state = AudioTrackState::Initialized;
                track.version += 1;
                true
            });

            avrcc.initialize(|event| handle_avrcc(&avrcc, &audio_track, event))?;

            info!("AVRCC initialized");

            a2dp.initialize(|event| handle_a2dp(&a2dp, &audio, audio_buffers, event))?;

            info!("A2DP initialized");

            hfpc.initialize(|event| handle_hfpc(&hfpc, &phone, &phone_call, audio_buffers, event))?;

            info!("HFPC initialized");

            a2dp.set_delay(core::time::Duration::from_millis(150))?;

            bus.service.started();

            SelectSpawn::run(bus.service.wait_disabled())
                .chain(process_commands(&bus.radio_commands, &a2dp, &avrcc, &hfpc))
                .chain(process_commands(&bus.button_commands, &a2dp, &avrcc, &hfpc))
                .await?;
        }
    }
}

async fn process_commands<'d, M>(
    commands: &Receiver<'_, impl RawMutex, BtCommand>,
    _a2dp: &EspA2dp<'d, M, &BtDriver<'d, M>, impl SinkEnabled>,
    avrcc: &EspAvrcc<'d, M, &BtDriver<'d, M>>,
    hfpc: &EspHfpc<'d, M, &BtDriver<'d, M>>,
) -> Result<(), Error>
where
    M: BtClassicEnabled,
{
    loop {
        match commands.recv().await {
            BtCommand::Answer => hfpc.answer()?,
            BtCommand::Reject => hfpc.reject()?,
            BtCommand::Hangup => hfpc.reject()?,
            BtCommand::Pause => avrcc.send_passthrough(0, KeyCode::Pause, true)?,
            BtCommand::Resume => avrcc.send_passthrough(0, KeyCode::Play, true)?,
            BtCommand::NextTrack => avrcc.send_passthrough(0, KeyCode::ChannelUp, true)?,
            BtCommand::PreviousTrack => avrcc.send_passthrough(0, KeyCode::ChannelDown, true)?,
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
    audio: &Sender<'_, impl RawMutex, AudioState>,
    audio_buffers: &SharedAudioBuffers<'_>,
    event: A2dpEvent<'_>,
) where
    M: BtClassicEnabled,
{
    match event {
        A2dpEvent::Initialized => audio.send(AudioState::Initialized),
        A2dpEvent::Deinitialized => audio.send(AudioState::Uninitialized),
        A2dpEvent::ConnectionState { status, .. } => match status {
            ConnectionStatus::Connected => audio.send(AudioState::Connected),
            ConnectionStatus::Disconnected => audio.send(AudioState::Initialized),
            _ => (),
        },
        A2dpEvent::AudioState { status, .. } => match status {
            AudioStatus::Started => audio.send(AudioState::Streaming),
            AudioStatus::SuspendedByRemote => audio.send(AudioState::Suspended),
            AudioStatus::Stopped => audio.send(AudioState::Connected),
        },
        A2dpEvent::SinkData(data) => {
            audio_buffers.lock(|buffers| {
                buffers.borrow_mut().push_incoming(data, true, || {});
            });
        }
        _ => (),
    }
}

fn handle_avrcc<'d, M>(
    avrcc: &EspAvrcc<'d, M, &BtDriver<'d, M>>,
    audio_track: &StatefulSender<'_, impl RawMutex, TrackInfo>,
    event: AvrccEvent<'_>,
) where
    M: BtClassicEnabled,
{
    match &event {
        AvrccEvent::Connected(_) => {
            audio_track.modify(|track| {
                track.state = AudioTrackState::Connected;
                track.version += 1;
                true
            });
            avrcc.request_capabilities(0).unwrap();
        }
        AvrccEvent::Disconnected(_) => audio_track.modify(|track| {
            track.state = AudioTrackState::Initialized;
            track.version += 1;
            true
        }),
        AvrccEvent::NotificationCapabilities { .. } => {
            request_info(avrcc);
        }
        AvrccEvent::Notification(notification) => {
            request_info(avrcc); // TODO: Necessary?

            match notification {
                Notification::Playback(status) => match status {
                    PlaybackStatus::Stopped => {
                        audio_track.modify(|track| {
                            track.reset();
                            track.version += 1;
                            true
                        });
                    }
                    PlaybackStatus::Playing
                    | PlaybackStatus::SeekForward
                    | PlaybackStatus::SeekBackward
                    | PlaybackStatus::Paused => {
                        audio_track.modify(|track| {
                            track.paused = matches!(status, PlaybackStatus::Paused);
                            track.version += 1;
                            true
                        });
                    }
                    _ => (),
                },
                Notification::TrackChanged
                | Notification::TrackStarted
                | Notification::TrackEnded => {
                    audio_track.modify(|track| {
                        track.reset();
                        track.version += 1;
                        true
                    });
                }
                Notification::PlaybackPosition(position) => {
                    audio_track.modify(|track| {
                        track.offset = core::time::Duration::from_secs(*position as _);
                        track.version += 1;
                        true
                    });
                }
                _ => (),
            }
        }
        AvrccEvent::Metadata { id, text } => match id {
            MetadataId::Title => audio_track.modify(|track| {
                track.song = (*text).into();
                track.version += 1;
                true
            }),
            MetadataId::Artist => audio_track.modify(|track| {
                track.artist = (*text).into();
                track.version += 1;
                true
            }),
            MetadataId::Album => audio_track.modify(|track| {
                track.album = (*text).into();
                track.version += 1;
                true
            }),
            MetadataId::PlayingTime => audio_track.modify(|track| {
                track.duration = core::time::Duration::from_secs(0); // TODO (*text).into();
                track.version += 1;
                true
            }),
            _ => (),
        },
        _ => (),
    }
}

fn handle_hfpc<'d, M>(
    hfpc: &EspHfpc<'d, M, &BtDriver<'d, M>>,
    phone: &Sender<'_, impl RawMutex, AudioState>,
    phone_call: &StatefulSender<'_, impl RawMutex, PhoneCallInfo>,
    audio_buffers: &SharedAudioBuffers<'_>,
    event: HfpcEvent<'_>,
) -> usize
where
    M: BtClassicEnabled,
{
    match event {
        HfpcEvent::ConnectionState { status, .. } => {
            match status {
                client::ConnectionStatus::Connected | client::ConnectionStatus::SlcConnected => {
                    phone.send(AudioState::Connected)
                }
                client::ConnectionStatus::Disconnected => phone.send(AudioState::Initialized),
                _ => (),
            }

            0
        }
        HfpcEvent::AudioState { status, .. } => {
            match status {
                client::AudioStatus::Connected | client::AudioStatus::ConnectedMsbc => {
                    phone.send(AudioState::Streaming)
                }
                client::AudioStatus::Disconnected => phone.send(AudioState::Suspended),
                _ => (),
            }

            0
        }
        HfpcEvent::CallSetupState(state) if state != CallSetupStatus::Idle => {
            hfpc.request_current_calls().unwrap();

            phone_call.modify(|call| {
                let state = match state {
                    CallSetupStatus::Idle => unreachable!(),
                    CallSetupStatus::Incoming => PhoneCallState::Ringing,
                    CallSetupStatus::OutgoingDialing => PhoneCallState::Dialing,
                    CallSetupStatus::OutgoingAlerting => PhoneCallState::DialingAlerting,
                };

                call.state = state;
                call.version += 1;
                true
            });

            0
        }
        HfpcEvent::CallState(active) => {
            if active {
                hfpc.request_current_calls().unwrap();
            }

            phone_call.modify(|call| {
                if active {
                    call.state = PhoneCallState::CallActive;
                } else {
                    call.reset();
                }

                call.version += 1;
                true
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
            audio_buffers.lock(|buffers| {
                buffers.borrow_mut().push_incoming(data, false, || {
                    hfpc.request_outgoing_data_ready();
                })
            });

            0
        }
        HfpcEvent::SendData(data) => {
            audio_buffers.lock(|buffers| buffers.borrow_mut().pop_outgoing(data, false))
        }
        _ => 0,
    }
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
