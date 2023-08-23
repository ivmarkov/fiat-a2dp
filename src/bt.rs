use esp_idf_svc::sys::EspError;
use esp_idf_svc::{
    bt::{
        a2dp::{A2dpEvent, EspA2dp, SinkEnabled},
        avrc::controller::{AvrccEvent, EspAvrcc},
        avrc::{MetadataId, NotificationType},
        gap::{
            Cod, CodMajorDeviceType, CodMode, CodServiceClass, DeviceProp, DiscoveryMode, EspGap,
            GapEvent, IOCapabilities, InqMode, PropData,
        },
        hfp::client::{AudioStatus, EspHfpc, HfpcEvent},
        BtClassic, BtClassicEnabled, BtDriver,
    },
    nvs::EspDefaultNvsPartition,
};

use esp_idf_svc::hal::{modem::BluetoothModemPeripheral, peripheral::Peripheral};

use log::*;

use crate::audio::AUDIO_BUFFERS;

pub async fn process<'d>(
    modem: impl Peripheral<P = impl BluetoothModemPeripheral> + 'd,
    nvs: EspDefaultNvsPartition,
) -> Result<(), EspError> {
    let bt = BtDriver::<BtClassic>::new(modem, Some(nvs))?;

    bt.set_device_name("Fiat")?;

    info!("Bluetooth initialized");

    let gap = EspGap::new(&bt)?;

    info!("GAP created");

    let avrcc = EspAvrcc::new(&bt)?;

    info!("AVRCC created");

    let a2dp = EspA2dp::new_sink(&bt)?;

    info!("A2DP created");

    let hfpc = EspHfpc::new(&bt, None)?;

    info!("HFPC created");

    gap.initialize(|event| handle_gap(&gap, event))?;

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

    avrcc.initialize(|event| handle_avrcc(&avrcc, event))?;

    info!("AVRCC initialized");

    a2dp.initialize(|event| handle_a2dp(&a2dp, event))?;

    info!("A2DP initialized");

    hfpc.initialize(|event| handle_hfpc(&hfpc, event))?;

    info!("HFPC initialized");

    a2dp.set_delay(core::time::Duration::from_millis(150))?;

    core::future::pending().await
}

fn handle_gap<'d, M>(gap: &EspGap<'d, M, &BtDriver<'d, M>>, event: GapEvent<'_>)
where
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
    event: A2dpEvent<'_>,
) where
    M: BtClassicEnabled,
{
    match event {
        A2dpEvent::SinkData(data) => {
            AUDIO_BUFFERS.lock(|buffers| {
                let mut buffers = buffers.borrow_mut();

                buffers.push_incoming(data, true, || {});
            });
        }
        _ => (),
    }
}

fn handle_hfpc<'d, M>(hfpc: &EspHfpc<'d, M, &BtDriver<'d, M>>, event: HfpcEvent<'_>) -> usize
where
    M: BtClassicEnabled,
{
    match event {
        HfpcEvent::AudioState { status, .. } => {
            AUDIO_BUFFERS.lock(|buffers| {
                buffers.borrow_mut().set_a2dp(!matches!(
                    status,
                    AudioStatus::Connected | AudioStatus::ConnectedMsbc
                ));
            });

            0
        }
        HfpcEvent::RecvData(data) => {
            AUDIO_BUFFERS.lock(|buffers| {
                let mut buffers = buffers.borrow_mut();

                buffers.push_incoming(data, false, || {
                    hfpc.request_outgoing_data_ready();
                });
            });

            0
        }
        HfpcEvent::SendData(data) => AUDIO_BUFFERS.lock(|buffers| {
            let mut buffers = buffers.borrow_mut();

            buffers.pop_outgoing(data, false)
        }),
        _ => 0,
    }
}

fn handle_avrcc<'d, M>(avrcc: &EspAvrcc<'d, M, &BtDriver<'d, M>>, event: AvrccEvent<'_>)
where
    M: BtClassicEnabled,
{
    match &event {
        AvrccEvent::Connected(_) | AvrccEvent::Notification(_) => {
            if matches!(event, AvrccEvent::Connected(_)) {
                avrcc.request_capabilities(0).unwrap();
            }

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
                    MetadataId::Title
                        | MetadataId::Artist
                        | MetadataId::Album
                        | MetadataId::PlayingTime,
                )
                .unwrap();
            // avrcc
            //     .register_notification(5, NotificationType::TrackStart, 0)
            //     .unwrap();
            // avrcc
            //     .register_notification(6, NotificationType::TrackEnd, 0)
            //     .unwrap();
        }
        _ => (),
    }
}
