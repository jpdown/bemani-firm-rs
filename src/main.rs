#![no_std]
#![no_main]

mod button;
mod encoder;
mod rgb;
mod usb;

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::pac::pio::Pio;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use rgb::rgb_task;
use {defmt_rtt as _, panic_probe as _};

use crate::{
    button::{ButtonGPIO, button_task},
    encoder::encoder_task,
    usb::usb_task,
};

static BUTTON_SIGNAL: Signal<CriticalSectionRawMutex, u16> = Signal::new();
static ENCODER_SIGNAL: Signal<CriticalSectionRawMutex, u8> = Signal::new();

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let buttons = ButtonGPIO {
        key_1: p.PIN_2.into(),
        key_2: p.PIN_3.into(),
        key_3: p.PIN_4.into(),
        key_4: p.PIN_8.into(),
        key_5: p.PIN_5.into(),
        key_6: p.PIN_6.into(),
        key_7: p.PIN_7.into(),

        e_1: p.PIN_13.into(),
        e_2: p.PIN_9.into(),
        e_3: p.PIN_10.into(),
        e_4: p.PIN_11.into(),
    };

    unwrap!(spawner.spawn(usb_task(p.USB, &BUTTON_SIGNAL, &ENCODER_SIGNAL)));
    unwrap!(spawner.spawn(button_task(buttons, &BUTTON_SIGNAL)));
    unwrap!(spawner.spawn(encoder_task(p.PIO0, p.PIN_0, p.PIN_1, &ENCODER_SIGNAL)));
    unwrap!(spawner.spawn(rgb_task(p.PIO1, p.PIN_28, p.DMA_CH0)));
}
