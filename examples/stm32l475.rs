#![no_std]
#![no_main]

extern crate ay_driver;
extern crate cortex_m;

use panic_halt as _;

use stm32l4xx_hal as hal;

use ay_driver::ay38910;
use cortex_m_rt::{entry, exception, ExceptionFrame};
use embedded_hal::spi::{Mode, Phase, Polarity};
use hal::delay::Delay;
use hal::prelude::*;

/// SPI mode
pub const MODE: Mode = Mode {
    phase: Phase::CaptureOnFirstTransition,
    polarity: Polarity::IdleLow,
};

#[entry]
fn main() -> ! {
    let cp = cortex_m::Peripherals::take().unwrap();
    let dp = hal::stm32::Peripherals::take().unwrap();

    let mut flash = dp.FLASH.constrain();
    let mut rcc = dp.RCC.constrain();
    let mut pwr = dp.PWR.constrain(&mut rcc.apb1r1);

    let clocks = rcc
        .cfgr
        .sysclk(80.MHz())
        .pclk1(80.MHz())
        .pclk2(80.MHz())
        .freeze(&mut flash.acr, &mut pwr);

    let mut gpioa = dp.GPIOA.split(&mut rcc.ahb2);
    let sck = gpioa
        .pa5
        .into_alternate(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrl);
    let miso = gpioa
        .pa6
        .into_alternate(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrl);
    let mosi = gpioa
        .pa7
        .into_alternate(&mut gpioa.moder, &mut gpioa.otyper, &mut gpioa.afrl);

    let latch = gpioa
        .pa4
        .into_push_pull_output(&mut gpioa.moder, &mut gpioa.otyper);

    let spi = hal::spi::Spi::spi1(
        dp.SPI1,
        (sck, miso, mosi),
        MODE,
        4.MHz(),
        clocks,
        &mut rcc.apb2,
    );

    let mut gpiob = dp.GPIOB.split(&mut rcc.ahb2);
    let bdir = gpiob
        .pb1
        .into_push_pull_output(&mut gpiob.moder, &mut gpiob.otyper);
    let bc1 = gpiob
        .pb2
        .into_push_pull_output(&mut gpiob.moder, &mut gpiob.otyper);
    let bc2 = gpiob
        .pb4
        .into_push_pull_output(&mut gpiob.moder, &mut gpiob.otyper);

    let mut ay = ay38910::Driver::new(spi, latch, bdir, bc1, bc2);

    let mut settings = ay38910::MixerSettings(0xFF);
    settings.set_tone_channel_a(false);
    settings.set_tone_channel_b(false);
    settings.set_tone_channel_c(false);
    ay.write(ay38910::MixerControl { settings });

    ay.write(ay38910::EnvelopeShapeCycleControl {
        shape: ay38910::EnvelopeShapeType::RepeatedSaw,
    });
    ay.write(ay38910::EnvelopeFrequencyControl { freq: 3500.0 });

    ay.write(ay38910::AmplitudeControl {
        chan: ay38910::Channel::A,
        mode: ay38910::AmplitudeMode::Variable,
        level: 15,
    });

    ay.write(ay38910::AmplitudeControl {
        chan: ay38910::Channel::B,
        mode: ay38910::AmplitudeMode::Variable,
        level: 0,
    });

    ay.write(ay38910::AmplitudeControl {
        chan: ay38910::Channel::C,
        mode: ay38910::AmplitudeMode::Variable,
        level: 0,
    });

    ay.write(ay38910::ToneControl {
        chan: ay38910::Channel::A,
        freq: 880,
    });

    let mut timer = Delay::new(cp.SYST, clocks);
    let mut current_freq = 0;
    let freqs = [440, 660, 220, 880];
    loop {
        timer.delay_ms(500_u32);

        current_freq = (current_freq + 1) % 4;
        ay.write(ay38910::ToneControl {
            chan: ay38910::Channel::A,
            freq: freqs[current_freq],
        });

        timer.delay_ms(50_u32);

        ay.write(ay38910::ToneControl {
            chan: ay38910::Channel::A,
            freq: freqs[(current_freq + 1) % 4],
        });
    }
}

#[exception]
unsafe fn HardFault(ef: &ExceptionFrame) -> ! {
    panic!("{:#?}", ef);
}
