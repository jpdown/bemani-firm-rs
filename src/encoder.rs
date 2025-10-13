use defmt::debug;
use embassy_rp::Peri;
use embassy_rp::bind_interrupts;
use embassy_rp::peripherals::PIN_0;
use embassy_rp::peripherals::PIN_1;
use embassy_rp::peripherals::PIO0;
use embassy_rp::pio::InterruptHandler;
use embassy_rp::pio::Pio;
use embassy_rp::pio_programs::rotary_encoder::Direction;
use embassy_rp::pio_programs::rotary_encoder::PioEncoder;
use embassy_rp::pio_programs::rotary_encoder::PioEncoderProgram;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;

const PPR: u16 = 360;
const TARGET_STEPS: u16 = 144;

// TODO: make the encoder do quarter steps so this math is nicer
const THRESHOLD: u8 = ((PPR) / gcd(PPR, TARGET_STEPS)) as u8;
const ENCODER_STEP: u8 = (TARGET_STEPS / gcd(PPR, TARGET_STEPS)) as u8;

const fn gcd(n: u16, m: u16) -> u16 {
    if m == 0 { n } else { gcd(m, n % m) }
}

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

#[embassy_executor::task]
pub async fn encoder_task(
    pio: Peri<'static, PIO0>,
    pin_0: Peri<'static, PIN_0>,
    pin_1: Peri<'static, PIN_1>,
    output: &'static Signal<CriticalSectionRawMutex, u8>,
) {
    let Pio {
        mut common, sm0, ..
    } = Pio::new(pio, Irqs);

    let prg = PioEncoderProgram::new(&mut common);
    let mut encoder_0 = PioEncoder::new(&mut common, sm0, pin_0, pin_1, &prg);

    let mut rolling_delta: i16 = 0;
    let mut game_reported_value: u8 = 0;

    loop {
        rolling_delta += match encoder_0.read().await {
            Direction::Clockwise => ENCODER_STEP as i16,
            Direction::CounterClockwise => -(ENCODER_STEP as i16),
        };

        if rolling_delta > THRESHOLD as i16 {
            rolling_delta -= THRESHOLD as i16;
            game_reported_value += 1;
        } else if rolling_delta < 0 {
            rolling_delta += THRESHOLD as i16;
            game_reported_value -= 1;
        }

        debug!(
            "rolling {} game reported {}",
            rolling_delta, game_reported_value
        );

        output.signal(game_reported_value);
    }
}
