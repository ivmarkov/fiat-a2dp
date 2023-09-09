#![feature(type_alias_impl_trait)]

use core::borrow::BorrowMut;
use std::marker::PhantomData;

use esp_idf_svc::hal::gpio::{InputMode, InputPin, PinDriver, RTCMode};
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

    let phone_state_for_audio = &StateSignal::new();

    let bt_state_for_can = &StateSignal::new();
    let audio_state_for_can = &StateSignal::new();
    let phone_state_for_can = &StateSignal::new();
    let radio_state_for_can = &StateSignal::new();

    executor
        .spawn_local_collect(
            bt::process(modem, nvs, [], [], [phone_state_for_audio]),
            &mut tasks,
        )
        .unwrap()
        .spawn_local_collect(audio::process_state(phone_state_for_audio), &mut tasks)
        .unwrap()
        .spawn_local_collect(
            audio::process_outgoing(adc1, adc_pin, i2s0, adc_buf, || {}),
            &mut tasks,
        )
        .unwrap()
        .spawn_local_collect(
            audio::process_incoming(i2s, i2s_bclk, i2s_dout, i2s_ws, i2s_buf),
            &mut tasks,
        )
        .unwrap()
        .spawn_local_collect(
            can::process(can, tx, rx, bt_state_for_can, audio_state_for_can, phone_state_for_can, radio_state_for_can, [radio_state_for_can], []), 
            &mut tasks,
        )
        .unwrap();

    executor.run_tasks(|| true, tasks);

    Ok(())
}

pub struct Foo<'d>(u8, PhantomData<&'d mut ()>);

impl<'d> Foo<'d> {
    pub fn new<T, P, M>(driver: T) -> Self
    where
        T: BorrowMut<PinDriver<'d, P, M>>,
        P: InputPin,
        M: InputMode + RTCMode,
    {
        Self(driver.borrow().pin() as u8, PhantomData)
    }
}

fn test0<'d1, 'd2, P1, M1, P2, M2>(
    driver1: &mut PinDriver<'d1, P1, M1>,
    driver2: &mut PinDriver<'d2, P2, M2>,
) where
    P1: InputPin,
    M1: InputMode + RTCMode,
    P2: InputPin,
    M2: InputMode + RTCMode,
    'd2: 'd1,
{
    let foos = test(driver1, driver2);

    driver1.pin();

    &foos[0];
}

fn test<'d1, 'd2, P1, M1, P2, M2>(
    driver1: &mut PinDriver<'d1, P1, M1>,
    driver2: &mut PinDriver<'d2, P2, M2>,
) -> [Foo<'d1>; 2]
where
    P1: InputPin,
    M1: InputMode + RTCMode,
    P2: InputPin,
    M2: InputMode + RTCMode,
    'd2: 'd1,
{
    let foo1 = Foo::new(driver1);
    let foo2 = Foo::new(driver2);

    [foo1, foo2]
}
