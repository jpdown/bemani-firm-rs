#![no_std]
#![no_main]
#![feature(generic_const_exprs)]

mod button;
mod encoder;
mod rgb;
mod usb;

use defmt::*;
use embassy_executor::{Executor, Spawner};
use embassy_rp::multicore::{Stack, spawn_core1};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use rgb::rgb_task;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

use crate::{
    button::{ButtonGPIO, button_task},
    encoder::encoder_task,
    rgb::RGBButtonPins,
    usb::usb_task,
};

static mut CORE1_STACK: Stack<4096> = Stack::new();
static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();

static BUTTON_SIGNAL: Signal<CriticalSectionRawMutex, u16> = Signal::new();
static ENCODER_SIGNAL: Signal<CriticalSectionRawMutex, u8> = Signal::new();

#[cortex_m_rt::entry]
fn main() -> ! {
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

    let rgb_buttons = RGBButtonPins {
        key_1: p.PIN_20,
        key_2: p.PIN_21,
        key_3: p.PIN_22,
    };

    spawn_core1(
        p.CORE1,
        unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) },
        move || {
            let executor1 = EXECUTOR1.init(Executor::new());
            executor1.run(|spawner| {
                unwrap!(spawner.spawn(rgb_task(
                    p.PIO1,
                    p.PIN_28,
                    rgb_buttons,
                    p.DMA_CH0,
                    p.DMA_CH1
                )));
            });
        },
    );

    let executor0 = EXECUTOR0.init(Executor::new());
    executor0.run(|spawner| {
        unwrap!(spawner.spawn(usb_task(p.USB, &BUTTON_SIGNAL, &ENCODER_SIGNAL)));
        unwrap!(spawner.spawn(button_task(buttons, &BUTTON_SIGNAL)));
        unwrap!(spawner.spawn(encoder_task(p.PIO0, p.PIN_0, p.PIN_1, &ENCODER_SIGNAL)));
    })
}
