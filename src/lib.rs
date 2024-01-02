#![cfg_attr(not(test), no_std)]

pub mod ay38910 {
    use embedded_hal::blocking::spi;
    use embedded_hal::digital::v2::OutputPin;

    use self::chip::*;

    pub use amplitude::Mode as AmplitudeMode;
    pub use envelope::ShapeType as EnvelopeShapeType;
    pub use mixer::Settings as MixerSettings;

    pub enum DataPayload {
        Single(chip::Packet),
        Double([chip::Packet; 2]),
    }

    pub trait PayloadGenerator {
        fn get(&self) -> DataPayload;
    }

    pub struct ToneControl {
        pub chan: Channel,
        pub freq: u32,
    }

    impl PayloadGenerator for ToneControl {
        fn get(&self) -> DataPayload {
            DataPayload::Double(tone::period(&self.chan, self.freq, chip::CLOCK_FREQ))
        }
    }

    pub struct NoiseControl {
        pub freq: u32,
    }

    impl PayloadGenerator for NoiseControl {
        fn get(&self) -> DataPayload {
            DataPayload::Single(noise::period(self.freq, chip::CLOCK_FREQ))
        }
    }

    pub struct AmplitudeControl {
        pub chan: Channel,
        pub mode: AmplitudeMode,
        pub level: u8,
    }

    impl PayloadGenerator for AmplitudeControl {
        fn get(&self) -> DataPayload {
            DataPayload::Single(amplitude::set(&self.chan, &self.mode, self.level))
        }
    }

    pub struct EnvelopeShapeCycleControl {
        pub shape: EnvelopeShapeType,
    }

    impl PayloadGenerator for EnvelopeShapeCycleControl {
        fn get(&self) -> DataPayload {
            DataPayload::Single(envelope::shape(&self.shape))
        }
    }

    pub struct EnvelopeFrequencyControl {
        pub freq: f32,
    }

    impl PayloadGenerator for EnvelopeFrequencyControl {
        fn get(&self) -> DataPayload {
            DataPayload::Double(envelope::period(self.freq, chip::CLOCK_FREQ))
        }
    }

    pub struct MixerControl {
        pub settings: MixerSettings,
    }

    impl PayloadGenerator for MixerControl {
        fn get(&self) -> DataPayload {
            DataPayload::Single(mixer::set(&self.settings))
        }
    }

    pub struct Driver<Bus, LatchPin, BDIR, BC1, BC2> {
        bus_control: BusCtrl<BDIR, BC1, BC2>,
        address_bus: AddressBus<Bus, LatchPin>,
    }

    impl<
            Bus: spi::Write<u8>,
            LatchPin: OutputPin,
            BDIR: OutputPin,
            BC1: OutputPin,
            BC2: OutputPin,
        > Driver<Bus, LatchPin, BDIR, BC1, BC2>
    {
        pub fn new(addr_bus: Bus, bus_latch: LatchPin, bdir: BDIR, bc1: BC1, bc2: BC2) -> Self {
            Self {
                bus_control: BusCtrl { bdir, bc1, bc2 },
                address_bus: AddressBus {
                    bus: addr_bus,
                    latch: bus_latch,
                },
            }
        }

        pub fn write<T: PayloadGenerator>(&mut self, data: T) {
            let payload = data.get();
            match payload {
                DataPayload::Single(packet) => {
                    self.write_data(packet.address, packet.value);
                }
                DataPayload::Double(packets) => {
                    for p in packets {
                        self.write_data(p.address, p.value);
                    }
                }
            }
        }

        fn write_data(&mut self, addr: u8, val: u8) {
            self.bus_control.set_inactive();
            self.bus_control.latch_address();
            self.address_bus.write(addr);
            self.bus_control.set_inactive();

            self.bus_control.set_inactive();
            self.address_bus.write(val);
            self.bus_control.write_to_psg();
            self.bus_control.set_inactive();
        }
    }

    pub enum Channel {
        A,
        B,
        C,
    }

    struct BusCtrl<BDIR, BC1, BC2> {
        bdir: BDIR,
        bc1: BC1,
        bc2: BC2,
    }
    impl<BDIR: OutputPin, BC1: OutputPin, BC2: OutputPin> BusCtrl<BDIR, BC1, BC2> {
        pub fn set_inactive(&mut self) {
            let _ = self.bdir.set_low();
            let _ = self.bc1.set_low();
            let _ = self.bc2.set_high();
        }

        pub fn write_to_psg(&mut self) {
            let _ = self.bdir.set_high();
            let _ = self.bc1.set_low();
            let _ = self.bc2.set_high();
        }

        pub fn latch_address(&mut self) {
            let _ = self.bdir.set_high();
            let _ = self.bc1.set_high();
            let _ = self.bc2.set_high();
        }
    }

    struct AddressBus<Bus, LatchPin> {
        bus: Bus,
        latch: LatchPin,
    }

    impl<Bus: spi::Write<u8>, LatchPin: OutputPin> AddressBus<Bus, LatchPin> {
        pub fn write(&mut self, data: u8) {
            let _ = self.latch.set_low();
            let _ = self.bus.write(&mut [data]);
            let _ = self.latch.set_high();
        }
    }

    mod chip {
        const fn parse_u32(s: &str) -> u32 {
            let mut out: u32 = 0;
            let mut i: usize = 0;
            while i < s.len() {
                out *= 10;
                out += (s.as_bytes()[i] - b'0') as u32;
                i += 1;
            }
            out
        }

        pub const CLOCK_FREQ: u32 = parse_u32(core::env!(
            "CLOCK_FREQ_MHz",
            "Must set A-Y-38910 clock frequency!"
        ));
        const CLOCK_COUNTDOWN_COEFF: u32 = 16;

        pub struct Packet {
            pub address: u8,
            pub value: u8,
        }

        mod addr {
            pub const TONE_FINE_CHANNEL_A: u8 = 0x0;
            pub const TONE_COARSE_CHANNEL_A: u8 = 0x1;
            pub const TONE_FINE_CHANNEL_B: u8 = 0x2;
            pub const TONE_COARSE_CHANNEL_B: u8 = 0x3;
            pub const TONE_FINE_CHANNEL_C: u8 = 0x4;
            pub const TONE_COARSE_CHANNEL_C: u8 = 0x5;
            pub const NOISE: u8 = 0x6;
            pub const MIXER_ENABLE: u8 = 0x7;
            pub const AMPLITUDE_CHANNEL_A: u8 = 0x8;
            pub const AMPLITUDE_CHANNEL_B: u8 = 0x9;
            pub const AMPLITUDE_CHANNEL_C: u8 = 0xA;
            pub const COARSE_TUNE: u8 = 0xB;
            pub const FINE_TUNE: u8 = 0xC;
            pub const SHAPE_CYCLE: u8 = 0xD;
        }

        pub mod amplitude {
            use super::*;
            use crate::ay38910::Channel;

            pub enum Mode {
                Fixed,
                Variable,
            }

            pub fn set(chan: &Channel, mode: &Mode, level: u8) -> Packet {
                let addr = match chan {
                    Channel::A => addr::AMPLITUDE_CHANNEL_A,
                    Channel::B => addr::AMPLITUDE_CHANNEL_B,
                    Channel::C => addr::AMPLITUDE_CHANNEL_C,
                };
                let val = match mode {
                    Mode::Fixed => level & 0xF_u8,
                    Mode::Variable => 1 << 4_u8,
                };

                Packet {
                    address: addr,
                    value: val,
                }
            }

            #[cfg(test)]
            mod tests {
                use super::*;

                #[test]
                fn test_amplitude_set_fixed_mode() {
                    let packet = set(&Channel::A, &Mode::Fixed, 2);

                    assert_eq!(packet.address, addr::AMPLITUDE_CHANNEL_A);
                    assert_eq!(packet.value, 2);
                }

                #[test]
                fn test_amplitude_set_variable_mode() {
                    let packet = set(&Channel::A, &Mode::Variable, 2);

                    assert_eq!(packet.address, addr::AMPLITUDE_CHANNEL_A);
                    assert_eq!(packet.value, 1 << 4);
                }
            }
        }

        pub mod envelope {
            use super::*;

            pub fn period(freq: f32, clock_freq: u32) -> [Packet; 2] {
                const COEFF: u32 = 256;
                let clk_div = (clock_freq as f32 / (COEFF as f32 * freq)) as u32;
                let env_per_coarse = (clk_div / COEFF) as u8;
                let env_per_fine = (clk_div % COEFF) as u8;

                [
                    Packet {
                        address: addr::COARSE_TUNE,
                        value: env_per_coarse,
                    },
                    Packet {
                        address: addr::FINE_TUNE,
                        value: env_per_fine,
                    },
                ]
            }

            bitfield::bitfield! {
                pub struct ShapeCycle(u8);
                pub hold, set_hold: 0;
                pub alternate, set_alternate: 1;
                pub attack, set_attack: 2;
                pub cont, set_cont: 3;
            }

            pub enum ShapeType {
                OneShotSaw,
                RampDown,
                RampUp,
                RepeatedSaw,
                RepeatedTriangle,
            }

            pub fn shape(shape_type: &ShapeType) -> Packet {
                let mut shape = ShapeCycle(0);

                match shape_type {
                    ShapeType::OneShotSaw => {
                        shape.set_attack(true);
                    }
                    ShapeType::RampDown => {
                        shape.0 = 0;
                    }
                    ShapeType::RampUp => {
                        shape.set_hold(true);
                        shape.set_attack(true);
                        shape.set_cont(true);
                    }
                    ShapeType::RepeatedSaw => {
                        shape.set_cont(true);
                    }
                    ShapeType::RepeatedTriangle => {
                        shape.set_cont(true);
                        shape.set_alternate(true);
                    }
                }

                Packet {
                    address: addr::SHAPE_CYCLE,
                    value: shape.0,
                }
            }

            #[cfg(test)]
            mod tests {
                use super::*;

                #[test]
                fn test_envelope_shape() {
                    let packet = shape(&ShapeType::RepeatedTriangle);

                    assert_eq!(packet.address, addr::SHAPE_CYCLE);
                    assert_eq!(packet.value, 0xA);
                }

                #[test]
                fn test_envelope_period() {
                    let packets = period(0.5, 2000000);

                    assert_eq!(packets[0].address, addr::COARSE_TUNE);
                    assert_eq!(packets[0].value, 61);
                    assert_eq!(packets[1].address, addr::FINE_TUNE);
                    assert_eq!(packets[1].value, 9);
                }
            }
        }

        pub mod mixer {
            use super::*;

            bitfield::bitfield! {
                pub struct Settings(u8);
                pub tone_channel_a, set_tone_channel_a: 0;
                pub tone_channel_b, set_tone_channel_b: 1;
                pub tone_channel_c, set_tone_channel_c: 2;
                pub noise_channel_a, set_noise_channel_a: 3;
                pub noise_channel_b, set_noise_channel_b: 4;
                pub noise_channel_c, set_noise_channel_c: 5;
                pub input_enable_a, set_input_enable_a: 6;
                pub input_enable_b, set_input_enable_b: 7;
            }

            pub fn set(settings: &Settings) -> Packet {
                Packet {
                    address: addr::MIXER_ENABLE,
                    value: settings.0 & 0x3F,
                }
            }

            #[cfg(test)]
            mod tests {
                use super::*;

                #[test]
                fn test_mixer_settings() {
                    let mut settings = Settings(0);
                    settings.set_tone_channel_a(true);

                    let packet = set(&settings);

                    assert_eq!(packet.address, addr::MIXER_ENABLE);
                    assert_eq!(packet.value, 0x1);
                }
            }
        }

        pub mod noise {
            use super::*;

            pub fn period(freq: u32, clock_freq: u32) -> Packet {
                let mut period = 0;
                if freq > 0 {
                    period = ((clock_freq / (CLOCK_COUNTDOWN_COEFF * freq)) & 0x1F) as u8;
                }
                Packet {
                    address: addr::NOISE,
                    value: period,
                }
            }

            #[cfg(test)]
            mod tests {
                use super::*;

                #[test]
                fn test_noise_period() {
                    let packet = period(4000, 2000000);

                    assert_eq!(packet.address, addr::NOISE);
                    assert_eq!(packet.value, 0x1F);
                }
            }
        }

        pub mod tone {
            use super::*;
            use crate::ay38910::Channel;

            pub fn period(chan: &Channel, freq: u32, clock_freq: u32) -> [Packet; 2] {
                const MEMORY_WIDTH: u32 = 256;
                let scaled_freq = CLOCK_COUNTDOWN_COEFF * freq;
                let tone_period = clock_freq / scaled_freq;
                let coarse = (tone_period / MEMORY_WIDTH) as u8;
                let fine = (tone_period % MEMORY_WIDTH) as u8;

                let (fine_channel_addr, coarse_channel_addr) = match chan {
                    Channel::A => (addr::TONE_FINE_CHANNEL_A, addr::TONE_COARSE_CHANNEL_A),
                    Channel::B => (addr::TONE_FINE_CHANNEL_B, addr::TONE_COARSE_CHANNEL_B),
                    Channel::C => (addr::TONE_FINE_CHANNEL_C, addr::TONE_COARSE_CHANNEL_C),
                };

                [
                    Packet {
                        address: fine_channel_addr,
                        value: fine,
                    },
                    Packet {
                        address: coarse_channel_addr,
                        value: coarse,
                    },
                ]
            }

            #[cfg(test)]
            mod tests {
                use super::*;

                #[test]
                fn test_tone_period() {
                    let packets = period(&Channel::A, 1000, 2000000);

                    assert_eq!(packets[0].address, addr::TONE_FINE_CHANNEL_A);
                    assert_eq!(packets[0].value, 125);
                    assert_eq!(packets[1].address, addr::TONE_COARSE_CHANNEL_A);
                    assert_eq!(packets[1].value, 0);
                }
            }
        }
    }
}
