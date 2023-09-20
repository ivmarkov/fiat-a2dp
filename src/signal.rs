use std::cell::RefCell;

use embassy_sync::{
    blocking_mutex::{raw::RawMutex, Mutex},
    signal::Signal,
};

use crate::state::Service;

const MAX_RECEIVERS: usize = 8;

pub struct SpmcSignal<M, T>([Signal<M, T>; MAX_RECEIVERS])
where
    M: RawMutex;

impl<M, T> SpmcSignal<M, T>
where
    M: RawMutex,
{
    const INIT: Signal<M, T> = Signal::new();

    pub const fn new() -> Self {
        Self([Self::INIT; MAX_RECEIVERS])
    }

    pub fn receiver(&self, service: Service) -> Receiver<'_, M, T> {
        let index = service as usize;

        Receiver(&self.0[index])
    }

    pub fn sender(&self) -> Sender<'_, M, T> {
        Sender(&self.0)
    }
}

pub struct Receiver<'a, M, T>(&'a Signal<M, T>)
where
    M: RawMutex;

impl<'a, M, T> Receiver<'a, M, T>
where
    M: RawMutex,
    T: Send,
{
    pub async fn recv(&self) -> T {
        self.0.wait().await
    }
}

pub struct Sender<'a, M, T>(&'a [Signal<M, T>])
where
    M: RawMutex;

impl<'a, M, T> Sender<'a, M, T>
where
    M: RawMutex,
    T: Send + Clone,
{
    pub fn send(&self, value: T) {
        for signal in self.0 {
            signal.signal(value.clone());
        }
    }
}

pub struct SharedStateSpmcSignal<M, S>
where
    M: RawMutex,
{
    state: Mutex<M, RefCell<S>>,
    signal: SpmcSignal<M, ()>,
}

impl<M, S> SharedStateSpmcSignal<M, S>
where
    M: RawMutex,
{
    pub const fn new(state: S) -> Self {
        Self {
            state: Mutex::new(RefCell::new(state)),
            signal: SpmcSignal::new(),
        }
    }

    pub fn receiver(&self, service: Service) -> SharedStateReceiver<'_, M, S> {
        SharedStateReceiver(self.signal.receiver(service), &self.state)
    }

    pub fn sender(&self) -> SharedStateSender<'_, M, S> {
        SharedStateSender(&self.signal.0, &self.state)
    }
}

pub struct SharedStateReceiver<'a, M, S>(Receiver<'a, M, ()>, &'a Mutex<M, RefCell<S>>)
where
    M: RawMutex;

impl<'a, M, S> SharedStateReceiver<'a, M, S>
where
    M: RawMutex,
{
    pub async fn recv(&self) {
        self.0.recv().await
    }

    pub fn state<R, F: FnMut(&S) -> R>(&self, mut f: F) -> R {
        self.1.lock(|state| f(&state.borrow()))
    }
}

pub struct SharedStateSender<'a, M, S>(&'a [Signal<M, ()>], &'a Mutex<M, RefCell<S>>)
where
    M: RawMutex;

impl<'a, M, S> SharedStateSender<'a, M, S>
where
    M: RawMutex,
{
    pub fn modify<F: FnMut(&mut S) -> bool>(&self, mut f: F) {
        self.1.lock(|state| {
            if f(&mut state.borrow_mut()) {
                for signal in self.0 {
                    signal.signal(());
                }
            }
        })
    }
}
