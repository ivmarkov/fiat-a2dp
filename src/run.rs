use core::cell::Cell;
use core::mem::MaybeUninit;

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::blocking_mutex::Mutex;

use enumset::EnumSet;

use esp_idf_svc::hal::sys::EspError;
use esp_idf_svc::hal::task::embassy_sync::EspRawMutex;
use esp_idf_svc::hal::task::executor::EspExecutor;
use esp_idf_svc::hal::{adc::AdcMeasurement, peripherals::Peripherals};
use esp_idf_svc::nvs::EspDefaultNvsPartition;

use crate::state::{Service, State};
use crate::{audio, bt, can};

pub fn run(peripherals: Peripherals) -> Result<(), EspError> {
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

    let mut adc_buf = MaybeUninit::<[AdcMeasurement; 1000]>::uninit();
    let mut i2s_buf = MaybeUninit::<[u8; 4000]>::uninit();

    let adc_buf = unsafe { adc_buf.assume_init_mut() };
    let i2s_buf = unsafe { i2s_buf.assume_init_mut() };

    let started_services = &Mutex::new(Cell::new(EnumSet::EMPTY));

    let start_state = &State::<NoopRawMutex, _>::new();
    let phone_state = &State::<EspRawMutex, _>::new();
    let bt_state = &State::<EspRawMutex, _>::new();
    let audio_state = &State::<EspRawMutex, _>::new();
    let radio_state = &State::<NoopRawMutex, _>::new();
    let buttons_state = &State::<NoopRawMutex, _>::new();

    executor
        .spawn_local_collect(
            bt::process(
                modem,
                nvs,
                start_state.receiver(Service::Bt),
                started_services,
                bt_state.sender(),
                audio_state.sender(),
                phone_state.sender(),
            ),
            &mut tasks,
        )
        .unwrap()
        .spawn_local_collect(
            audio::process_state(
                phone_state.receiver(Service::AudioState),
                start_state.receiver(Service::AudioState),
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
                start_state.receiver(Service::AudioOutgoing),
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
                start_state.receiver(Service::AudioIncoming),
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
                start_state.sender(),
                started_services,
                bt_state.receiver(Service::Can),
                audio_state.receiver(Service::Can),
                phone_state.receiver(Service::Can),
                radio_state.receiver(Service::Can),
                radio_state.sender(),
                buttons_state.sender(),
            ),
            &mut tasks,
        )
        .unwrap();

    executor.run_tasks(|| true, tasks);

    Ok(())
}
