use core::mem::MaybeUninit;

use edge_executor::LocalExecutor;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::task::block_on;
use esp_idf_svc::hal::{adc::AdcMeasurement, peripherals::Peripherals};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::timer::EspTimerService;

use log::warn;

use crate::audio::create_audio_buffers;
use crate::bus::{Bus, Service};
use crate::error::Error;
use crate::usb_cutoff::UsbCutoff;
use crate::{audio, bt, can, commands, displays, updates};

pub fn run(peripherals: Peripherals) -> Result<(), Error> {
    let modem = Mutex::<NoopRawMutex, _>::new(peripherals.modem);

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

    let mut str_buf = heapless::String::<32>::new();

    let str_buf = &mut str_buf;

    let nvs = EspDefaultNvsPartition::take()?;

    warn!("Before allocations");

    let mut adc_buf: Box<MaybeUninit<[AdcMeasurement; 1000]>> = Box::new_uninit();
    let mut i2s_buf: Box<MaybeUninit<[u8; 4000]>> = Box::new_uninit();

    let adc_buf = unsafe { adc_buf.assume_init_mut() };
    let i2s_buf = unsafe { i2s_buf.assume_init_mut() };

    warn!("ADC/I2S bufs allocated: {:p} {:p}", adc_buf, i2s_buf);

    let bus = Bus::new();

    bus.system.sender().modify(|system| {
        system.set_normal_mode();
        true
    });

    let mut audio_incoming: Box<MaybeUninit<[u8; 32768]>> = Box::new_uninit();
    let mut audio_outgoing: Box<MaybeUninit<[u8; 8192]>> = Box::new_uninit();

    warn!(
        "Audio bufs allocated {:p}, {:p}",
        &audio_incoming, &audio_outgoing
    );

    let audio_buffers = create_audio_buffers(unsafe { audio_incoming.assume_init_mut() }, unsafe {
        audio_outgoing.assume_init_mut()
    });

    let executor: LocalExecutor = Default::default();

    warn!("Spawning");

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

    executor
        .spawn(can::process(
            bus.subscription(Service::Can),
            can,
            tx,
            rx,
            str_buf,
            bus.radio.sender(),
            bus.buttons.sender(),
            bus.radio_commands.sender(),
        ))
        .detach();

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
        .spawn(updates::process(
            bus.subscription(Service::Wifi),
            &modem,
            EspSystemEventLoop::take()?,
            EspTimerService::new()?,
        ))
        .detach();

    // executor
    //     .spawn(
    //         async move {
    //             loop {
    //                 Timer::after(Duration::from_secs(10)).await;

    //                 unsafe {
    //                     heap_caps_print_heap_info(MALLOC_CAP_DEFAULT);
    //                 }
    //             }
    //         },
    //     )
    //     .detach();

    block_on(executor.run(core::future::pending::<()>()));

    Ok(())
}
