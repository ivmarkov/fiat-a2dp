use core::cell::Cell;

use embassy_sync::blocking_mutex::{
    raw::{NoopRawMutex, RawMutex},
    Mutex,
};

use enumset::EnumSet;

use esp_idf_svc::sys::EspError;

use log::info;

use crate::state::{Receiver, Service};

pub fn get_started_services(
    started_services: &Mutex<NoopRawMutex, Cell<EnumSet<Service>>>,
) -> EnumSet<Service> {
    started_services.lock(Cell::get)
}

pub fn set_service_started(
    started_services: &Mutex<NoopRawMutex, Cell<EnumSet<Service>>>,
    service: Service,
    started: bool,
) {
    started_services.lock(|cell| {
        let mut started_services = cell.get();

        if started {
            started_services |= service;
        } else {
            started_services &= service;
        }

        cell.set(started_services);
    });

    if started {
        info!("Service {:?} started", service);
    } else {
        info!("Service {:?} stopped", service);
    }
}

pub async fn wait_start(
    start: &Receiver<'_, impl RawMutex, bool>,
    wait_start: bool,
) -> Result<(), EspError> {
    while start.recv().await != wait_start {}

    Ok(())
}
