use core::cmp::min;

use embassy_futures::select::{select, select3, select_slice, Either, Either3};

use embassy_sync::{
    blocking_mutex::raw::{NoopRawMutex, RawMutex},
    signal::Signal,
};

use embassy_time::{Duration, Timer};

use enumset::EnumSet;

use esp_idf_svc::hal::{
    can::{AsyncCanDriver, CanConfig, Frame, OwnedAsyncCanDriver, CAN},
    gpio::{InputPin, OutputPin},
    peripheral::Peripheral,
};

use crate::{
    bus::{
        bt::{AudioState, BtCommand},
        can::{DisplayText, RadioState},
        BusSubscription,
    },
    select_spawn::SelectSpawn,
    signal::{Receiver, Sender, StatefulReceiver},
};
use crate::{
    error::Error,
    service::{ServiceLifecycle, SystemState},
};

use self::message::{
    BodyComputer, Bt, Display, Message, Proxi, Publisher, RadioSource, SteeringWheel,
    SteeringWheelButton, Topic,
};

pub mod message {
    use core::iter::repeat;

    use enumset::{EnumSet, EnumSetType};

    use esp_idf_svc::hal::can::Frame;

    const UNIT_BODY_COMPUTER: u16 = 0x4000;
    const UNIT_INSTRUMENT_PANEL: u16 = 0x4003;
    const UNIT_RADIO: u16 = 0x4005;
    const UNIT_PARKING_SENSORS: u16 = 0x4018;
    const UNIT_BT: u16 = 0x4021;

    const TOPIC_UNITS_STATUS: u16 = 0xe09;
    const TOPIC_PROXI: u16 = 0x1e11;
    const TOPIC_STEERING_WHEEL: u16 = 0x0635;
    const TOPIC_DATETIME: u16 = 0xc21;
    const TOPIC_DISPLAY: u16 = 0xa39;
    const TOPIC_BT: u16 = 0x631;
    const TOPIC_RADIO_STATION: u16 = 0xa19;
    const TOPIC_RADIO_SOURCE: u16 = 0xa11;

    const CHAR_MAP: &str = "0123456789.ABCDEFGHIJKLMNOPQRSTUVWXYZ%% %ij%%%%%%_%%?@!+-:/#*%;";

    pub type FramePayload = heapless::Vec<u8, 8>;
    pub type DisplayString = heapless::String<12>;

    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    pub enum Publisher {
        BodyComputer,
        InstrumentPanel,
        Radio,
        ParkingSensors,
        Bt,
        Unknown(u16),
    }

    impl From<u16> for Publisher {
        fn from(id: u16) -> Self {
            match id {
                UNIT_BODY_COMPUTER => Publisher::BodyComputer,
                UNIT_INSTRUMENT_PANEL => Publisher::InstrumentPanel,
                UNIT_RADIO => Publisher::Radio,
                UNIT_PARKING_SENSORS => Publisher::ParkingSensors,
                UNIT_BT => Publisher::Bt,
                other => Publisher::Unknown(other),
            }
        }
    }

    impl From<Publisher> for u16 {
        fn from(value: Publisher) -> Self {
            match value {
                Publisher::BodyComputer => UNIT_BODY_COMPUTER,
                Publisher::InstrumentPanel => UNIT_INSTRUMENT_PANEL,
                Publisher::Radio => UNIT_RADIO,
                Publisher::ParkingSensors => UNIT_PARKING_SENSORS,
                Publisher::Bt => UNIT_BT,
                Publisher::Unknown(other) => other,
            }
        }
    }

    pub enum Topic<'a> {
        BodyComputer(BodyComputer<'a>),
        Proxi(Proxi<'a>),
        SteeringWheel(SteeringWheel<'a>),
        DateTime(DateTime<'a>),
        Display(Display<'a>),
        Bt(Bt<'a>),
        RadioStation(RadioStation<'a>),
        RadioSource(RadioSource<'a>),
        Unknown { topic: u16, payload: &'a [u8] },
    }

    impl<'a> From<(u16, &'a [u8])> for Topic<'a> {
        fn from(value: (u16, &'a [u8])) -> Self {
            let payload = value.1;

            match value.0 {
                TOPIC_UNITS_STATUS => Topic::BodyComputer(payload.into()),
                TOPIC_PROXI => Topic::Proxi(payload.into()),
                TOPIC_STEERING_WHEEL => Topic::SteeringWheel(payload.into()),
                TOPIC_DATETIME => Topic::DateTime(payload.into()),
                TOPIC_BT => Topic::Bt(payload.into()),
                TOPIC_DISPLAY => Topic::Display(payload.into()),
                TOPIC_RADIO_STATION => Topic::RadioStation(payload.into()),
                TOPIC_RADIO_SOURCE => Topic::RadioSource(payload.into()),
                other => Topic::Unknown {
                    topic: other,
                    payload: payload,
                },
            }
        }
    }

    impl<'a> From<Topic<'a>> for (u16, FramePayload) {
        fn from(value: Topic<'a>) -> Self {
            match value {
                Topic::BodyComputer(payload) => (TOPIC_UNITS_STATUS, payload.into()),
                Topic::Proxi(payload) => (TOPIC_PROXI, payload.into()),
                Topic::SteeringWheel(payload) => (TOPIC_STEERING_WHEEL, payload.into()),
                Topic::DateTime(payload) => (TOPIC_DATETIME, payload.into()),
                Topic::Bt(payload) => (TOPIC_BT, payload.into()),
                Topic::Display(payload) => (TOPIC_DISPLAY, payload.into()),
                Topic::RadioStation(payload) => (TOPIC_RADIO_STATION, payload.into()),
                Topic::RadioSource(payload) => (TOPIC_RADIO_SOURCE, payload.into()),
                Topic::Unknown { topic, payload } => {
                    (topic, FramePayload::from_slice(payload).unwrap())
                }
            }
        }
    }

    pub struct Message<'a> {
        pub publisher: Publisher,
        pub topic: Topic<'a>,
    }

    impl<'a> From<&'a Frame> for Message<'a> {
        fn from(frame: &'a Frame) -> Self {
            Self {
                publisher: get_publisher(frame.identifier()).into(),
                topic: (get_topic(frame.identifier()), frame.data()).into(),
            }
        }
    }

    impl<'a> From<Message<'a>> for Frame {
        fn from(message: Message<'a>) -> Self {
            let (topic, payload) = message.topic.into();
            Frame::new(get_id(topic, message.publisher.into()), true, &payload).unwrap()
        }
    }

    pub enum BodyComputer<'a> {
        WakeupRequest,
        StatusRequest,
        ShutDownRequest,
        PoweringOn,
        Active,
        AboutToSleep,
        Unknown(&'a [u8]),
    }

    impl<'a> From<&'a [u8]> for BodyComputer<'a> {
        fn from(value: &'a [u8]) -> Self {
            match value {
                &[0x00, 0x1c, 0x00, 0x00, 0x00, 0x01] => Self::WakeupRequest,
                &[0x00, 0x1e, 0x00, 0x00, 0x00, 0x01] => Self::StatusRequest,
                &[0x00, 0x1A, 0x04, 0x00, 0x10, 0x6B] => Self::ShutDownRequest,
                &[0x00, 0x1c] => Self::PoweringOn,
                &[0x00, 0x1e] => Self::Active,
                &[0x00, 0x1a] => Self::AboutToSleep,
                other => Self::Unknown(other),
            }
        }
    }

    impl<'a> From<BodyComputer<'a>> for FramePayload {
        fn from(value: BodyComputer<'a>) -> Self {
            let slice: &[u8] = match value {
                BodyComputer::WakeupRequest => &[0x00, 0x1c, 0x00, 0x00, 0x00, 0x01],
                BodyComputer::StatusRequest => &[0x00, 0x1e, 0x00, 0x00, 0x00, 0x01],
                BodyComputer::ShutDownRequest => &[0x00, 0x1A, 0x04, 0x00, 0x10, 0x6B],
                BodyComputer::PoweringOn => &[0x00, 0x1c],
                BodyComputer::Active => &[0x00, 0x1e],
                BodyComputer::AboutToSleep => &[0x00, 0x1a],
                BodyComputer::Unknown(other) => other,
            };

            FramePayload::from_slice(slice).unwrap()
        }
    }

    pub enum Proxi<'a> {
        Request,
        Response(&'a [u8]),
        Unknown(&'a [u8]),
    }

    impl<'a> From<&'a [u8]> for Proxi<'a> {
        fn from(value: &'a [u8]) -> Self {
            match value {
                &[] => Self::Request,
                value if value.len() == 6 => Self::Response(value),
                other => Self::Unknown(other),
            }
        }
    }

    impl<'a> From<Proxi<'a>> for FramePayload {
        fn from(value: Proxi<'a>) -> Self {
            let slice = match value {
                Proxi::Request => &[],
                Proxi::Response(value) => value,
                Proxi::Unknown(other) => other,
            };

            FramePayload::from_slice(slice).unwrap()
        }
    }

    #[derive(Debug, EnumSetType)]
    #[enumset(repr = "u16")]
    #[repr(u16)]
    pub enum SteeringWheelButton {
        Windows = 7,     // 0x0040
        Menu = 8,        // 0x0080
        Src = 10,        // 0x0400
        Down = 11,       // 0x0800
        Up = 12,         // 0x1000
        Mute = 13,       // 0x2000
        VolumeDown = 14, // 0x4000
        VolumeUp = 15,   // 0x8000
    }

    pub enum SteeringWheel<'a> {
        Buttons(EnumSet<SteeringWheelButton>),
        Unknown(&'a [u8]),
    }

    impl<'a> From<&'a [u8]> for SteeringWheel<'a> {
        fn from(value: &'a [u8]) -> Self {
            match value {
                value if value.len() == 2 => {
                    Self::Buttons(EnumSet::from_repr_truncated(u16::from_be_bytes([
                        value[0], value[1],
                    ])))
                }
                other => Self::Unknown(other),
            }
        }
    }

    impl<'a> From<SteeringWheel<'a>> for FramePayload {
        fn from(value: SteeringWheel<'a>) -> Self {
            match value {
                SteeringWheel::Buttons(buttons) => {
                    FramePayload::from_slice(&buttons.as_repr().to_be_bytes()).unwrap()
                }
                SteeringWheel::Unknown(other) => FramePayload::from_slice(other).unwrap(),
            }
        }
    }

    pub enum DateTime<'a> {
        Current {
            year: u16,
            month: u8,
            day: u8,
            hour: u8,
            minute: u8,
        },
        Unknown(&'a [u8]),
    }

    impl<'a> From<&'a [u8]> for DateTime<'a> {
        fn from(value: &'a [u8]) -> Self {
            match value {
                value if value.len() == 6 => panic!(), //// TODO
                other => Self::Unknown(other),
            }
        }
    }

    impl<'a> From<DateTime<'a>> for FramePayload {
        fn from(value: DateTime<'a>) -> Self {
            let slice: &[u8] = match value {
                DateTime::Current { .. } => &[], // TODO
                DateTime::Unknown(other) => other,
            };

            FramePayload::from_slice(slice).unwrap()
        }
    }

    pub enum Bt<'a> {
        Mute,
        Phone,
        Voice,
        Navigation,
        Media,
        Unknown(&'a [u8]),
    }

    impl<'a> From<&'a [u8]> for Bt<'a> {
        fn from(value: &'a [u8]) -> Self {
            match value {
                &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80] => Self::Mute,
                &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x81] => Self::Phone,
                &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x82] => Self::Voice,
                &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x83] => Self::Navigation,
                &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x84] => Self::Media,
                other => Self::Unknown(other),
            }
        }
    }

    impl<'a> From<Bt<'a>> for FramePayload {
        fn from(value: Bt<'a>) -> Self {
            let slice = match value {
                Bt::Mute => &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80],
                Bt::Phone => &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x81],
                Bt::Voice => &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x82],
                Bt::Navigation => &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x83],
                Bt::Media => &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x84],
                Bt::Unknown(other) => other,
            };

            FramePayload::from_slice(slice).unwrap()
        }
    }

    pub enum Display<'a> {
        Text {
            for_radio: bool,
            menu: bool,
            text: DisplayString,
            chunk: usize,
            total_chunks: usize,
        },
        Unknown(&'a [u8]),
    }

    impl<'a> From<&'a [u8]> for Display<'a> {
        fn from(value: &'a [u8]) -> Self {
            match value {
                value if value.len() == 8 => Self::Text {
                    text: decode_display_text(value),
                    chunk: (value[0] & 0x0f) as _,
                    total_chunks: ((value[0] >> 4) + 1) as _,
                    for_radio: value[1] >> 4 == 2,
                    menu: value[1] & 0x0f == 6,
                },
                other => Self::Unknown(other),
            }
        }
    }

    impl<'a> From<Display<'a>> for FramePayload {
        fn from(value: Display<'a>) -> Self {
            match value {
                Display::Text {
                    for_radio,
                    menu,
                    chunk,
                    total_chunks,
                    text,
                } => {
                    let mut payload = encode_display_text(&text);
                    payload[0] = (((total_chunks - 1) << 4) | chunk) as u8;
                    payload[1] = (((if for_radio { 2 } else { 1 }) << 4)
                        | (if !for_radio && menu { 0x06 } else { 0x0a }))
                        as u8;

                    payload
                }
                Display::Unknown(other) => FramePayload::from_slice(other).unwrap(),
            }
        }
    }

    pub enum RadioStation<'a> {
        Station(DisplayString),
        Unknown(&'a [u8]),
    }

    impl<'a> From<&'a [u8]> for RadioStation<'a> {
        fn from(value: &'a [u8]) -> Self {
            Self::Station(decode_text(value))
        }
    }

    impl<'a> From<RadioStation<'a>> for FramePayload {
        fn from(value: RadioStation<'a>) -> Self {
            match value {
                RadioStation::Station(text) => {
                    let mut payload = FramePayload::new();
                    payload.extend(repeat(0).take(8));

                    encode_text(&text, &mut payload);

                    payload
                }
                RadioStation::Unknown(other) => FramePayload::from_slice(other).unwrap(),
            }
        }
    }

    pub enum RadioSource<'a> {
        Fm(u16),
        BtPlaying,
        BtMuted,
        Unknown(&'a [u8]),
    }

    impl<'a> From<&'a [u8]> for RadioSource<'a> {
        fn from(value: &'a [u8]) -> Self {
            match value {
                &[0xe3, 0x00, 0x00, 0x00, 0x02, 0x00] => Self::BtPlaying,
                &[0xe3, 0x00, 0x00, 0x00, 0x00, 0x00] => Self::BtMuted,
                &[_, _, h, l, 0x00, 0x00] => Self::Fm(u16::from_be_bytes([h, l])),
                other => Self::Unknown(other),
            }
        }
    }

    impl<'a> From<RadioSource<'a>> for FramePayload {
        fn from(value: RadioSource<'a>) -> Self {
            match value {
                RadioSource::BtPlaying => {
                    FramePayload::from_slice(&[0xe3, 0x00, 0x00, 0x00, 0x02, 0x00])
                }
                RadioSource::BtMuted => {
                    FramePayload::from_slice(&[0xe3, 0x00, 0x00, 0x00, 0x00, 0x00])
                }
                RadioSource::Fm(freq) => FramePayload::from_slice(&[
                    freq.to_be_bytes()[0],
                    freq.to_be_bytes()[1],
                    0x00,
                    0x00,
                ]),
                RadioSource::Unknown(other) => FramePayload::from_slice(other),
            }
            .unwrap()
        }
    }

    fn get_id(topic: u16, publisher: u16) -> u32 {
        ((topic as u32) << 16) | (publisher as u32)
    }

    fn get_topic(id: u32) -> u16 {
        (id >> 16) as _
    }

    fn get_publisher(id: u32) -> u16 {
        (id & 0xffff) as _
    }

    fn decode_display_text<'a>(payload: &[u8]) -> DisplayString {
        decode_text(&payload[2..])
    }

    fn decode_text<'a>(payload: &[u8]) -> DisplayString {
        let mut offset = 0;

        let mut string = DisplayString::new();
        while offset < payload.len() << 3 {
            let char_start = offset >> 3;
            let char_end = (offset + 6) >> 3;

            if char_end >= payload.len() {
                break;
            }

            let index_data = if char_start < char_end {
                u16::from_be_bytes([payload[char_start], payload[char_end]])
            } else {
                payload[char_start] as _
            };

            let shift = 8 - (offset + 6) % 8;

            let index = (index_data >> shift) & 0b111111;
            if index == 0 {
                break;
            }

            let _ = string.push(CHAR_MAP.as_bytes()[(index - 1) as usize] as char);

            offset += 6;
        }

        string
    }

    fn encode_display_text(text: &str) -> FramePayload {
        let mut payload = FramePayload::new();
        payload.extend(repeat(0).take(8));

        encode_text(text, &mut payload[2..]);

        payload
    }

    fn encode_text(text: &str, payload: &mut [u8]) {
        let mut offset = 0;

        for ch in payload.iter_mut() {
            *ch = 0;
        }

        for ch in text.chars() {
            let index = CHAR_MAP
                .chars()
                .position(|chm| chm == ch)
                .unwrap_or(CHAR_MAP.chars().position(|chm| chm == ' ').unwrap())
                + 1;

            let char_start = offset >> 3;
            let char_end = (offset + 6) >> 3;

            if char_end >= payload.len() {
                break;
            }

            let shift = 8 - (offset + 6) % 8;

            let index_payload = index << shift;

            if char_start < char_end {
                let [h, l] = u16::to_be_bytes(index_payload as u16);

                payload[char_start] |= h;
                payload[char_end] |= l;
            } else {
                payload[char_start] |= index_payload as u8;
            };

            offset += 6;
        }
    }

    #[test]
    fn test() {
        assert_eq!(
            decode_display_text(&0x101A8177D4610A0E_u64.to_be_bytes()),
            "ULTIME "
        );
        assert_eq!(
            decode_display_text(&0x111A4D43182E8000_u64.to_be_bytes()),
            "HIAM. "
        );
        assert_eq!(
            u64::from_be_bytes(
                encode_display_text(&decode_display_text(&0x101A8177D4610A0E_u64.to_be_bytes()))
                    .into_array()
                    .unwrap()
            ),
            0x00008177d4610a00
        );
        assert_eq!(
            u64::from_be_bytes(encode_display_text("0").into_array().unwrap()),
            0x0000040000000000
        );
        assert_eq!(
            decode_display_text(
                &u64::from_be_bytes(encode_display_text("BLAH ").into_array().unwrap())
                    .to_be_bytes()
            ),
            "BLAH "
        );
    }
}

pub async fn process(
    bus: BusSubscription<'_>,
    mut can: impl Peripheral<P = CAN>,
    mut tx: impl Peripheral<P = impl OutputPin>,
    mut rx: impl Peripheral<P = impl InputPin>,
    radio: Sender<'_, impl RawMutex, RadioState>,
    buttons: Sender<'_, impl RawMutex, EnumSet<SteeringWheelButton>>,
    radio_commands: Sender<'_, impl RawMutex, BtCommand>,
) -> Result<(), Error> {
    loop {
        bus.service.wait_enabled().await?;

        loop {
            bus.service.starting();

            let mut driver = create(&mut can, &mut tx, &mut rx)?;

            let raw_buttons = &Signal::<NoopRawMutex, _>::new();

            let send_radio_switch = &Signal::<NoopRawMutex, _>::new();
            let send_radio_display = &Signal::<NoopRawMutex, _>::new();
            let send_cockpit_display = &Signal::<NoopRawMutex, _>::new();
            let send_proxi = &Signal::<NoopRawMutex, _>::new();
            let send_status = &Signal::<NoopRawMutex, _>::new();

            driver.start()?;

            bus.service.started();

            let res = SelectSpawn::run(bus.service.wait_disabled())
                .chain(process_radio_mux(
                    &bus.audio,
                    &bus.phone,
                    &bus.radio,
                    &radio_commands,
                    send_radio_switch,
                ))
                // .chain(process_display(
                //     &bus.radio_display,
                //     true,
                //     send_radio_display,
                // ))
                // .chain(process_display(
                //     &bus.cockpit_display,
                //     false,
                //     send_cockpit_display,
                // ))
                .chain(process_send(
                    &driver,
                    &[
                        send_radio_switch,
                        send_radio_display,
                        send_cockpit_display,
                        send_proxi,
                        send_status,
                    ],
                ))
                //.chain(process_debounce_buttons(raw_buttons, &buttons))
                .chain(process_recv(
                    &driver,
                    &bus.service,
                    send_status,
                    send_proxi,
                    &radio,
                    raw_buttons,
                ))
                .await;

            driver.stop()?;

            bus.service.stopped();

            res?;
        }
    }
}

fn create<'d>(
    can: impl Peripheral<P = CAN> + 'd,
    tx: impl Peripheral<P = impl OutputPin> + 'd,
    rx: impl Peripheral<P = impl InputPin> + 'd,
) -> Result<OwnedAsyncCanDriver<'d>, Error> {
    Ok(AsyncCanDriver::new(can, tx, rx, &CanConfig::new())?)
}

async fn process_radio_mux(
    audio: &Receiver<'_, impl RawMutex, AudioState>,
    phone: &Receiver<'_, impl RawMutex, AudioState>,
    radio: &Receiver<'_, impl RawMutex, RadioState>,
    radio_commands: &Sender<'_, impl RawMutex, BtCommand>,
    radio_switch_out: &Signal<impl RawMutex, Frame>,
) -> Result<(), Error> {
    let mut sradio = RadioState::Unknown;
    let mut sphone = AudioState::Uninitialized;
    let mut saudio = AudioState::Uninitialized;

    loop {
        let ret = select3(radio.recv(), phone.recv(), audio.recv()).await;

        match ret {
            Either3::First(new) => {
                sradio = new;

                if saudio.is_active() && !sphone.is_active() {
                    match new {
                        RadioState::BtActive => radio_commands.send(BtCommand::Resume),
                        _ => radio_commands.send(BtCommand::Pause),
                    }
                }
            }
            Either3::Second(new) => {
                sphone = new;

                if sphone.is_active() && !sradio.is_bt_active() {
                    radio_switch_out.signal(as_frame(Topic::Bt(Bt::Phone)));
                }

                // TODO: Switch back on phone disconnect
            }
            Either3::Third(new) => saudio = new,
        }
    }
}

async fn process_display(
    text: &StatefulReceiver<'_, impl RawMutex, DisplayText>,
    for_radio: bool,
    display_out: &Signal<impl RawMutex, Frame>,
) -> Result<(), Error> {
    let mut version = None;
    let mut offset = 0;
    let mut processing = false;

    loop {
        select(text.recv(), Timer::after(Duration::from_millis(10))).await;

        text.state(|text| {
            if Some(text.version) != version {
                version = Some(text.version);
                offset = 0;
                processing = true;
            }

            if !display_out.signaled() && processing {
                let menu = text.menu && !for_radio;
                let text = &text.text;

                let chunk_payload = &text[offset..min(offset + 8, text.len())];
                let chunk = offset / 8;
                let total_chunks = if text.is_empty() {
                    1
                } else {
                    text.len() / 8 + (if text.len() % 8 > 0 { 1 } else { 0 })
                } - 1;

                let topic = Topic::Display(Display::Text {
                    for_radio,
                    menu,
                    text: chunk_payload.into(),
                    chunk,
                    total_chunks,
                });

                display_out.signal(as_frame(topic));

                if chunk == total_chunks {
                    processing = false;
                }
            }
        });
    }
}

async fn process_send<'d, const N: usize>(
    driver: &OwnedAsyncCanDriver<'d>,
    frames: &[&Signal<impl RawMutex, Frame>; N],
) -> Result<(), Error> {
    loop {
        let mut array = heapless::Vec::<_, N>::from_iter(frames.iter().map(|signal| signal.wait()));

        let (frame, _) = select_slice(&mut array).await;

        driver.transmit(&frame).await?;
    }
}

async fn process_recv<'d>(
    driver: &OwnedAsyncCanDriver<'d>,
    service: &ServiceLifecycle<'_, impl RawMutex>,
    status_out: &Signal<impl RawMutex, Frame>,
    proxi_out: &Signal<impl RawMutex, Frame>,
    radio: &Sender<'_, impl RawMutex, RadioState>,
    raw_buttons: &Signal<impl RawMutex, EnumSet<SteeringWheelButton>>,
) -> Result<(), Error> {
    let mut pending_proxi_request = false;
    let mut pending_proxi_value = None;

    loop {
        let frame = driver.receive().await?;
        let message: Message<'_> = (&frame).into();

        match message.topic {
            Topic::BodyComputer(payload) => {
                process_recv_body_computer(payload, service, status_out)
            }
            Topic::Proxi(payload) => process_recv_proxi(
                payload,
                &mut pending_proxi_request,
                &mut pending_proxi_value,
                proxi_out,
            ),
            Topic::SteeringWheel(payload) => process_recv_steering_wheel(payload, raw_buttons),
            Topic::RadioSource(payload) => process_recv_radio_source(payload, &radio),
            _ => (),
        }
    }
}

async fn process_debounce_buttons(
    raw_buttons: &Signal<impl RawMutex, EnumSet<SteeringWheelButton>>,
    buttons: &Sender<'_, impl RawMutex, EnumSet<SteeringWheelButton>>,
) -> Result<(), Error> {
    const TICK: Duration = Duration::from_millis(10);

    let mut debouncing = [None; 16];
    let mut debounced_state = EnumSet::EMPTY;
    let mut latest_state = EnumSet::EMPTY;

    loop {
        match select(raw_buttons.wait(), Timer::after(TICK)).await {
            Either::First(new) => {
                for button in EnumSet::ALL {
                    if latest_state.contains(button) != new.contains(button) {
                        let debouncing = &mut debouncing[button as usize];
                        if !debouncing.is_some() {
                            *debouncing = Some(Duration::from_millis(100));
                        }
                    }
                }

                latest_state = new;
            }
            Either::Second(_) => {
                let mut send_buttons = false;

                for button in EnumSet::<SteeringWheelButton>::ALL {
                    let debouncing = &mut debouncing[button as usize];

                    if let Some(duration) = *debouncing {
                        if duration < TICK {
                            if latest_state.contains(button) {
                                debounced_state |= button;
                            } else {
                                debounced_state &= button;
                            }

                            send_buttons = true;
                            *debouncing = None;
                        } else {
                            *debouncing = Some(duration - TICK);
                        }
                    }
                }

                if send_buttons {
                    buttons.send(debounced_state);
                }
            }
        }
    }
}

fn process_recv_steering_wheel(
    payload: SteeringWheel<'_>,
    buttons: &Signal<impl RawMutex, EnumSet<SteeringWheelButton>>,
) {
    match payload {
        SteeringWheel::Buttons(state) => buttons.signal(state),
        _ => (),
    }
}

fn process_recv_proxi(
    payload: Proxi<'_>,
    pending_proxi_request: &mut bool,
    proxi_value: &mut Option<[u8; 8]>,
    proxi_out: &Signal<impl RawMutex, Frame>,
) {
    match payload {
        Proxi::Request => {
            if !*pending_proxi_request {
                *pending_proxi_request = true;
            }
        }
        Proxi::Response(pvr) => {
            if proxi_value.is_none() {
                let mut pv = [0; 8];
                pv.copy_from_slice(pvr);

                *proxi_value = Some(pv);
            }
        }
        _ => (),
    }

    if *pending_proxi_request {
        if let Some(proxi_value) = proxi_value.as_ref() {
            proxi_out.signal(as_frame(Topic::Proxi(Proxi::Response(proxi_value))));
            *pending_proxi_request = false;
        }
    }
}

fn process_recv_body_computer(
    payload: BodyComputer<'_>,
    service: &ServiceLifecycle<'_, impl RawMutex>,
    status_out: &Signal<impl RawMutex, Frame>,
) {
    match payload {
        BodyComputer::WakeupRequest => service.sys_start(),
        BodyComputer::ShutDownRequest => service.sys_stop(),
        BodyComputer::StatusRequest => {
            let state = match service.get_sys_state() {
                SystemState::Stopped => BodyComputer::AboutToSleep,
                SystemState::Starting => BodyComputer::PoweringOn,
                SystemState::Started => BodyComputer::Active,
                SystemState::Stopping => BodyComputer::AboutToSleep,
            };

            status_out.signal(as_frame(Topic::BodyComputer(state)));
        }
        _ => (),
    }
}

fn process_recv_radio_source(
    payload: RadioSource<'_>,
    radio: &Sender<'_, impl RawMutex, RadioState>,
) {
    let state = match payload {
        RadioSource::Fm(_) => RadioState::Fm,
        RadioSource::BtPlaying => RadioState::BtActive,
        RadioSource::BtMuted => RadioState::BtMuted,
        RadioSource::Unknown(_) => RadioState::Unknown,
    };

    radio.send(state);
}

fn as_frame(topic: Topic<'_>) -> Frame {
    let message = Message {
        publisher: Publisher::Bt,
        topic,
    };

    message.into()
}
