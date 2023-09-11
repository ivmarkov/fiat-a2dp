use std::thread;

use esp_idf_svc::bt::reduce_bt_memory;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::hal::sys::EspError;
use esp_idf_svc::sys::{heap_caps_print_heap_info, MALLOC_CAP_DEFAULT};

mod audio;
mod bt;
mod can;
mod ringbuf;
mod run;
mod select_spawn;
mod start;
mod state;

fn main() -> Result<(), EspError> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    unsafe {
        heap_caps_print_heap_info(MALLOC_CAP_DEFAULT);
    }

    reduce_bt_memory()?;

    unsafe {
        heap_caps_print_heap_info(MALLOC_CAP_DEFAULT);
    }

    thread::Builder::new()
        .stack_size(10000)
        .spawn(move || run::run(Peripherals::take().unwrap()).unwrap())
        .unwrap();

    Ok(())
}
