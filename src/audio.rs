use core::cell::RefCell;

use embassy_sync::{blocking_mutex::Mutex, signal::Signal};

use esp_idf_sys::EspError;

use esp_idf_hal::{
    adc::{AdcContConfig, AdcContDriver, AdcMeasurement, Attenuated, ADC1},
    gpio::{ADCPin, AnyIOPin, InputPin, OutputPin},
    i2s::{
        config::{
            ClockSource, Config, DataBitWidth, MclkMultiple, SlotMode, StdClkConfig, StdConfig,
            StdSlotConfig,
        },
        I2s, I2sDriver, I2sTx, I2S0,
    },
    peripheral::Peripheral,
    task::embassy_sync::EspRawMutex,
    units::*,
};
use log::info;

use crate::ringbuf::RingBuf;

pub struct AudioBuffers<const I: usize, const O: usize> {
    ringbuf_incoming: RingBuf<{ I }>,
    ringbuf_outgoing: RingBuf<{ O }>,
    a2dp: bool,
}

impl<const I: usize, const O: usize> AudioBuffers<I, O> {
    #[inline(always)]
    const fn new(a2dp: bool) -> Self {
        Self {
            ringbuf_incoming: RingBuf::new(),
            ringbuf_outgoing: RingBuf::new(),
            a2dp,
        }
    }

    #[inline(always)]
    fn is_a2dp(&self) -> bool {
        self.a2dp
    }

    #[inline(always)]
    pub fn set_a2dp(&mut self, a2dp: bool) {
        if self.a2dp != a2dp {
            self.a2dp = a2dp;
            self.ringbuf_incoming.clear();
            self.ringbuf_outgoing.clear();
        }
    }

    #[inline(always)]
    fn outgoing(&mut self) -> &mut RingBuf<O> {
        &mut self.ringbuf_outgoing
    }

    #[inline(always)]
    pub fn push_incoming<F>(&mut self, data: &[u8], a2dp: bool, outgoing_notif: F) -> usize
    where
        F: Fn(),
    {
        if self.a2dp == a2dp && !data.is_empty() {
            let len = self.ringbuf_incoming.push(data);

            if self.is_incoming_above_watermark(a2dp) {
                AUDIO_BUFFERS_INCOMING_NOTIF.signal(());
            }

            if self.is_outgoing_above_watermark(a2dp) {
                outgoing_notif();
            }

            len
        } else {
            0
        }
    }

    #[inline(always)]
    fn pop_incoming(&mut self, buf: &mut [u8], a2dp: bool) -> usize {
        if self.is_incoming_above_watermark(a2dp) {
            self.ringbuf_incoming.pop(buf)
        } else {
            0
        }
    }

    #[inline(always)]
    fn push_outgoing(&mut self, data: &[u8], a2dp: bool) -> usize {
        if self.a2dp == a2dp {
            self.ringbuf_outgoing.push(data)
        } else {
            0
        }
    }

    #[inline(always)]
    pub fn pop_outgoing(&mut self, buf: &mut [u8], a2dp: bool) -> usize {
        if self.is_outgoing_above_watermark(a2dp) {
            self.ringbuf_outgoing.pop(buf)
        } else {
            0
        }
    }

    #[inline(always)]
    fn is_incoming_above_watermark(&self, a2dp: bool) -> bool {
        self.a2dp == a2dp
            && self.ringbuf_incoming.len() >= (if a2dp { I / 3 * 2 } else { I / 12 * 2 })
    }

    #[inline(always)]
    fn is_outgoing_above_watermark(&self, a2dp: bool) -> bool {
        self.a2dp == a2dp && !a2dp && self.ringbuf_outgoing.len() >= O / 3 * 2
    }
}

pub static AUDIO_BUFFERS: Mutex<EspRawMutex, RefCell<AudioBuffers<32768, 8192>>> =
    Mutex::new(RefCell::new(AudioBuffers::new(true)));

static AUDIO_BUFFERS_INCOMING_NOTIF: Signal<EspRawMutex, ()> = Signal::new();

pub async fn process_outgoing<'d, O>(
    adc1: impl Peripheral<P = ADC1> + 'd,
    pin: impl Peripheral<P = impl ADCPin<Adc = ADC1>> + 'd,
    i2s0: impl Peripheral<P = I2S0> + 'd,
    adc_buf: &mut [AdcMeasurement],
    notify_outgoing: O,
) -> Result<(), EspError>
where
    O: Fn(),
{
    info!("Creating ADC 22kHz input");

    let mut adc_driver = AdcContDriver::new(
        adc1,
        i2s0,
        &AdcContConfig::new()
            .sample_freq(20000.Hz())
            .frame_measurements(500)
            .frames_count(4),
        Attenuated::db11(pin),
    )?;

    adc_driver.start()?;

    loop {
        let len = adc_driver.read_async(adc_buf).await?;

        if len > 0 {
            if false {
                let adc_buf = AdcMeasurement::as_pcm16(&mut adc_buf[..len]);

                for src_offset in (0..len).step_by(2) {
                    let dst_offset = src_offset;
                    adc_buf[dst_offset] = adc_buf[src_offset] + adc_buf[src_offset + 1];
                    adc_buf[dst_offset + 1] = adc_buf[dst_offset];
                }

                if len > 0 {
                    AUDIO_BUFFERS.lock(|buffers| {
                        let mut buffers = buffers.borrow_mut();

                        buffers.push_outgoing(as_u8_slice(&adc_buf[..(len >> 2)]), false);

                        notify_outgoing();
                    });
                }
            } else {
                AUDIO_BUFFERS.lock(|buffers| {
                    if !buffers.borrow().is_a2dp() {
                        let mut buffers = buffers.borrow_mut();
                        let outgoing = buffers.outgoing();

                        for src_offset in (0..len).step_by(2) {
                            let sample =
                                adc_buf[src_offset].data() + adc_buf[src_offset + 1].data();

                            let ls = (sample & 0xff) as u8;
                            let ms = (sample >> 8) as u8;

                            outgoing.push_byte(ls);
                            outgoing.push_byte(ms);
                            outgoing.push_byte(ls);
                            outgoing.push_byte(ms);
                        }

                        notify_outgoing();
                    }
                });
            }
        }
    }
}

pub async fn process_incoming(
    mut i2s: impl Peripheral<P = impl I2s>,
    mut bclk: impl Peripheral<P = impl InputPin + OutputPin>,
    mut dout: impl Peripheral<P = impl OutputPin>,
    mut ws: impl Peripheral<P = impl InputPin + OutputPin>,
    buf: &mut [u8],
) -> Result<(), EspError> {
    let mut a2dp_conf = AUDIO_BUFFERS.lock(|buffers| buffers.borrow().is_a2dp());

    loop {
        info!("Creating I2S output with A2DP: {}", a2dp_conf);

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
            } else {
                AUDIO_BUFFERS_INCOMING_NOTIF.wait().await;
            }
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
                if a2dp { 44100 } else { 8000 },
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

fn as_u8_slice(slice: &[u16]) -> &[u8] {
    unsafe { core::slice::from_raw_parts(slice.as_ptr() as *const _, slice.len() * 2) }
}
