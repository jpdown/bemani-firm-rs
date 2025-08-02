use defmt::debug;
use embassy_rp::{Peripherals, gpio::Input};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Instant, Ticker};

const DEBOUNCE_TIME: Duration = Duration::from_millis(4);
const POLL_PERIOD: Duration = Duration::from_micros(250);

pub struct Button<'a> {
    pin: Input<'a>,
    output_index: i16,
    pressed: bool,
    transition_time: Instant,
}

fn new_button<'a>(pin: Input<'a>, output_index: i16) -> Button<'a> {
    Button {
        pin: pin,
        output_index: output_index,
        pressed: false,
        transition_time: Instant::from_secs(0),
    }
}

#[embassy_executor::task]
pub async fn button_task(p: Peripherals, output: &'static Signal<CriticalSectionRawMutex, u16>) {
    let mut buttons = [
        new_button(Input::new(p.PIN_2, embassy_rp::gpio::Pull::Up), 0),
        new_button(Input::new(p.PIN_3, embassy_rp::gpio::Pull::Up), 1),
        new_button(Input::new(p.PIN_4, embassy_rp::gpio::Pull::Up), 2),
        new_button(Input::new(p.PIN_5, embassy_rp::gpio::Pull::Up), 4),
        new_button(Input::new(p.PIN_6, embassy_rp::gpio::Pull::Up), 5),
        new_button(Input::new(p.PIN_7, embassy_rp::gpio::Pull::Up), 6),
        new_button(Input::new(p.PIN_8, embassy_rp::gpio::Pull::Up), 3),
        new_button(Input::new(p.PIN_9, embassy_rp::gpio::Pull::Up), 9),
        new_button(Input::new(p.PIN_10, embassy_rp::gpio::Pull::Up), 10),
        new_button(Input::new(p.PIN_11, embassy_rp::gpio::Pull::Up), 11),
        new_button(Input::new(p.PIN_13, embassy_rp::gpio::Pull::Up), 8),
    ];

    let mut ticker = Ticker::every(POLL_PERIOD);

    loop {
        poll_buttons(&mut buttons);
        let bits = buttons_to_bitstring(buttons.as_slice());
        output.signal(bits);
        debug!("buttons state {:b}", bits);
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
