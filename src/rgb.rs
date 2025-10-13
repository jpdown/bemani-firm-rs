use defmt::debug;
use embassy_rp::Peri;
use embassy_rp::bind_interrupts;
use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::dma::AnyChannel;
use embassy_rp::dma::Channel;
use embassy_rp::peripherals::DMA_CH0;
use embassy_rp::peripherals::PIN_28;
use embassy_rp::peripherals::PIO1;
use embassy_rp::pio::Common;
use embassy_rp::pio::Config;
use embassy_rp::pio::Direction;
use embassy_rp::pio::FifoJoin;
use embassy_rp::pio::Instance;
use embassy_rp::pio::InterruptHandler;
use embassy_rp::pio::LoadedProgram;
use embassy_rp::pio::PinConfig;
use embassy_rp::pio::Pio;
use embassy_rp::pio::PioPin;
use embassy_rp::pio::ShiftConfig;
use embassy_rp::pio::ShiftDirection;
use embassy_rp::pio::StateMachine;
use embassy_rp::pio::program::pio_asm;
use embassy_rp::pio_programs::ws2812::PioWs2812;
use embassy_rp::pio_programs::ws2812::PioWs2812Program;
use embassy_time::Duration;
use embassy_time::Ticker;
use embassy_time::Timer;
use fixed::types::U24F8;
use fixed_macro::types::U24F8;
use smart_leds::RGB8;
use smart_leds::hsv::Hsv;
use smart_leds::hsv::hsv2rgb;

bind_interrupts!(struct Irqs {
    PIO1_IRQ_0 => InterruptHandler<PIO1>;
});

const HUE_CYCLE_TIME_MS: usize = 1000;
const TICKER_TIME_MS: usize = 10;

#[embassy_executor::task]
pub async fn rgb_task(
    pio: Peri<'static, PIO1>,
    pin_28: Peri<'static, PIN_28>,
    dma: Peri<'static, DMA_CH0>,
) {
    let Pio {
        mut common, sm0, ..
    } = Pio::new(pio, Irqs);

    const NUM_LEDS: usize = 26;
    let mut data = [RGB8::default(); NUM_LEDS];

    let prg = PioWs2812Program::new(&mut common);
    let mut rgb_0 = PioWs2812::new(&mut common, sm0, dma, pin_28, &prg);

    let mut ticker = Ticker::every(Duration::from_millis(10));
    let mut hue = 0;
    loop {
        let mut hsv = Hsv {
            hue,
            sat: 255,
            val: 128,
        };

        for i in 0..NUM_LEDS {
            data[i] = hsv2rgb(hsv);
            hsv.hue += 1;
        }

        hue += 1;

        rgb_0.write(&data).await;

        ticker.next().await;
    }
}
