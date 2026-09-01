use std::time::Duration;

use evdev::{
    AbsInfo,
    AbsoluteAxisCode,
    AbsoluteAxisEvent,
    AttributeSet,
    BusType,
    EventSummary,
    FFEffectCode,
    FFEffectKind,
    InputEvent,
    InputId,
    KeyCode,
    KeyEvent,
    UinputAbsSetup,
    uinput::VirtualDevice,
};
use strum::FromRepr;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub enum ControllerButton {
    A,
    B,
    Y,
    X,
    LB,
    RB,
    BL,
    BR,
    Start,
    Back,
    Guide,
}

impl ControllerButton {
    pub fn key_code(self) -> KeyCode {
        match self {
            Self::A => KeyCode::BTN_SOUTH,
            Self::B => KeyCode::BTN_EAST,
            Self::Y => KeyCode::BTN_WEST,
            Self::X => KeyCode::BTN_NORTH,
            Self::LB => KeyCode::BTN_TL,
            Self::RB => KeyCode::BTN_TR,
            Self::BL => KeyCode::BTN_THUMBL,
            Self::BR => KeyCode::BTN_THUMBR,
            Self::Start => KeyCode::BTN_START,
            Self::Back => KeyCode::BTN_SELECT,
            Self::Guide => KeyCode::BTN_MODE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub enum ControllerTrigger {
    Left,
    Right,
}

impl ControllerTrigger {
    pub fn key_code(self) -> AbsoluteAxisCode {
        match self {
            Self::Left => AbsoluteAxisCode::ABS_Z,
            Self::Right => AbsoluteAxisCode::ABS_RZ,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub enum ControllerDpad {
    UpDown,
    LeftRight,
}

impl ControllerDpad {
    pub fn key_code(self) -> AbsoluteAxisCode {
        match self {
            Self::UpDown => AbsoluteAxisCode::ABS_HAT0Y,
            Self::LeftRight => AbsoluteAxisCode::ABS_HAT0X,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, FromRepr)]
#[repr(u8)]
pub enum ControllerJoystick {
    Left,
    Right,
}

impl ControllerJoystick {
    pub fn key_code(self) -> (AbsoluteAxisCode, AbsoluteAxisCode) {
        match self {
            Self::Left => (AbsoluteAxisCode::ABS_X, AbsoluteAxisCode::ABS_Y),
            Self::Right => (AbsoluteAxisCode::ABS_RX, AbsoluteAxisCode::ABS_RY),
        }
    }
}

pub struct Controller {
    button_batch_tx: mpsc::Sender<InputEvent>,
}

impl Controller {
    pub fn new(
        polling_rate: Duration,
        vibration_tx: mpsc::Sender<(u8, u16)>
    ) -> Result<Self, std::io::Error> {
        let abs_setup = AbsInfo::new(0, -32768, 32767, 16, 128, 0);
        let abs_x = UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, abs_setup);
        let abs_y = UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, abs_setup);
        let abs_rx = UinputAbsSetup::new(AbsoluteAxisCode::ABS_RX, abs_setup);
        let abs_ry = UinputAbsSetup::new(AbsoluteAxisCode::ABS_RY, abs_setup);

        let trigger_setup = AbsInfo::new(0, 0, 255, 0, 0, 0);
        let abs_z = UinputAbsSetup::new(
            ControllerTrigger::Left.key_code(),
            trigger_setup
        );
        let abs_rz = UinputAbsSetup::new(
            ControllerTrigger::Right.key_code(),
            trigger_setup
        );

        let hat_setup = AbsInfo::new(0, -1, 1, 0, 0, 0);
        let abs_hat0x = UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_HAT0X,
            hat_setup
        );
        let abs_hat0y = UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_HAT0Y,
            hat_setup
        );

        let mut keys = AttributeSet::<KeyCode>::new();
        keys.insert(ControllerButton::A.key_code());
        keys.insert(ControllerButton::B.key_code());
        keys.insert(ControllerButton::Y.key_code());
        keys.insert(ControllerButton::X.key_code());
        keys.insert(ControllerButton::LB.key_code());
        keys.insert(ControllerButton::RB.key_code());
        keys.insert(ControllerButton::BL.key_code());
        keys.insert(ControllerButton::BR.key_code());
        keys.insert(ControllerButton::Back.key_code());
        keys.insert(ControllerButton::Start.key_code());
        keys.insert(ControllerButton::Guide.key_code());

        let mut ff_effects = AttributeSet::<FFEffectCode>::new();
        ff_effects.insert(FFEffectCode::FF_RUMBLE);

        let device = VirtualDevice::builder()?
            .input_id(InputId::new(BusType::BUS_USB, 0x045e, 0x028e, 0x0114))
            .name("XPalm Controller")
            .with_absolute_axis(&abs_x)?
            .with_absolute_axis(&abs_y)?
            .with_absolute_axis(&abs_rx)?
            .with_absolute_axis(&abs_ry)?
            .with_absolute_axis(&abs_z)?
            .with_absolute_axis(&abs_rz)?
            .with_absolute_axis(&abs_hat0x)?
            .with_absolute_axis(&abs_hat0y)?
            .with_keys(&keys)?
            .with_ff(&ff_effects)?
            .with_ff_effects_max(16)
            .build()
            .unwrap();

        let (button_batch_tx, button_batch_rx) = mpsc::channel(2048);

        tokio::spawn(
            Controller::event_loop(
                button_batch_rx,
                vibration_tx,
                polling_rate,
                device
            )
        );

        Ok(Controller { button_batch_tx })
    }

    async fn event_loop(
        mut button_batch_rx: mpsc::Receiver<InputEvent>,
        vibration_tx: mpsc::Sender<(u8, u16)>,
        polling_rate: Duration,
        device: VirtualDevice
    ) {
        let mut stream = device.into_event_stream().unwrap();
        let mut interval = tokio::time::interval(polling_rate);

        loop {
            let mut vibration = None;

            tokio::select! {
                event = stream.next_event() => {
                    if let EventSummary::UInput(uinput_event, _, _) = event.unwrap().destructure() {
                        match uinput_event.code() {
                            evdev::UInputCode::UI_FF_UPLOAD => {
                                let mut upload = stream
                                    .device_mut()
                                    .process_ff_upload(uinput_event)
                                    .unwrap();
                                upload.set_retval(0);

                                let effect = upload.effect();

                                if let FFEffectKind::Rumble { strong_magnitude, weak_magnitude } = effect.kind {
                                    let average = (strong_magnitude as f32 + weak_magnitude as f32) / 2.0;
                                    vibration = Some(((average / 65535.0 * 255.0) as u8, effect.replay.length / 5));
                                }
                            }
                            evdev::UInputCode::UI_FF_ERASE => {
                                let mut erase = stream
                                    .device_mut()
                                    .process_ff_erase(uinput_event)
                                    .unwrap();
                                erase.set_retval(0);
                            }
                            _ => {}
                        }
                    }
                    interval.tick().await;
                }
                _ = interval.tick() => {}
            }

            if button_batch_rx.is_closed() {
                break;
            }

            match vibration {
                None => {}
                Some(vibration) => {
                    if vibration_tx.send(vibration).await.is_err() {
                        break;
                    }
                }
            }

            let count = button_batch_rx.len();

            if count != 0 {
                let mut events: Vec<InputEvent> = Vec::with_capacity(count);
                button_batch_rx.recv_many(&mut events, count).await;
                stream.device_mut().emit(&events).unwrap();
            }
        }
    }

    pub async fn button(&self, button: ControllerButton, state: u8) {
        self.button_batch_tx
            .send(*KeyEvent::new(button.key_code(), state as i32)).await
            .unwrap();
    }

    pub async fn trigger(&self, trigger: ControllerTrigger, power: u8) {
        self.button_batch_tx
            .send(
                *AbsoluteAxisEvent::new(trigger.key_code(), power as i32)
            ).await
            .unwrap();
    }

    pub async fn dpad(&self, dpad: ControllerDpad, direction: u8) {
        self.button_batch_tx
            .send(
                *AbsoluteAxisEvent::new(dpad.key_code(), direction as i32)
            ).await
            .unwrap();
    }

    pub async fn joystick(&self, joystick: ControllerJoystick, x: i16, y: i16) {
        self.button_batch_tx
            .send(
                *AbsoluteAxisEvent::new(joystick.key_code().0, x as i32)
            ).await
            .unwrap();
        self.button_batch_tx
            .send(
                *AbsoluteAxisEvent::new(joystick.key_code().1, y as i32)
            ).await
            .unwrap();
    }
}
