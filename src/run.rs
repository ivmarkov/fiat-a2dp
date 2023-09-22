use core::mem::MaybeUninit;

use embassy_time::{Duration, Timer};

use esp_idf_svc::hal::task::executor::EspExecutor;
use esp_idf_svc::hal::{adc::AdcMeasurement, peripherals::Peripherals};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::{heap_caps_print_heap_info, MALLOC_CAP_DEFAULT};

use crate::bus::{Bus, Service};
use crate::error::Error;
use crate::flash_mode::FlashMode;
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

    let flash_mode_flash = peripherals.pins.gpio12;
    let flash_mode_reset = peripherals.pins.gpio13;

    let nvs = EspDefaultNvsPartition::take()?;

    let executor = EspExecutor::<16, _>::new();
    let mut tasks = heapless::Vec::<_, 16>::new();

    let mut adc_buf = MaybeUninit::<[AdcMeasurement; 1000]>::uninit();
    let mut i2s_buf = MaybeUninit::<[u8; 4000]>::uninit();

    let adc_buf = unsafe { adc_buf.assume_init_mut() };
    let i2s_buf = unsafe { i2s_buf.assume_init_mut() };

    let bus = Bus::new();

    executor
        .spawn_local_collect(
            bt::process(
                modem,
                nvs,
                bus.subscription(Service::Bt),
                bus.bt.sender(),
                bus.audio.sender(),
                bus.audio_track.sender(),
                bus.phone.sender(),
                bus.phone_call.sender(),
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            audio::process_audio_mux(bus.subscription(Service::AudioMux)),
            &mut tasks,
        )?
        .spawn_local_collect(
            audio::process_microphone(
                bus.subscription(Service::Microphone),
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
                bus.subscription(Service::Speakers),
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
                bus.subscription(Service::Can),
                can,
                tx,
                rx,
                bus.radio.sender(),
                bus.buttons.sender(),
                bus.radio_commands.sender(),
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            displays::process_radio(
                bus.subscription(Service::RadioDisplay),
                bus.radio_display.sender(),
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            commands::process(
                bus.subscription(Service::Can),
                FlashMode::new(flash_mode_flash, flash_mode_reset)?,
                bus.button_commands.sender(),
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
