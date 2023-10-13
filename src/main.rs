#![feature(new_uninit)]

use std::thread;

use error::Error;
use esp_idf_svc::bt::reduce_bt_memory;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::task::thread::ThreadSpawnConfiguration;
use esp_idf_svc::sys::{heap_caps_print_heap_info, MALLOC_CAP_DEFAULT};

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
        .stack_size(20000)
        .spawn(move || run::run(peripherals).unwrap())
        .unwrap();

    Ok(())
}
