use core::cell::RefCell;
use core::mem::MaybeUninit;
use std::future::Future;

use async_executor::{LocalExecutor, Task};
use embassy_time::{Duration, Timer};

//use edge_executor::EspExecutor;

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::gpio::{PinDriver, Pull};
//use esp_idf_svc::hal::interrupt::asynch::HAL_WAKE_RUNNER;
use esp_idf_svc::hal::{adc::AdcMeasurement, peripherals::Peripherals};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::{heap_caps_print_heap_info, EspError, MALLOC_CAP_DEFAULT};
use esp_idf_svc::timer::EspTimerService;

use log::{error, warn};

use crate::audio::create_audio_buffers;
use crate::bus::{Bus, Service};
use crate::error::Error;
use crate::usb_cutoff::UsbCutoff;
use crate::{audio, bt, can, commands, displays, updates};

pub fn run(peripherals: Peripherals) -> Result<(), Error> {
    let modem = RefCell::new(peripherals.modem);

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

    let usb_cutoff = peripherals.pins.gpio13;

    let nvs = EspDefaultNvsPartition::take()?;

    warn!("Before allocations");

    let mut adc_buf: Box<MaybeUninit<[AdcMeasurement; 1000]>> = Box::new_uninit();
    let mut i2s_buf: Box<MaybeUninit<[u8; 4000]>> = Box::new_uninit();

    let adc_buf = unsafe { adc_buf.assume_init_mut() };
    let i2s_buf = unsafe { i2s_buf.assume_init_mut() };

    warn!("ADC/I2S bufs allocated: {:p} {:p}", adc_buf, i2s_buf);

    let bus = Bus::new();

    let mut audio_incoming: Box<MaybeUninit<[u8; 32768]>> = Box::new_uninit();
    let mut audio_outgoing: Box<MaybeUninit<[u8; 8192]>> = Box::new_uninit();

    warn!(
        "Audio bufs allocated {:p}, {:p}",
        &audio_incoming, &audio_outgoing
    );

    let audio_buffers = create_audio_buffers(unsafe { audio_incoming.assume_init_mut() }, unsafe {
        audio_outgoing.assume_init_mut()
    });

    let mut a_pin = PinDriver::input(peripherals.pins.gpio16)?;
    a_pin.set_pull(Pull::Down)?;

    // let executor = EspExecutor::<16, _>::new();
    // let mut tasks = heapless::Vec::<_, 16>::new();
    let executor = LocalExecutor::new();

    warn!("Spawning");

    // executor
    //     .spawn_local_collect(
    //         bt::process(
    //             &modem,
    //             nvs,
    //             bus.subscription(Service::Bt),
    //             bus.bt.sender(),
    //             bus.audio.sender(),
    //             bus.audio_track.sender(),
    //             bus.phone.sender(),
    //             bus.phone_call.sender(),
    //             &audio_buffers,
    //         ),
    //         &mut tasks,
    //     )?
    //     .spawn_local_collect(
    //         audio::process_audio_mux(bus.subscription(Service::AudioMux), &audio_buffers),
    //         &mut tasks,
    //     )?
    //     .spawn_local_collect(
    //         audio::process_microphone(
    //             bus.subscription(Service::Microphone),
    //             adc1,
    //             adc_pin,
    //             i2s0,
    //             adc_buf,
    //             &audio_buffers,
    //             || {},
    //         ),
    //         &mut tasks,
    //     )?
    //     .spawn_local_collect(
    //         audio::process_speakers(
    //             bus.subscription(Service::Speakers),
    //             i2s,
    //             i2s_bclk,
    //             i2s_dout,
    //             i2s_ws,
    //             &audio_buffers,
    //             i2s_buf,
    //         ),
    //         &mut tasks,
    //     )?
    //     // .spawn_local_collect(
    //     //     can::process(
    //     //         bus.subscription(Service::Can),
    //     //         can,
    //     //         tx,
    //     //         rx,
    //     //         bus.radio.sender(),
    //     //         bus.buttons.sender(),
    //     //         bus.radio_commands.sender(),
    //     //     ),
    //     //     &mut tasks,
    //     // )?
    // //     .spawn_local_collect(
    // //         displays::process_radio(
    // //             bus.subscription(Service::RadioDisplay),
    // //             bus.radio_display.sender(),
    // //         ),
    // //         &mut tasks,
    // //     )?
    //     .spawn_local_collect(
    //         commands::process(
    //             bus.subscription(Service::Commands),
    //             UsbCutoff::new(usb_cutoff)?,
    //             bus.button_commands.sender(),
    //         ),
    //         &mut tasks,
    //     )?
    // //     // .spawn_local_collect(
    // //     //     updates::process(
    // //     //         bus.subscription(Service::Wifi),
    // //     //         &modem,
    // //     //         EspSystemEventLoop::take()?,
    // //     //         EspTimerService::new()?,
    // //     //     ),
    // //     //     &mut tasks,
    // //     // )?
    // //     .spawn_local_collect(
    // //         async move {
    // //             loop {
    // //                 Timer::after(Duration::from_secs(10)).await;

    // //                 unsafe {
    // //                     heap_caps_print_heap_info(MALLOC_CAP_DEFAULT);
    // //                 }
    // //             }

    // //             Ok(())
    // //         },
    // //         &mut tasks,
    // //     )?
    // ;

    // executor.run_tasks(|| true, tasks);

    executor
        .spawn(bt::process(
            &modem,
            nvs,
            bus.subscription(Service::Bt),
            bus.bt.sender(),
            bus.audio.sender(),
            bus.audio_track.sender(),
            bus.phone.sender(),
            bus.phone_call.sender(),
            &audio_buffers,
        ))
        .detach();

    executor
        .spawn(audio::process_audio_mux(
            bus.subscription(Service::AudioMux),
            &audio_buffers,
        ))
        .detach();

    executor
        .spawn(audio::process_microphone(
            bus.subscription(Service::Microphone),
            adc1,
            adc_pin,
            i2s0,
            adc_buf,
            &audio_buffers,
            || {},
        ))
        .detach();

    executor
        .spawn(audio::process_speakers(
            bus.subscription(Service::Speakers),
            i2s,
            i2s_bclk,
            i2s_dout,
            i2s_ws,
            &audio_buffers,
            i2s_buf,
        ))
        .detach();

    // .spawn_local_collect(
    //     can::process(
    //         bus.subscription(Service::Can),
    //         can,
    //         tx,
    //         rx,
    //         bus.radio.sender(),
    //         bus.buttons.sender(),
    //         bus.radio_commands.sender(),
    //     ),
    //     &mut tasks,
    // )?
    executor
        .spawn(displays::process_radio(
            bus.subscription(Service::RadioDisplay),
            bus.radio_display.sender(),
        ))
        .detach();

    executor
        .spawn(commands::process(
            bus.subscription(Service::Commands),
            UsbCutoff::new(usb_cutoff)?,
            bus.button_commands.sender(),
        ))
        .detach();

    executor
        .spawn(async move {
            loop {
                a_pin.wait_for_high().await.unwrap();
                Timer::after(Duration::from_millis(50)).await;
            }
        })
        .detach();

    //     // .spawn_local_collect(
    //     //     updates::process(
    //     //         bus.subscription(Service::Wifi),
    //     //         &modem,
    //     //         EspSystemEventLoop::take()?,
    //     //         EspTimerService::new()?,
    //     //     ),
    //     //     &mut tasks,
    //     // )?
    //     .spawn_local_collect(
    //         async move {
    //             loop {
    //                 Timer::after(Duration::from_secs(10)).await;

    //                 unsafe {
    //                     heap_caps_print_heap_info(MALLOC_CAP_DEFAULT);
    //                 }
    //             }

    //             Ok(())
    //         },
    //         &mut tasks,
    //     )?

    futures_lite::future::block_on(executor.run(core::future::pending::<()>()));

    Ok(())
}
