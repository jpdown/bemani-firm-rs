use defmt::debug;
use embassy_rp::Peri;
use embassy_rp::bind_interrupts;
use embassy_rp::clocks::clk_sys_freq;
use embassy_rp::peripherals::PIN_0;
use embassy_rp::peripherals::PIN_1;
use embassy_rp::peripherals::PIO0;
use embassy_rp::pio::Common;
use embassy_rp::pio::Config;
use embassy_rp::pio::FifoJoin;
use embassy_rp::pio::Instance;
use embassy_rp::pio::InterruptHandler;
use embassy_rp::pio::LoadedProgram;
use embassy_rp::pio::Pin;
use embassy_rp::pio::Pio;
use embassy_rp::pio::ShiftDirection;
use embassy_rp::pio::StateMachine;
use embassy_rp::pio::program::pio_asm;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use fixed::traits::ToFixed;

pub const PPR: i32 = 360 * 4;
const TARGET_STEPS: i32 = 144;

const THRESHOLD: i32 = (PPR) / gcd(PPR, TARGET_STEPS);
const ENCODER_STEP: i32 = TARGET_STEPS / gcd(PPR, TARGET_STEPS);

const EXPECTED_MAX_ROTATIONS_PER_SECOND: u32 = 50;
const REQUIRED_SAMPLE_CLOCK_RATE: u32 = PPR as u32 * 10 * EXPECTED_MAX_ROTATIONS_PER_SECOND;

const fn gcd(n: i32, m: i32) -> i32 {
    if m == 0 { n } else { gcd(m, n % m) }
}

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

pub struct QuadratureEncoderProgram<'a, PIO: Instance> {
    prg: LoadedProgram<'a, PIO>,
}

impl<'a, PIO: Instance> QuadratureEncoderProgram<'a, PIO> {
    pub fn new(common: &mut Common<'a, PIO>) -> Self {
        // https://github.com/raspberrypi/pico-examples/blob/master/pio/quadrature_encoder/quadrature_encoder.pio
        let prg = pio_asm!(
            ".origin 0",
            // 00 state
            "JMP update",    // read 00
            "JMP decrement", // read 01
            "JMP increment", // read 10
            "JMP update",    // read 11
            // 01 state
            "JMP increment", // read 00
            "JMP update",    // read 01
            "JMP update",    // read 10
            "JMP decrement", // read 11
            // 10 state
            "JMP decrement", // read 00
            "JMP update",    // read 01
            "JMP update",    // read 10
            "JMP increment", // read 11
            // last 2 states implemented in place, become target for other jumps
            // 11 state
            "JMP update",    // read 00
            "JMP increment", // read 01
            "decrement:",
            "JMP Y--, update", // read 10
            // main loop begins
            ".wrap_target",
            "update:",
            "MOV ISR, Y", // read 11
            "PUSH noblock",
            "sample_pins:",
            "OUT ISR, 2",
            "IN PINS, 2",
            "MOV OSR, ISR",
            "MOV PC, ISR",
            "increment:",
            "MOV Y, ~Y",
            "JMP Y--, increment_cont",
            "increment_cont:",
            "MOV Y, ~Y",
            ".wrap"
        );

        let prg = common.load_program(&prg.program);

        Self { prg }
    }
}

pub struct QuadratureEncoder<'d, T: Instance, const SM: usize> {
    sm: StateMachine<'d, T, SM>,
}

impl<'d, T: Instance, const SM: usize> QuadratureEncoder<'d, T, SM> {
    pub fn new(
        mut sm: StateMachine<'d, T, SM>,
        mut pin_0: Pin<'d, T>,
        mut pin_1: Pin<'d, T>,
        program: &QuadratureEncoderProgram<'d, T>,
    ) -> Self {
        pin_0.set_pull(embassy_rp::gpio::Pull::Up);
        pin_1.set_pull(embassy_rp::gpio::Pull::Up);

        let borrowed_pio_pins = [&pin_0, &pin_1];

        sm.set_pin_dirs(embassy_rp::pio::Direction::In, &borrowed_pio_pins);

        let mut cfg = Config::default();
        cfg.set_in_pins(&borrowed_pio_pins);
        cfg.set_jmp_pin(&pin_0);

        cfg.use_program(&program.prg, &[]);

        cfg.shift_in.auto_fill = false;
        cfg.shift_in.direction = ShiftDirection::Left;
        cfg.shift_in.threshold = 32;

        cfg.fifo_join = FifoJoin::Duplex;

        let clock_freq = clk_sys_freq();

        let divider = clock_freq / REQUIRED_SAMPLE_CLOCK_RATE;

        cfg.clock_divider = divider.to_fixed();
        debug!(
            "clock freq {} requested rate {} clock divider {}",
            clock_freq, REQUIRED_SAMPLE_CLOCK_RATE, divider
        );

        sm.set_config(&cfg);
        sm.set_enable(true);

        Self { sm }
    }

    pub async fn read(&mut self) -> i32 {
        let num_to_purge = self.sm.rx().level();
        for _ in 0..num_to_purge {
            self.sm.rx().pull();
        }

        self.sm.rx().wait_pull().await as i32
    }
}

#[embassy_executor::task]
pub async fn encoder_task(
    pio: Peri<'static, PIO0>,
    pin_0: Peri<'static, PIN_0>,
    pin_1: Peri<'static, PIN_1>,
    output: &'static Signal<CriticalSectionRawMutex, u8>,
    output_raw: &'static Signal<CriticalSectionRawMutex, i32>,
) {
    let Pio {
        mut common, sm0, ..
    } = Pio::new(pio, Irqs);

    let pin_0 = common.make_pio_pin(pin_0);
    let pin_1 = common.make_pio_pin(pin_1);

    let prg = QuadratureEncoderProgram::new(&mut common);
    let mut encoder_0 = QuadratureEncoder::new(sm0, pin_0, pin_1, &prg);

    let mut last_value: i32 = 0;
    let mut rolling_delta: i32 = 0;
    let mut game_reported_value: u8 = 0;

    loop {
        let new_reading = encoder_0.read().await;

        rolling_delta += (new_reading - last_value) * ENCODER_STEP;

        if rolling_delta > THRESHOLD {
            rolling_delta -= THRESHOLD;
            game_reported_value += 1;
        } else if rolling_delta < 0 {
            rolling_delta += THRESHOLD;
            game_reported_value -= 1;
        }

        // if last_value != new_reading {
        //     debug!(
        //         "threshold {} encoder_step {} raw {} rolling {} game reported {}",
        //         THRESHOLD, ENCODER_STEP, new_reading, rolling_delta, game_reported_value
        //     );
        // }

        last_value = new_reading;

        output.signal(game_reported_value);
        output_raw.signal(new_reading);
    }
}
