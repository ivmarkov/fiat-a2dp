use core::mem::MaybeUninit;

use embassy_time::{Duration, Timer};

use esp_idf_svc::hal::task::executor::EspExecutor;
use esp_idf_svc::hal::{adc::AdcMeasurement, peripherals::Peripherals};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::{heap_caps_print_heap_info, MALLOC_CAP_DEFAULT};

use crate::error::Error;
use crate::state::{Service, State};
use crate::{audio, bt, can, commands, displays};

pub fn run(peripherals: Peripherals) -> Result<(), Error> {
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

    let executor = EspExecutor::<16, _>::new();
    let mut tasks = heapless::Vec::<_, 16>::new();

    let mut adc_buf = MaybeUninit::<[AdcMeasurement; 1000]>::uninit();
    let mut i2s_buf = MaybeUninit::<[u8; 4000]>::uninit();

    let adc_buf = unsafe { adc_buf.assume_init_mut() };
    let i2s_buf = unsafe { i2s_buf.assume_init_mut() };

    let state = State::new();

    executor
        .spawn_local_collect(
            bt::process(
                modem,
                nvs,
                state.subscription(Service::Bt),
                state.bt.sender(),
                state.audio.sender(),
                state.audio_track.sender(),
                state.phone.sender(),
                state.phone_call.sender(),
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            audio::process_audio_mux(state.subscription(Service::AudioMux)),
            &mut tasks,
        )?
        .spawn_local_collect(
            audio::process_microphone(
                state.subscription(Service::Microphone),
                adc1,
                adc_pin,
                i2s0,
                adc_buf,
                || {},
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            audio::process_speakers(
                state.subscription(Service::Speakers),
                i2s,
                i2s_bclk,
                i2s_dout,
                i2s_ws,
                i2s_buf,
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            can::process(
                state.subscription(Service::Can),
                can,
                tx,
                rx,
                state.radio.sender(),
                state.buttons.sender(),
                state.radio_commands.sender(),
                state.start.sender(),
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            displays::process_radio(
                state.subscription(Service::RadioDisplay),
                state.radio_display.sender(),
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            commands::process(
                state.subscription(Service::Can),
                state.button_commands.sender(),
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            async move {
                loop {
                    Timer::after(Duration::from_secs(10)).await;

                    unsafe {
                        heap_caps_print_heap_info(MALLOC_CAP_DEFAULT);
                    }
                }

                Ok(())
            },
            &mut tasks,
        )?;

    executor.run_tasks(|| true, tasks);

    Ok(())
}
