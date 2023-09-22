use embassy_sync::blocking_mutex::raw::RawMutex;

use enumset::{enum_set, EnumSet};

use log::info;

use crate::{
    bus::Service,
    error::Error,
    signal::{StatefulBroadcastSignal, StatefulReceiver, StatefulSender},
};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SystemState {
    Stopped,
    Starting,
    Started,
    Stopping,
}

const ALWAYS_ON: EnumSet<Service> =
    enum_set!(Service::Can | Service::CockpitDisplay | Service::RadioDisplay | Service::Commands);

pub struct System {
    enabled: EnumSet<Service>,
    always_on: EnumSet<Service>,
    started: EnumSet<Service>,
    sys_enabled: bool,
}

impl System {
    pub const fn new(enabled: EnumSet<Service>, always_on: EnumSet<Service>) -> Self {
        Self {
            enabled,
            always_on,
            started: EnumSet::EMPTY,
            sys_enabled: true,
        }
    }

    pub fn set_update_mode(&mut self) {
        self.enabled = enum_set!(Service::Wifi) & !ALWAYS_ON;
    }

    pub fn set_normal_mode(&mut self) {
        self.enabled = EnumSet::ALL & !(Service::Wifi | ALWAYS_ON);
    }

    pub fn get_state(&self) -> SystemState {
        if self.sys_enabled {
            if self.started == self.enabled | self.always_on {
                SystemState::Started
            } else {
                SystemState::Starting
            }
        } else {
            if self.started == self.always_on {
                SystemState::Stopped
            } else {
                SystemState::Stopping
            }
        }
    }
}

pub struct ServiceLifecycle<'d, M>
where
    M: RawMutex,
{
    service: Service,
    receiver: StatefulReceiver<'d, M, System>,
    sender: StatefulSender<'d, M, System>,
}

impl<'d, M> ServiceLifecycle<'d, M>
where
    M: RawMutex,
{
    pub fn new(service: Service, system: &'d StatefulBroadcastSignal<M, System>) -> Self {
        Self {
            service,
            receiver: system.receiver(service),
            sender: system.sender(),
        }
    }

    pub fn service(&self) -> Service {
        self.service
    }

    pub fn get_sys_state(&self) -> SystemState {
        self.receiver.state(|state| state.get_state())
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

    pub fn sys_start(&self) {
        self.sender.modify(|sys| {
            if !sys.sys_enabled {
                sys.sys_enabled = true;
                true
            } else {
                false
            }
        });
    }

    pub fn sys_stop(&self) {
        self.sender.modify(|sys| {
            if sys.sys_enabled {
                sys.sys_enabled = false;
                true
            } else {
                false
            }
        });
    }

    pub async fn wait_disabled(&self) -> Result<(), Error> {
        self.wait_enabled_disabled(false).await
    }

    pub async fn wait_enabled(&self) -> Result<(), Error> {
        self.set_started(false);
        self.wait_enabled_disabled(true).await
    }

    fn set_started(&self, started: bool) {
        self.sender.modify(|state| {
            let was_started = state.started.contains(self.service);

            if started != was_started {
                if started {
                    state.started |= self.service;
                    info!("Service {:?} started", self.service);
                } else {
                    state.started &= self.service;
                    info!("Service {:?} stopped", self.service);
                }

                true
            } else {
                false
            }
        });
    }

    async fn wait_enabled_disabled(&self, wait_enabled: bool) -> Result<(), Error> {
        loop {
            self.receiver.recv().await;

            let enabled = self.receiver.state(|state| {
                if state.sys_enabled {
                    state.enabled.contains(self.service) | state.always_on.contains(self.service)
                } else {
                    state.always_on.contains(self.service)
                }
            });

            if enabled == wait_enabled {
                break;
            }
        }

        Ok(())
    }
}
