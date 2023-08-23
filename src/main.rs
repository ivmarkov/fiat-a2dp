#![feature(type_alias_impl_trait)]

use esp_idf_svc::hal::sys::EspError;
use esp_idf_svc::hal::task::executor::EspExecutor;
use esp_idf_svc::hal::{adc::AdcMeasurement, peripherals::Peripherals};
use esp_idf_svc::nvs::EspDefaultNvsPartition;

use static_cell::make_static;

mod audio;
mod bt;
mod ringbuf;

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

    let nvs = EspDefaultNvsPartition::take()?;

    let executor = EspExecutor::<8, _>::new();
    let mut tasks = heapless::Vec::<_, 8>::new();

    let adc_buf = make_static!([AdcMeasurement::INIT; 1000]);
    let i2s_buf = make_static!([0u8; 4000]);

    executor
        .spawn_local_collect(
            async move {
                bt::process(modem, nvs).await.unwrap();
            },
            &mut tasks,
        )
        .unwrap()
        .spawn_local_collect(
            async move {
                audio::process_outgoing(adc1, adc_pin, i2s0, adc_buf, || {})
                    .await
                    .unwrap();
            },
            &mut tasks,
        )
        .unwrap()
        .spawn_local_collect(
            async move {
                audio::process_incoming(i2s, i2s_bclk, i2s_dout, i2s_ws, i2s_buf)
                    .await
                    .unwrap();
            },
            &mut tasks,
        )
        .unwrap();

    executor.run_tasks(|| true, tasks);

    Ok(())
}
