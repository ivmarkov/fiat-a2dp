#![feature(type_alias_impl_trait)]

use core::cell::RefCell;

use edge_executor::Executor;
use embassy_sync::{blocking_mutex::Mutex, signal::Signal};
use esp_idf_svc::{
    bt::{
        a2dp::{A2dpEvent, EspA2dp},
        gap::{
            Cod, CodMode, DeviceProp, DiscoveryMode, EspGap, GapEvent, IOCapabilities, InqMode,
            PropData,
        },
        hfp::client::{EspHfpc, HfpcEvent, Source},
        BtClassic, BtDriver,
    },
    nvs::EspDefaultNvsPartition,
};
use esp_idf_sys::{
    esp, esp_bt_cod_major_dev_t_ESP_BT_COD_MAJOR_DEV_AV, esp_bt_cod_mode_t_ESP_BT_INIT_COD,
    esp_bt_cod_srvc_t_ESP_BT_COD_SRVC_AUDIO, esp_bt_cod_srvc_t_ESP_BT_COD_SRVC_TELEPHONY,
    esp_bt_gap_set_cod, EspError,
};

use esp_idf_hal::{
    adc::{AdcContConfig, AdcContDriver, AdcMeasurement, Attenuated},
    gpio::{AnyIOPin, InputPin, OutputPin},
    i2s::{
        config::{
            ClockSource, Config, DataBitWidth, MclkMultiple, SlotMode, StdClkConfig, StdConfig,
            StdSlotConfig,
        },
        I2s, I2sDriver, I2sTx,
    },
    peripheral::Peripheral,
    peripherals::Peripherals,
    task::{embassy_sync::EspRawMutex, executor::FreeRtosMonitor},
    units::*,
};

use log::*;

use static_cell::make_static;

use crate::ringbuf::RingBuf;

mod ringbuf;

struct AudioBuffers<const I: usize, const O: usize> {
    ringbuf_incoming: RingBuf<{ I }>,
    ringbuf_outgoing: RingBuf<{ O }>,
    a2dp: bool,
}

impl<const I: usize, const O: usize> AudioBuffers<I, O> {
    pub const fn new(a2dp: bool) -> Self {
        Self {
            ringbuf_incoming: RingBuf::new(),
            ringbuf_outgoing: RingBuf::new2(),
            a2dp,
        }
    }

    pub fn is_a2dp(&self) -> bool {
        self.a2dp
    }

    pub fn set_a2dp(&mut self, a2dp: bool) {
        if self.a2dp != a2dp {
            self.a2dp = a2dp;
            self.ringbuf_incoming.clear();
            self.ringbuf_outgoing.clear();
        }
    }

    pub fn push_incoming<F>(
        &mut self,
        data: &[u8],
        a2dp: bool,
        notif: &Signal<EspRawMutex, ()>,
        outgoing_notif: F,
    ) -> usize
    where
        F: Fn(),
    {
        if self.a2dp == a2dp && !data.is_empty() {
            let len = self.ringbuf_incoming.push(data);

            if len > 0 {
                notif.signal(());
            }

            if self.ringbuf_outgoing.len() > 100 {
                outgoing_notif();
            }

            len
        } else {
            0
        }
    }

    pub fn pop_incoming(&mut self, buf: &mut [u8], a2dp: bool) -> usize {
        if self.a2dp == a2dp && !buf.is_empty() {
            self.ringbuf_incoming.pop(buf)
        } else {
            0
        }
    }

    pub fn push_outgoing(&mut self, data: &[u8], a2dp: bool) -> usize {
        if self.a2dp == a2dp {
            self.ringbuf_outgoing.push(data)
        } else {
            0
        }
    }

    pub fn pop_outgoing(&mut self, buf: &mut [u8], a2dp: bool) -> usize {
        if self.a2dp == a2dp {
            self.ringbuf_outgoing.pop(buf)
        } else {
            0
        }
    }
}

static AUDIO_BUFFERS: Mutex<EspRawMutex, RefCell<AudioBuffers<32768, 4000>>> =
    Mutex::new(RefCell::new(AudioBuffers::new(false)));
static AUDIO_BUFFERS_INCOMING_NOTIF: Signal<EspRawMutex, ()> = Signal::new();

fn main() -> Result<(), EspError> {
    esp_idf_sys::link_patches();

    esp_idf_svc::log::EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();

    let adc_driver = AdcContDriver::new(
        peripherals.adc1,
        peripherals.i2s0,
        &AdcContConfig::new().sample_freq(20000.Hz()),
        Attenuated::db6(peripherals.pins.gpio32),
    )?;

    let mut i2s = peripherals.i2s1;
    let mut i2s_bclk = peripherals.pins.gpio25;
    let mut i2s_dout = peripherals.pins.gpio26;
    let mut i2s_ws = peripherals.pins.gpio27;

    let nvs = EspDefaultNvsPartition::take()?;

    let bt = BtDriver::<BtClassic>::new(peripherals.modem, Some(nvs))?;

    bt.set_device_name("Fiat")?;

    let gap = EspGap::new(&bt)?;

    // TODO
    esp!(unsafe {
        esp_bt_gap_set_cod(
            Cod::new(
                esp_bt_cod_major_dev_t_ESP_BT_COD_MAJOR_DEV_AV,
                0,
                esp_bt_cod_srvc_t_ESP_BT_COD_SRVC_AUDIO
                    | esp_bt_cod_srvc_t_ESP_BT_COD_SRVC_TELEPHONY,
            )
            .raw(),
            esp_bt_cod_mode_t_ESP_BT_INIT_COD,
        )
    })?;

    //gap.set_cod(Cod::new(), CodMode::SetAll)?;

    let a2dp = EspA2dp::new_sink(&bt)?;

    info!("GAP and A2DP new");

    let hfpc = EspHfpc::new(
        &bt,
        Some(Source {
            sample_rate_hz: 20000,
            bits_per_sample: 12,
            stereo: false,
        }),
    )?;

    info!("HPFC new");

    let gap = &gap;
    let a2dp = &a2dp;

    gap.initialize(move |event| match &event {
        GapEvent::DeviceDiscovered { bd_addr, props } => {
            info!("Found device: {:?}", bd_addr);

            for prop in *props {
                info!("Prop: {:?}", prop.prop());
            }

            //let _ = gap.stop_discovery();
        }
        GapEvent::PairingUserConfirmationRequest { bd_addr, .. } => {
            gap.reply_ssp_confirm(&bd_addr, true).unwrap();
        }
        _ => (),
    })?;

    gap.set_ssp_io_cap(IOCapabilities::None)?;
    gap.set_pin("1234")?;
    gap.set_scan_mode(true, DiscoveryMode::Discoverable)?;

    info!("GAP initialized");

    a2dp.initialize(move |event| match &event {
        A2dpEvent::SinkData(data) => {
            AUDIO_BUFFERS.lock(|buffers| {
                let mut buffers = buffers.borrow_mut();

                buffers.push_incoming(data, true, &AUDIO_BUFFERS_INCOMING_NOTIF, || {});
            });
        }
        _ => (),
    })?;

    info!("A2DP initialized");

    hfpc.initialize(|event| match event {
        HfpcEvent::IncomingCall => {
            hfpc.answer().unwrap();

            0
        }
        HfpcEvent::RecvData(data) => {
            AUDIO_BUFFERS.lock(|buffers| {
                let mut buffers = buffers.borrow_mut();

                buffers.push_incoming(data, false, &AUDIO_BUFFERS_INCOMING_NOTIF, || {
                    hfpc.request_outgoing_data_ready();
                });
            });

            0
        }
        HfpcEvent::SendData(data) => AUDIO_BUFFERS.lock(|buffers| {
            let mut buffers = buffers.borrow_mut();

            buffers.pop_outgoing(data, false)
        }),
        _ => 0,
    })?;

    info!("HPFC initialized");

    a2dp.set_delay(core::time::Duration::from_millis(150))?;

    //gap.start_discovery(InqMode::General, 10, 0)?;

    let executor = Executor::<64, FreeRtosMonitor>::new();
    let mut tasks = heapless::Vec::<_, 64>::new();

    let adc_buf = make_static!([AdcMeasurement::INIT; 2000]);
    let adc_resample_buf = make_static!([0u8; 4000]);

    let i2s_buf = make_static!([0u8; 4000]);

    executor
        .spawn_local_collect(
            async move {
                adc_process(adc_driver, adc_buf, adc_resample_buf, |data, out_buf| {
                    hfpc.pcm_resample(data, out_buf)
                })
                .await
                .unwrap();
            },
            &mut tasks,
        )
        .unwrap()
        .spawn_local_collect(
            async move {
                i2s_process(&mut i2s, &mut i2s_bclk, &mut i2s_dout, &mut i2s_ws, i2s_buf)
                    .await
                    .unwrap();
            },
            &mut tasks,
        )
        .unwrap();

    executor.run_tasks(|| true, tasks);

    Ok(())
}

async fn adc_process<F>(
    mut adc: AdcContDriver<'_>,
    adc_buf: &mut [AdcMeasurement],
    adc_resample_buf: &mut [u8],
    resample: F,
) -> Result<(), EspError>
where
    F: Fn(&[u8], &mut [u8]) -> Result<usize, EspError>,
{
    adc.start()?;

    loop {
        let len = adc.read_async(adc_buf).await?;
        //info!("MAIN: {:?}", &adc_buf[..len]);

        let adc_buf = AdcMeasurement::as_pcm(&mut adc_buf[..len]);
        let len = resample(adc_buf, adc_resample_buf)?;

        if len > 0 {
            AUDIO_BUFFERS.lock(|buffers| {
                let mut buffers = buffers.borrow_mut();

                buffers.push_outgoing(&adc_resample_buf[..len], false);
            });
        }
    }
}

async fn i2s_process(
    mut i2s: impl Peripheral<P = impl I2s>,
    mut bclk: impl Peripheral<P = impl InputPin + OutputPin>,
    mut dout: impl Peripheral<P = impl OutputPin>,
    mut ws: impl Peripheral<P = impl InputPin + OutputPin>,
    buf: &mut [u8],
) -> Result<(), EspError> {
    let mut a2dp_conf = false;

    loop {
        let mut driver = i2s_create(&mut i2s, &mut bclk, &mut dout, &mut ws, a2dp_conf)?;

        loop {
            let (len, a2dp) = AUDIO_BUFFERS.lock(|buffers| {
                let mut buffers = buffers.borrow_mut();
                let a2dp = buffers.a2dp;

                if a2dp_conf == a2dp {
                    let len = buffers.pop_incoming(buf, a2dp);

                    (len, a2dp)
                } else {
                    (0, a2dp)
                }
            });

            if a2dp_conf != a2dp {
                a2dp_conf = a2dp;
                break;
            } else if len > 0 {
                driver.write_all_async(&buf[..len]).await?;
            }

            AUDIO_BUFFERS_INCOMING_NOTIF.wait().await;
        }
    }
}

fn i2s_create<'a>(
    i2s: impl Peripheral<P = impl I2s> + 'a,
    bclk: impl Peripheral<P = impl InputPin + OutputPin> + 'a,
    dout: impl Peripheral<P = impl OutputPin> + 'a,
    ws: impl Peripheral<P = impl InputPin + OutputPin> + 'a,
    a2dp: bool,
) -> Result<I2sDriver<'a, I2sTx>, EspError> {
    let mut driver = I2sDriver::new_std_tx(
        i2s,
        &StdConfig::new(
            Config::new().auto_clear(true),
            StdClkConfig::new(
                if a2dp { 44100 } else { 8192 },
                ClockSource::Pll160M,
                MclkMultiple::M256,
            ),
            StdSlotConfig::msb_slot_default(DataBitWidth::Bits16, SlotMode::Stereo),
            Default::default(),
        ),
        bclk,
        dout,
        AnyIOPin::none(),
        ws,
    )?;

    driver.tx_enable()?;

    Ok(driver)
}
