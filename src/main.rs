#![feature(new_uninit)]

use std::num::NonZeroU32;
use std::thread;
use std::time::Duration;

use error::Error;
use esp_idf_svc::bt::reduce_bt_memory;
use esp_idf_svc::hal::delay;
use esp_idf_svc::hal::gpio::{InterruptType, PinDriver, Pull};
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
use esp_idf_svc::sys::{heap_caps_print_heap_info, MALLOC_CAP_DEFAULT};
use log::{info, warn};

mod audio;
mod bt;
mod bus;
mod can;
mod commands;
mod displays;
mod error;
mod ringbuf;
mod run;
mod select_spawn;
mod service;
mod signal;
mod updates;
mod usb_cutoff;

fn main() -> Result<(), Error> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    unsafe {
        heap_caps_print_heap_info(MALLOC_CAP_DEFAULT);
    }

    let mut peripherals = Peripherals::take().unwrap();

    // let mut a_pin = PinDriver::input(peripherals.pins.gpio16)?;
    // a_pin.set_pull(Pull::Down)?;

    // a_pin.set_interrupt_type(InterruptType::HighLevel)?;

    // warn!("Interrupt set");

    // let monitor = Monitor::new();

    // let notifier = monitor.notifier();

    // warn!("About to subscribe");

    // unsafe {
    //     a_pin.subscribe(move || {
    //         notifier.notify_and_yield(NonZeroU32::new(1).unwrap());
    //     })?;
    // }

    // warn!("Subscribed");

    // loop {
    //     info!("Looping");

    //     let bits = monitor.wait(delay::BLOCK);

    //     if let Some(bits) = bits {
    //         info!("BITS: {bits:#b}");
    //         a_pin.enable_interrupt()?;
    //     }
    // }

    reduce_bt_memory(&mut peripherals.modem)?;

    unsafe {
        heap_caps_print_heap_info(MALLOC_CAP_DEFAULT);
    }

    ThreadSpawnConfiguration {
        name: Some(b"run\0"),
        ..Default::default()
    }
    .set()?;

    thread::Builder::new()
        .stack_size(10000)
        .spawn(move || run::run(peripherals).unwrap())
        .unwrap();

    Ok(())
}
