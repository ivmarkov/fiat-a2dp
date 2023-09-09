#![feature(type_alias_impl_trait)]

use core::cell::Cell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;

use enumset::EnumSet;

use esp_idf_svc::hal::sys::EspError;
use esp_idf_svc::hal::task::executor::EspExecutor;
use esp_idf_svc::hal::{adc::AdcMeasurement, peripherals::Peripherals};
use esp_idf_svc::nvs::EspDefaultNvsPartition;

use state::StateSignal;

use static_cell::make_static;

mod audio;
mod bt;
mod can;
mod ringbuf;
mod select_spawn;
mod start;
mod state;

fn main() -> Result<(), EspError> {
    esp_idf_svc::sys::link_patches();

    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();

    let modem = peripherals.modem;

    let adc1 = peripherals.adc1;
    let adc_pin = peripherals.pins.gpio32;
    let i2s0 = peripherals.i2s0;

    let i2s = peripherals.i2s1;
    let i2s_bclk = peripherals.pins.gpio25;
    let i2s_dout = peripherals.pins.gpio26;
    let i2s_ws = peripherals.pins.gpio27;

    let can = peripherals.can;
    let tx = peripherals.pins.gpio22;
    let rx = peripherals.pins.gpio23;

    let nvs = EspDefaultNvsPartition::take()?;

    let executor = EspExecutor::<8, _>::new();
    let mut tasks = heapless::Vec::<_, 8>::new();

    let adc_buf = make_static!([AdcMeasurement::INIT; 1000]);
    let i2s_buf = make_static!([0u8; 4000]);

    let started_services = &Mutex::new(Cell::new(EnumSet::EMPTY));

    let start_state_for_bt = &Signal::new();
    let start_state_for_audio_state = &Signal::new();
    let start_state_for_audio_outgoing = &Signal::new();
    let start_state_for_audio_incoming = &Signal::new();

    let phone_state_for_audio = &StateSignal::new();
    let phone_state_for_can = &StateSignal::new();

    let bt_state_for_can = &StateSignal::new();

    let audio_state_for_can = &StateSignal::new();

    let radio_state_for_can = &StateSignal::new();

    executor
        .spawn_local_collect(
            bt::process(
                modem,
                nvs,
                start_state_for_bt,
                started_services,
                [bt_state_for_can],
                [audio_state_for_can],
                [phone_state_for_audio, phone_state_for_can],
            ),
            &mut tasks,
        )
        .unwrap()
        .spawn_local_collect(
            audio::process_state(
                phone_state_for_audio,
                start_state_for_audio_state,
                started_services,
            ),
            &mut tasks,
        )
        .unwrap()
        .spawn_local_collect(
            audio::process_outgoing(
                adc1,
                adc_pin,
                i2s0,
                adc_buf,
                || {},
                start_state_for_audio_outgoing,
                started_services,
            ),
            &mut tasks,
        )
        .unwrap()
        .spawn_local_collect(
            audio::process_incoming(
                i2s,
                i2s_bclk,
                i2s_dout,
                i2s_ws,
                i2s_buf,
                start_state_for_audio_incoming,
                started_services,
            ),
            &mut tasks,
        )
        .unwrap()
        .spawn_local_collect(
            can::process(
                can,
                tx,
                rx,
                started_services,
                bt_state_for_can,
                audio_state_for_can,
                phone_state_for_can,
                radio_state_for_can,
                [
                    start_state_for_bt,
                    start_state_for_audio_state,
                    start_state_for_audio_outgoing,
                    start_state_for_audio_incoming,
                ],
                [radio_state_for_can],
                [],
            ),
            &mut tasks,
        )
        .unwrap();

    executor.run_tasks(|| true, tasks);

    Ok(())
}
