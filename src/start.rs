use core::cell::Cell;

use embassy_sync::blocking_mutex::{raw::RawMutex, Mutex};

use enumset::EnumSet;

use log::info;

use crate::{
    error::Error,
    state::{Receiver, Service, State},
};

pub struct ServiceLifecycle<'d, M>
where
    M: RawMutex,
{
    service: Service,
    start: Receiver<'d, M, bool>,
    started_services: &'d Mutex<M, Cell<EnumSet<Service>>>,
}

impl<'d, M> ServiceLifecycle<'d, M>
where
    M: RawMutex,
{
    pub fn new(
        service: Service,
        start_state: &'d State<M, bool>,
        started_services: &'d Mutex<M, Cell<EnumSet<Service>>>,
    ) -> Self {
        Self {
            service,
            start: start_state.receiver(service),
            started_services,
        }
    }

    pub fn service(&self) -> Service {
        self.service
    }

    pub fn get_all_started(&self) -> EnumSet<Service> {
        self.started_services.lock(Cell::get)
    }

    pub fn starting(&self) {
        info!("Starting service {:?}", self.service);
    }

    pub fn started(&self) {
        self.set_started(true);
    }

    pub fn stopped(&self) {
        self.set_started(false);
    }

    pub async fn wait_stop(&self) -> Result<(), Error> {
        self.wait_start_stop(false).await
    }

    pub async fn wait_start(&self) -> Result<(), Error> {
        self.set_started(false);
        self.wait_start_stop(true).await
    }

    fn set_started(&self, started: bool) {
        let changed = self.started_services.lock(|cell| {
            let mut started_services = cell.get();

            let was_started = started_services.contains(self.service);

            if started != was_started {
                if started {
                    started_services |= self.service;
                } else {
                    started_services &= self.service;
                }

                cell.set(started_services);

                true
            } else {
                false
            }
        });

        if changed {
            if started {
                info!("Service {:?} started", self.service);
            } else {
                info!("Service {:?} stopped", self.service);
            }
        }
    }

    async fn wait_start_stop(&self, wait_start: bool) -> Result<(), Error> {
        while self.start.recv().await != wait_start {}

        Ok(())
    }
}
