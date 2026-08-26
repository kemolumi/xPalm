use std::time::Duration;

use evdev::{
    AbsInfo,
    AbsoluteAxisCode,
    AttributeSet,
    BusType,
    EventSummary,
    FFEffectCode,
    InputEvent,
    InputId,
    KeyCode,
    KeyEvent,
    UinputAbsSetup,
    uinput::VirtualDevice,
};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerButton {
    A,
    B,
    Y,
    X,
    LB,
    RB,
    Back,
    Start,
    Guide,
}

impl ControllerButton {
    pub fn key_code(self) -> KeyCode {
        match self {
            Self::A => KeyCode::BTN_SOUTH,
            Self::B => KeyCode::BTN_EAST,
            Self::Y => KeyCode::BTN_NORTH,
            Self::X => KeyCode::BTN_WEST,
            Self::LB => KeyCode::BTN_TL,
            Self::RB => KeyCode::BTN_TR,
            Self::Back => KeyCode::BTN_SELECT,
            Self::Start => KeyCode::BTN_START,
            Self::Guide => KeyCode::BTN_MODE,
        }
    }
}

pub struct Controller {
    button_batch_tx: mpsc::Sender<InputEvent>,
}

impl Controller {
    pub fn new(polling_rate: Duration) -> Result<Self, std::io::Error> {
        let abs_setup = AbsInfo::new(0, -32768, 32767, 16, 128, 0);
        let abs_x = UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, abs_setup);
        let abs_y = UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, abs_setup);
        let abs_rx = UinputAbsSetup::new(AbsoluteAxisCode::ABS_RX, abs_setup);
        let abs_ry = UinputAbsSetup::new(AbsoluteAxisCode::ABS_RY, abs_setup);

        let trigger_setup = AbsInfo::new(0, 0, 255, 0, 0, 0);
        let abs_z = UinputAbsSetup::new(AbsoluteAxisCode::ABS_Z, trigger_setup);
        let abs_rz = UinputAbsSetup::new(
            AbsoluteAxisCode::ABS_RZ,
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
        keys.insert(ControllerButton::Back.key_code());
        keys.insert(ControllerButton::Start.key_code());
        keys.insert(ControllerButton::Guide.key_code());

        // Impossible to use on a phone, added to make the controller
        // a bit genuine.
        keys.insert(KeyCode::BTN_THUMBL);
        keys.insert(KeyCode::BTN_THUMBR);

        let mut ff_effects = AttributeSet::<FFEffectCode>::new();
        ff_effects.insert(FFEffectCode::FF_RUMBLE);
        ff_effects.insert(FFEffectCode::FF_PERIODIC);
        ff_effects.insert(FFEffectCode::FF_CONSTANT);

        let mut device = VirtualDevice::builder()?
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

        println!(
            "Mounted as {}",
            device.get_syspath().unwrap().as_os_str().to_str().unwrap()
        );

        let (button_batch_tx, button_batch_rx) = mpsc::channel(2048);

        tokio::spawn(
            Controller::event_loop(button_batch_rx, polling_rate, device)
        );

        Ok(Controller { button_batch_tx })
    }

    async fn event_loop(
        mut button_batch_rx: mpsc::Receiver<InputEvent>,
        polling_rate: Duration,
        device: VirtualDevice
    ) {
        let mut stream = device.into_event_stream().unwrap();
        let mut interval = tokio::time::interval(polling_rate);

        loop {
            tokio::select! {
                event = stream.next_event() => {
                    let ev = event.unwrap();
                    match ev.destructure() {
                        EventSummary::UInput(uinput_event, _, _) => {
                            match uinput_event.code() {
                                evdev::UInputCode::UI_FF_UPLOAD => {
                                    let mut upload = stream
                                        .device_mut()
                                        .process_ff_upload(uinput_event)
                                        .unwrap();
                                    upload.set_retval(0);
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
                        other => {
                            println!("{:?}", other);
                        }
                    }
                    interval.tick().await;
                }
                _ = async {
                    interval.tick().await;
                } => {}
            }

            let count = button_batch_rx.len();

            match count {
                0 => {}
                count => {
                    let mut events: Vec<InputEvent> = Vec::with_capacity(
                        button_batch_rx.len()
                    );
                    button_batch_rx.recv_many(&mut events, count).await;
                    stream.device_mut().emit(&events).unwrap();
                }
            }
        }
    }

    pub async fn press(&self, button: ControllerButton) {
        self.button_batch_tx
            .send(*KeyEvent::new(button.key_code(), 1)).await
            .unwrap();
    }

    pub async fn release(&self, button: ControllerButton) {
        self.button_batch_tx
            .send(*KeyEvent::new(button.key_code(), 0)).await
            .unwrap();
    }
}
