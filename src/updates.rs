use core::cell::RefCell;

use embassy_sync::blocking_mutex::raw::RawMutex;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{modem::WifiModemPeripheral, peripheral::Peripheral},
    http::{
        client::{self, EspHttpConnection, FollowRedirectsPolicy},
        Method,
    },
    io::utils::try_read_full,
    ota::{EspFirmwareInfoLoader, EspOta},
    sys::{EspError, ESP_FAIL},
    timer::EspTaskTimerService,
    wifi::{AsyncWifi, AuthMethod, ClientConfiguration, Configuration, EspWifi},
};

use crate::{bus::BusSubscription, error::Error, select_spawn::SelectSpawn, signal::Receiver};

pub async fn process(
    bus: BusSubscription<'_>,
    modem: &RefCell<impl Peripheral<P = impl WifiModemPeripheral>>,
    sysloop: EspSystemEventLoop,
    timer_service: EspTaskTimerService,
) -> Result<(), Error> {
    loop {
        bus.service.wait_enabled().await?;

        loop {
            bus.service.starting();

            let mut modem = modem.borrow_mut();

            let mut driver = AsyncWifi::wrap(
                create(&mut modem, sysloop.clone())?,
                sysloop.clone(),
                timer_service.clone(),
            )?;

            driver.set_configuration(&Configuration::None)?;

            driver.start().await?;

            driver.set_configuration(&Configuration::Client(ClientConfiguration {
                auth_method: AuthMethod::None,
                ..Default::default()
            }))?;

            driver.start().await?;

            bus.service.started();

            let res = SelectSpawn::run(bus.service.wait_disabled())
                .chain(process_update(&mut driver, &bus.update))
                .await;

            driver.stop().await?;

            bus.service.stopped();

            res?;
        }
    }
}

async fn process_update(
    driver: &mut AsyncWifi<EspWifi<'_>>,
    update_request: &Receiver<'_, impl RawMutex, ()>,
) -> Result<(), Error> {
    loop {
        update_request.recv().await;

        connect(driver).await?;

        update().await?;

        driver.stop().await?;
    }
}

async fn connect(driver: &mut AsyncWifi<EspWifi<'_>>) -> Result<(), Error> {
    let (access_points, _) = driver.scan_n::<20>().await?;

    let access_point = access_points
        .iter()
        .filter(|access_point| access_point.auth_method == AuthMethod::None)
        .max_by(|ap1, ap2| ap1.signal_strength.cmp(&ap2.signal_strength));

    if let Some(access_point) = access_point {
        driver.stop().await?;

        driver.set_configuration(&Configuration::Client(ClientConfiguration {
            bssid: access_point.bssid.into(),
            ..Default::default()
        }))?;

        driver.start().await?;

        Ok(())
    } else {
        Err(EspError::from_infallible::<ESP_FAIL>().into()) // TODO
    }
}

async fn update() -> Result<(), Error> {
    let mut http = EspHttpConnection::new(&client::Configuration {
        buffer_size: Some(1024),
        follow_redirects_policy: FollowRedirectsPolicy::FollowAll,
        use_global_ca_store: true,
        ..Default::default()
    })?;

    http.initiate_request(Method::Get, "https:://github.com", &[])?;

    http.initiate_response()?;

    let mut firmware_info_loader = EspFirmwareInfoLoader::new();

    let mut buf = [0; 1024]; // TODO

    let size = try_read_full(&mut http, &mut buf).map_err(|(e, _)| e.0)?;

    firmware_info_loader.load(&buf[..size])?;

    let new_firmware = firmware_info_loader.get_info()?;

    let mut ota = EspOta::new()?;

    let slot = ota.get_running_slot()?;

    let update = if let Some(firmware) = slot.firmware {
        new_firmware.version > firmware.version
    } else {
        true
    };

    if update {
        let mut update = ota.initiate_update()?;

        loop {
            update.write(&buf[..size])?;

            let size = try_read_full(&mut http, &mut buf).map_err(|(e, _)| e.0)?;

            if size == 0 {
                break;
            }
        }

        update.complete()?;
    }

    Ok(())
}

fn create<'d>(
    modem: impl Peripheral<P = impl WifiModemPeripheral> + 'd,
    sysloop: EspSystemEventLoop,
) -> Result<EspWifi<'d>, Error> {
    Ok(EspWifi::new(modem, sysloop, None)?)
}
