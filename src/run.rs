use core::cell::{Cell, RefCell};
use core::mem::MaybeUninit;

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::blocking_mutex::Mutex;

use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use enumset::EnumSet;

use esp_idf_svc::hal::task::embassy_sync::EspRawMutex;
use esp_idf_svc::hal::task::executor::EspExecutor;
use esp_idf_svc::hal::{adc::AdcMeasurement, peripherals::Peripherals};
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::sys::{heap_caps_print_heap_info, MALLOC_CAP_DEFAULT};

use crate::displays::DisplayText;
use crate::error::Error;
use crate::start::ServiceLifecycle;
use crate::state::{PhoneCallInfo, Service, State, TrackInfo};
use crate::{audio, bt, can, commands, displays};

static TRACK_INFO: Mutex<EspRawMutex, RefCell<TrackInfo>> =
    Mutex::new(RefCell::new(TrackInfo::new()));
static PHONE_CALL_INFO: Mutex<EspRawMutex, RefCell<PhoneCallInfo>> =
    Mutex::new(RefCell::new(PhoneCallInfo::new()));

static COCKPIT_DISPLAY_TEXT: Mutex<EspRawMutex, RefCell<DisplayText>> =
    Mutex::new(RefCell::new(DisplayText::new()));
static RADIO_DISPLAY_TEXT: Mutex<EspRawMutex, RefCell<DisplayText>> =
    Mutex::new(RefCell::new(DisplayText::new()));

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

    let started_services = &Mutex::new(Cell::new(EnumSet::EMPTY));

    let start_state = &State::<NoopRawMutex, _>::new();
    let bt_state = &State::<EspRawMutex, _>::new();
    let audio_state = &State::<EspRawMutex, _>::new();
    let audio_track_state = &State::<EspRawMutex, _>::new();
    let phone_state = &State::<EspRawMutex, _>::new();
    let phone_call_state = &State::<EspRawMutex, _>::new();
    let radio_state = &State::<NoopRawMutex, _>::new();
    let buttons_state = &State::<NoopRawMutex, _>::new();
    let commands_state = &State::<NoopRawMutex, _>::new();

    let cockpit_display = &Signal::<NoopRawMutex, _>::new();
    let radio_display = &Signal::<NoopRawMutex, _>::new();

    executor
        .spawn_local_collect(
            bt::process(
                ServiceLifecycle::new(Service::Bt, start_state, started_services),
                modem,
                nvs,
                bt_state.sender(),
                audio_state.sender(),
                audio_track_state.sender(),
                &TRACK_INFO,
                phone_state.sender(),
                phone_call_state.sender(),
                &PHONE_CALL_INFO,
                commands_state.receiver(Service::Bt),
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            audio::process_state(
                ServiceLifecycle::new(Service::AudioState, start_state, started_services),
                phone_state.receiver(Service::AudioState),
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            audio::process_outgoing(
                ServiceLifecycle::new(Service::AudioOutgoing, start_state, started_services),
                adc1,
                adc_pin,
                i2s0,
                adc_buf,
                || {},
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            audio::process_incoming(
                ServiceLifecycle::new(Service::AudioIncoming, start_state, started_services),
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
                ServiceLifecycle::new(Service::Can, start_state, started_services),
                can,
                tx,
                rx,
                audio_state.receiver(Service::Can),
                phone_state.receiver(Service::Can),
                cockpit_display,
                &COCKPIT_DISPLAY_TEXT,
                radio_display,
                &RADIO_DISPLAY_TEXT,
                radio_state.receiver(Service::Can),
                radio_state.sender(),
                buttons_state.sender(),
                start_state.sender(),
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            displays::process_radio(
                ServiceLifecycle::new(Service::RadioDisplay, start_state, started_services),
                audio_track_state.receiver(Service::RadioDisplay),
                &TRACK_INFO,
                phone_call_state.receiver(Service::RadioDisplay),
                &PHONE_CALL_INFO,
                radio_state.receiver(Service::RadioDisplay),
                &RADIO_DISPLAY_TEXT,
                radio_display,
            ),
            &mut tasks,
        )?
        .spawn_local_collect(
            commands::process(
                ServiceLifecycle::new(Service::Commands, start_state, started_services),
                audio_state.receiver(Service::Commands),
                audio_track_state.receiver(Service::Commands),
                phone_state.receiver(Service::Commands),
                phone_call_state.receiver(Service::Commands),
                radio_state.receiver(Service::Commands),
                buttons_state.receiver(Service::Commands),
                commands_state.sender(),
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
