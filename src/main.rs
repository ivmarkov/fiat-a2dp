use std::{cell::RefCell, future::pending};

use edge_executor::Executor;
use embassy_futures::select::select;
use embassy_sync::{blocking_mutex::Mutex, signal::Signal};
use embassy_time::{Duration, Timer};
use esp_idf_svc::{
    bt::{
        a2dp::{A2dpEvent, EspA2dp},
        gap::{DiscoveryMode, EspGap, GapEvent, IOCapabilities},
        BtClassic, BtDriver,
    },
    nvs::EspDefaultNvsPartition,
};
use esp_idf_sys::EspError;

use esp_idf_hal::{
    adc::{AdcContConfig, AdcContDriver, AdcMeasurement, Attenuated},
    delay,
    gpio::AnyIOPin,
    i2s::{
        config::{
            self, ClockSource, Config, DataBitWidth, MclkMultiple, SlotMode, StdClkConfig,
            StdConfig, StdGpioConfig, StdSlotConfig,
        },
        I2sConfig, I2sDriver,
    },
    peripherals::Peripherals,
    task::{
        critical_section::EspCriticalSection, embassy_sync::EspRawMutex, executor::FreeRtosMonitor,
    },
    units::*,
};

use log::*;

use crate::ringbuf::RingBuf;

mod ringbuf;

fn main() -> Result<(), EspError> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_sys::link_patches();
    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    info!("Hello, world!");

    let peripherals = Peripherals::take().unwrap();

    let ringbuf = Mutex::<EspRawMutex, _>::new(RefCell::new(RingBuf::<32768>::new()));
    let notification = Signal::<EspRawMutex, ()>::new();

    let mut adc = AdcContDriver::new(
        peripherals.adc1,
        &AdcContConfig::new().sample_freq(44100.Hz()),
        Attenuated::db6(peripherals.pins.gpio36),
    )?;

    let mut i2s = I2sDriver::new_std_tx(
        peripherals.i2s1,
        &StdConfig::new(
            Config::new().auto_clear(true),
            StdClkConfig::new(44100, ClockSource::Pll160M, MclkMultiple::M256),
            StdSlotConfig::msb_slot_default(DataBitWidth::Bits16, SlotMode::Stereo),
            Default::default(),
        ),
        peripherals.pins.gpio25,
        peripherals.pins.gpio26,
        AnyIOPin::none(),
        peripherals.pins.gpio27,
    )?;

    let nvs = EspDefaultNvsPartition::take()?;

    let bt = BtDriver::<BtClassic>::new(peripherals.modem, Some(nvs))?;

    bt.set_device_name("Fiat")?;

    let gap = EspGap::new(&bt)?;

    let a2dp = EspA2dp::new_sink(&bt)?;

    let gap = &gap;
    let a2dp = &a2dp;
    let ringbuf = &ringbuf;
    let notification = &notification;

    gap.initialize(move |event| match &event {
        GapEvent::PairingUserConfirmationRequest { bd_addr, .. } => {
            gap.reply_ssp_confirm(&bd_addr, true).unwrap();
        }
        _ => (),
    })?;

    gap.set_ssp_io_cap(IOCapabilities::None)?;
    gap.set_pin("1234")?;
    gap.set_scan_mode(true, DiscoveryMode::Discoverable)?;

    a2dp.initialize(move |event| match &event {
        A2dpEvent::SinkData(data) => {
            if !data.is_empty() {
                ringbuf.lock(|ringbuf| ringbuf.borrow_mut().push(data));
                notification.signal(());
            }
        }
        _ => (),
    })?;

    a2dp.set_delay(core::time::Duration::from_millis(150))?;

    let executor = Executor::<64, FreeRtosMonitor>::new();
    let mut tasks = heapless::Vec::<_, 64>::new();

    let mut adc_buf = [AdcMeasurement::INIT; 16 * 8];
    //let mut i2s_buf = [0u8; 1440];
    let mut i2s_buf = [0u8; 4000];

    let adc_buf = &mut adc_buf;
    let i2s_buf = &mut i2s_buf;

    executor
        // .spawn_local_collect(
        //     async move {
        //         adc.start().unwrap();
        //         i2s.tx_enable().unwrap();
        //         loop {
        //             let len = adc.read_async(adc_buf).await.unwrap();
        //             //info!("ADC reading: {:?}", &adc_buf[..len]);
        //             for index in 0..len {
        //                 i2s_buf[index * 2] = (adc_buf[index].data() >> 8) as _;
        //                 i2s_buf[index * 2 + 1] = (adc_buf[index].data() & 0xff) as _;
        //             }
        //             i2s.write_all_async(&i2s_buf[..len]).await.unwrap();
        //         }
        //     },
        //     &mut tasks,
        // )
        .spawn_local_collect(
            async move {
                i2s.tx_enable().unwrap();

                loop {
                    loop {
                        let len = ringbuf.lock(|ringbuf| ringbuf.borrow_mut().pop(i2s_buf));

                        if len > 0 {
                            i2s.write_all_async(&i2s_buf[..len]).await.unwrap();
                        } else {
                            break;
                        }
                    }

                    notification.wait().await;
                }
            },
            &mut tasks,
        )
        .unwrap();

    executor.run_tasks(|| true, tasks);

    Ok(())
}
