use crate::managers::comms::AtError;
use embassy_stm32::Peri;
use embassy_stm32::gpio;
use embassy_stm32::peripherals;
use embassy_stm32::usart;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Timer;

pub struct RecoverableError {}
pub struct CriticalError {
    pub message: &'static str,
    pub error: ErrorWrap,
}

pub enum ErrorWrap {
    At(AtError),
    Usart(usart::Error),
}

pub static RECOVERABLE_ERROR_CHANNEL: Channel<CriticalSectionRawMutex, RecoverableError, 16> =
    Channel::new();
pub static CRITICAL_ERROR_CHANNEL: Channel<CriticalSectionRawMutex, CriticalError, 4> =
    Channel::new();

#[embassy_executor::task]
pub async fn error_manager_thread(p: ErrorManagerPeripherals) {
    let mut manager = ErrorManager::new(p);
    loop {
        match CRITICAL_ERROR_CHANNEL.try_receive() {
            Ok(critical_error) => manager.panic(critical_error),
            Err(e) => (), // channel is empty, no need to do anything
        }

        Timer::after_millis(20);
    }
}

pub struct ErrorManagerPeripherals {
    pub watchdog_pin: Peri<'static>,
    pub failsafe_trigger_pin: Peri<'static>,
    pub current_sense_pin: Peri<'static, peripherals::PA1>,
}

struct ErrorManager<'a> {
    failsafe_trigger: gpio::Output<'a>,
}

impl<'a> ErrorManager<'a> {
    fn new(p: ErrorManagerPeripherals) -> Self {
        ErrorManager {
            failsafe_trigger: gpio::Output::new(
                p.failsafe_trigger_pin,
                gpio::Level::High,
                gpio::Speed::High,
            ),
        }
    }

    fn panic(&mut self, error: CriticalError) {
        self.failsafe_trigger.set_low();
        Timer::after_secs(1);
        self.failsafe_trigger.set_high();
    }
}
