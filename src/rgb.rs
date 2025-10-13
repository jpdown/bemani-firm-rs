use core::array::from_fn;

use embassy_rp::Peri;
use embassy_rp::bind_interrupts;
use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::dma::AnyChannel;
use embassy_rp::dma::Channel;
use embassy_rp::peripherals::DMA_CH0;
use embassy_rp::peripherals::DMA_CH1;
use embassy_rp::peripherals::PIN_20;
use embassy_rp::peripherals::PIN_21;
use embassy_rp::peripherals::PIN_22;
use embassy_rp::peripherals::PIN_28;
use embassy_rp::peripherals::PIO1;
use embassy_rp::pio::Common;
use embassy_rp::pio::Config;
use embassy_rp::pio::Direction;
use embassy_rp::pio::FifoJoin;
use embassy_rp::pio::Instance;
use embassy_rp::pio::InterruptHandler;
use embassy_rp::pio::LoadedProgram;
use embassy_rp::pio::Pin;
use embassy_rp::pio::Pio;
use embassy_rp::pio::ShiftConfig;
use embassy_rp::pio::ShiftDirection;
use embassy_rp::pio::StateMachine;
use embassy_rp::pio::program::pio_asm;
use embassy_rp::pio_programs::ws2812::PioWs2812;
use embassy_rp::pio_programs::ws2812::PioWs2812Program;
use embassy_time::Duration;
use embassy_time::Ticker;
use embassy_time::Timer;
use fixed::traits::ToFixed;
use smart_leds::RGB8;
use smart_leds::hsv::Hsv;
use smart_leds::hsv::hsv2rgb;

bind_interrupts!(struct Irqs {
    PIO1_IRQ_0 => InterruptHandler<PIO1>;
});

const HUE_CYCLE_TIME_MS: u64 = 1000;
const TICKER_TIME_MS: u64 = HUE_CYCLE_TIME_MS / 256;
const T1: u8 = 2; // start bit
const T2: u8 = 5; // data bit
const T3: u8 = 3; // stop bit
const CYCLES_PER_BIT: u32 = (T1 + T2 + T3) as u32;

const NUM_LED_BITS: usize = 24;
const NUM_LEDS_PER_BUTTON: usize = 1;

pub struct ParallelWs2812Program<'a, PIO: Instance> {
    prg: LoadedProgram<'a, PIO>,
}

impl<'a, PIO: Instance> ParallelWs2812Program<'a, PIO> {
    pub fn new(common: &mut Common<'a, PIO>) -> Self {
        let prg = pio_asm!(
            ".define public T1 3",
            ".define public T2 3",
            ".define public T3 4",
            ".wrap_target",
            "    out x, 32",
            "    mov pins, !null [T1-1]",
            "    mov pins, x     [T2-1]",
            "    mov pins, null  [T3-2]",
            ".wrap"
        );

        let prg = common.load_program(&prg.program);

        Self { prg }
    }
}

pub struct ParallelWs2812<'d, T: Instance, const SM: usize, const NUM_STRIPS: usize> {
    dma: Peri<'d, AnyChannel>,
    sm: StateMachine<'d, T, SM>,
}

impl<'d, T: Instance, const SM: usize, const NUM_STRIPS: usize>
    ParallelWs2812<'d, T, SM, NUM_STRIPS>
{
    pub fn new(
        pio: &mut Common<'d, T>,
        mut sm: StateMachine<'d, T, SM>,
        pins: [Pin<'d, T>; NUM_STRIPS],
        dma: Peri<'d, impl Channel>,
        program: &ParallelWs2812Program<'d, T>,
    ) -> Self {
        let borrowed_pio_pins: [&Pin<'d, T>; NUM_STRIPS] = from_fn(|i| &pins[i]);

        sm.set_pin_dirs(Direction::Out, &borrowed_pio_pins);

        let mut cfg = Config::default();
        cfg.set_out_pins(&borrowed_pio_pins);
        cfg.set_set_pins(&borrowed_pio_pins);

        cfg.use_program(&program.prg, &[]);

        // Clock config, measured in kHz to avoid overflows
        let clock_freq = clk_sys_freq();
        let ws2812_freq = 800000;
        let bit_freq = ws2812_freq * CYCLES_PER_BIT;
        cfg.clock_divider = (clock_freq / bit_freq).to_fixed();

        cfg.fifo_join = FifoJoin::TxOnly;
        cfg.shift_out = ShiftConfig {
            auto_fill: true,
            threshold: 32,
            direction: ShiftDirection::Right,
        };

        sm.set_config(&cfg);
        sm.set_enable(true);

        Self {
            dma: dma.into(),
            sm,
        }
    }

    pub async fn write(&mut self, colours: &[[RGB8; NUM_LEDS_PER_BUTTON]; NUM_STRIPS])
    where
        [(); NUM_LEDS_PER_BUTTON * NUM_LED_BITS]:,
    {
        const BITS_PER_COLOUR: usize = 8;

        // Precompute the word bytes from the colors
        let mut words = [0u32; NUM_LEDS_PER_BUTTON * NUM_LED_BITS];
        for strip in 0..NUM_STRIPS {
            for led in 0..NUM_LEDS_PER_BUTTON {
                for bit in 0..NUM_LED_BITS {
                    let colour = if bit < BITS_PER_COLOUR {
                        colours[strip][led].g
                    } else if bit < (2 * BITS_PER_COLOUR) {
                        colours[strip][led].r
                    } else {
                        colours[strip][led].b
                    };

                    let colour_bit_index = bit % BITS_PER_COLOUR;

                    // We want MSB first
                    let colour_bit = (colour >> (NUM_LED_BITS - colour_bit_index - 1)) & 0b1;
                    words[(led * NUM_LED_BITS) + bit] |= (colour_bit as u32) << strip;
                }
            }
        }

        // DMA transfer
        self.sm
            .tx()
            .dma_push(self.dma.reborrow(), &words, false)
            .await;

        Timer::after_micros(55).await;
    }
}

pub struct RGBButtonPins {
    pub key_1: Peri<'static, PIN_20>,
    pub key_2: Peri<'static, PIN_21>,
    pub key_3: Peri<'static, PIN_22>,
}

#[embassy_executor::task]
pub async fn rgb_task(
    pio: Peri<'static, PIO1>,
    strip_pin: Peri<'static, PIN_28>,
    button_pins: RGBButtonPins,
    dma_strip: Peri<'static, DMA_CH0>,
    dma_buttons: Peri<'static, DMA_CH1>,
) {
    let Pio {
        mut common,
        sm0,
        sm1,
        ..
    } = Pio::new(pio, Irqs);

    let button_pins = [
        common.make_pio_pin(button_pins.key_1),
        common.make_pio_pin(button_pins.key_2),
        common.make_pio_pin(button_pins.key_3),
    ];

    const NUM_LEDS: usize = 26;
    let mut data = [RGB8::default(); NUM_LEDS];
    let mut data_buttons = [[RGB8::default(); NUM_LEDS_PER_BUTTON]; 3];

    let prg = PioWs2812Program::new(&mut common);
    let mut rgb_strip = PioWs2812::new(&mut common, sm0, dma_strip, strip_pin, &prg);
    let prg_parallel = ParallelWs2812Program::new(&mut common);
    let mut rgb_buttons =
        ParallelWs2812::new(&mut common, sm1, button_pins, dma_buttons, &prg_parallel);

    let mut ticker = Ticker::every(Duration::from_millis(TICKER_TIME_MS));
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

        rgb_strip.write(&data).await;

        data_buttons[0][0] = RGB8::new(0xA2, 0x2B, 0x95);
        data_buttons[1][0] = RGB8::new(0x12, 0x34, 0x56);
        data_buttons[2][0] = RGB8::new(0x63, 0x6a, 0x2c);

        rgb_buttons.write(&data_buttons).await;

        ticker.next().await;
    }
}
