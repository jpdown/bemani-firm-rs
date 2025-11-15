use defmt::debug;
use embassy_rp::{
    Peri,
    gpio::{AnyPin, Input},
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Instant, Ticker};

const DEBOUNCE_TIME: Duration = Duration::from_millis(4);
const POLL_PERIOD: Duration = Duration::from_micros(250);

struct Button<'a> {
    pin: Input<'a>,
    output_index: i16,
    pressed: bool,
    transition_time: Instant,
}

pub struct ButtonGPIO {
    pub key_1: Peri<'static, AnyPin>,
    pub key_2: Peri<'static, AnyPin>,
    pub key_3: Peri<'static, AnyPin>,
    pub key_4: Peri<'static, AnyPin>,
    pub key_5: Peri<'static, AnyPin>,
    pub key_6: Peri<'static, AnyPin>,
    pub key_7: Peri<'static, AnyPin>,

    pub e_1: Peri<'static, AnyPin>,
    pub e_2: Peri<'static, AnyPin>,
    pub e_3: Peri<'static, AnyPin>,
    pub e_4: Peri<'static, AnyPin>,
}

fn new_button<'a>(pin: Peri<'static, AnyPin>, output_index: i16) -> Button<'a> {
    Button {
        pin: Input::new(pin, embassy_rp::gpio::Pull::Up),
        output_index,
        pressed: false,
        transition_time: Instant::from_secs(0),
    }
}

#[embassy_executor::task]
pub async fn button_task(gpio: ButtonGPIO, output: &'static Signal<CriticalSectionRawMutex, u16>) {
    let mut buttons = [
        new_button(gpio.key_1, 0),
        new_button(gpio.key_2, 1),
        new_button(gpio.key_3, 2),
        new_button(gpio.key_4, 3),
        new_button(gpio.key_5, 4),
        new_button(gpio.key_6, 5),
        new_button(gpio.key_7, 6),
        new_button(gpio.e_1, 8),
        new_button(gpio.e_2, 9),
        new_button(gpio.e_3, 10),
        new_button(gpio.e_4, 11),
    ];

    let mut ticker = Ticker::every(POLL_PERIOD);

    loop {
        poll_buttons(&mut buttons);
        let bits = buttons_to_bitstring(buttons.as_slice());
        // debug!("{}", bits);
        output.signal(bits);
        ticker.next().await;
    }
}

fn poll_buttons(b: &mut [Button<'_>]) {
    for button in b {
        let new_pin_state = button.pin.is_low();

        let time = Instant::now();
        let debounce_time_elapsed = time - button.transition_time > DEBOUNCE_TIME;

        if new_pin_state != button.pressed && debounce_time_elapsed {
            button.pressed = new_pin_state;
            button.transition_time = time;
        }
    }
}

fn buttons_to_bitstring(b: &[Button]) -> u16 {
    let mut output: u16 = 0;

    for button in b {
        if button.pressed {
            output |= 1 << button.output_index;
        }
    }

    output
}
