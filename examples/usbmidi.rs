#![no_std]
#![no_main]

use panic_halt as _;

extern crate ay_driver;
extern crate cortex_m;

use core::borrow::Borrow;
use core::cell::RefCell;
use core::convert::TryInto;

use cortex_m::interrupt::Mutex;
use cortex_m_rt::{entry, exception, ExceptionFrame};
use embedded_hal::spi::{Mode, Phase, Polarity};

use stm32f4xx_hal::otg_fs::{UsbBus, USB};
use stm32f4xx_hal::pac::{interrupt, Interrupt};
use stm32f4xx_hal::{pac, prelude::*, spi::*};

use usb_device::class_prelude::UsbBusAllocator;
use usb_device::prelude::*;
use usbd_midi::data::midi::channel::Channel;
use usbd_midi::data::midi::message::Message;
use usbd_midi::data::usb::constants::USB_AUDIO_CLASS;
use usbd_midi::data::usb_midi::midi_packet_reader::MidiPacketBufferReader;
use usbd_midi::midi_device::MidiClass;

use ay_driver::ay38910;

use core::fmt::Write;

struct MessageBuffer {
    buf_: [Message; 32],
    start_: usize,
    end_: usize,
}

impl MessageBuffer {
    fn new() -> MessageBuffer {
        MessageBuffer {
            buf_: [Message; 32],
            start_: 0,
            end_: 0,
        }
    }

    fn is_full(&self) -> bool {
        self.start_ - self.end_ == self.buf_.len() - 1
    }

    fn is_empty(&self) -> bool {
        self.start_ == self.end_
    }

    fn push(&mut self, msg: Message) -> Result<(), Error> {
        if self.is_full() {
            Err(())
        }
        self.buf_[self.start_] = msg;
        self.start_ = self.start_ + 1 % self.buf_.len();
        Ok(())
    }

    fn pop(&mut self) -> Result<Message, Error> {
        if self.is_full() {
            Err(())
        }
        let msg = self.buf_[self.end_];
        self.end_ = self.end_ + 1 % self.buf_.len();
        Ok(msg)
    }
}

static mut EP_MEMORY: [u32; 1024] = [0; 1024];

/// SPI mode
pub const MODE: Mode = Mode {
    phase: Phase::CaptureOnFirstTransition,
    polarity: Polarity::IdleLow,
};

fn midi_note_to_freq(note: u8) -> u32 {
    440_u32 * 2_u32.pow((note as u32 - 69_u32) / 12_u32)
}

fn velocity_to_level(velocity: u8) -> u8 {
    if velocity == 0 {
        velocity
    } else {
        (velocity / 127) * 15
    }
}

static MESSAGE_FIFO: MessageBuffer = MessageBuffer::new();

// Make USB serial device globally available
static G_USB_MIDI: Mutex<RefCell<Option<MidiClass<UsbBus<USB>>>>> = Mutex::new(RefCell::new(None));

// Make USB device globally available
static G_USB_DEVICE: Mutex<RefCell<Option<UsbDevice<UsbBus<USB>>>>> =
    Mutex::new(RefCell::new(None));

#[entry]
fn main() -> ! {
    static mut USB_BUS: Option<UsbBusAllocator<stm32f4xx_hal::otg_fs::UsbBusType>> = None;
    let dp = pac::Peripherals::take().unwrap();

    let rcc = dp.RCC.constrain();
    // Drive the HSE using the MCO output of ST-Link on the Nucleo-f412ZG board
    let clocks = rcc
        .cfgr
        .use_hse(8.MHz())
        .sysclk(48.MHz())
        .require_pll48clk()
        .freeze();

    let gpioa = dp.GPIOA.split();

    let usb = USB::new(
        (dp.OTG_FS_GLOBAL, dp.OTG_FS_DEVICE, dp.OTG_FS_PWRCLK),
        (gpioa.pa11, gpioa.pa12),
        &clocks,
    );
    *USB_BUS = Some(UsbBus::new(usb, unsafe { &mut EP_MEMORY }));
    let usb_bus = USB_BUS.as_ref().unwrap();

    cortex_m::interrupt::free(|cs| {
        // Create a MIDI class with 1 input and 1 output jack.
        *G_USB_MIDI.borrow(cs).borrow_mut() = Some(MidiClass::new(&usb_bus, 1, 1).unwrap());

        *G_USB_DEVICE.borrow(cs).borrow_mut() = Some(
            UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x16c0, 0x27dd))
                .device_class(USB_AUDIO_CLASS)
                .device_sub_class(0)
                .strings(&[StringDescriptors::default()
                    .product("MIDI Test")
                    .serial_number("test")])
                .unwrap()
                .build(),
        );
    });

    let sck = gpioa.pa5.into_alternate();
    let miso = gpioa.pa6.into_alternate();
    let mosi = gpioa.pa7.into_alternate();
    // Set up the SPI in bidir mode
    let spi = Spi::new(dp.SPI1, (sck, miso, mosi), MODE, 3.MHz(), &clocks);

    let gpiob = dp.GPIOB.split();
    let latch = gpioa.pa4.into_push_pull_output();
    let bdir = gpiob.pb1.into_push_pull_output();
    let bc1 = gpiob.pb2.into_push_pull_output();
    let bc2 = gpiob.pb4.into_push_pull_output();

    let mut ay = ay38910::Driver::new(spi, latch, bdir, bc1, bc2);

    let settings = ay38910::MixerSettings(0x0);
    ay.write(ay38910::MixerControl { settings });

    let gpiod = dp.GPIOD.split();
    // configure serial
    let mut tx = dp.USART3.tx(gpiod.pd8, 9600.bps(), &clocks).unwrap();
    writeln!(tx, "it's alive!\r").unwrap();

    let mut MSG: Option<Message> = None;

    loop {
        if !MESSAGE_FIFO.is_empty() {
            let msg = MESSAGE_FIFO.pop().unwrap();
            match msg {
                Message::NoteOn(Channel::Channel1, note, velocity) => {
                    let note_num: u8 = note.into();
                    let vel: u8 = velocity.into();
                    writeln!(tx, "got a note on message {:?}\r", note_num).unwrap();
                    ay.write(ay38910::ToneControl {
                        chan: ay38910::Channel::A,
                        freq: midi_note_to_freq(note_num),
                    });

                    ay.write(ay38910::AmplitudeControl {
                        chan: ay38910::Channel::A,
                        mode: ay38910::AmplitudeMode::Variable,
                        level: velocity_to_level(vel),
                    });
                }
                Message::NoteOff(Channel::Channel1, ..) => {
                    writeln!(tx, "got a note off message\r").unwrap();
                    ay.write(ay38910::AmplitudeControl {
                        chan: ay38910::Channel::A,
                        mode: ay38910::AmplitudeMode::Variable,
                        level: 0,
                    });
                }
                _ => {}
            }
        }
    }
}

#[interrupt]
fn OTG_FS() {
    static mut USB_MIDI: Option<MidiClass<UsbBus<USB>>> = None;
    static mut USB_DEVICE: Option<UsbDevice<UsbBus<USB>>> = None;

    let usb_dev = USB_DEVICE.get_or_insert_with(|| {
        cortex_m::interrupt::free(|cs| {
            // Move USB device here, leaving a None in its place
            G_USB_DEVICE.borrow(cs).replace(None).unwrap()
        })
    });

    let midi = USB_MIDI.get_or_insert_with(|| {
        cortex_m::interrupt::free(|cs| {
            // Move USB midi device here, leaving a None in its place
            G_USB_MIDI.borrow(cs).replace(None).unwrap()
        })
    });

    if usb_dev.poll(&mut [midi]) {
        let mut buffer = [0; 64];

        if let Ok(size) = midi.read(&mut buffer) {
            let buf_reader = MidiPacketBufferReader::new(&buffer, size);

            for packet in buf_reader.into_iter() {
                if let Ok(packet) = packet {
                    MESSAGE_FIFO.push(packet.message);
                }
            }
        }
    }
}

#[exception]
unsafe fn HardFault(ef: &ExceptionFrame) -> ! {
    panic!("{:#?}", ef);
}
