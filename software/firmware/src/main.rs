#![no_std]
#![no_main]

use core::str::from_utf8;

use crate::managers::comms::ATCommand;
use crate::managers::comms::ATResponse;
use crate::managers::comms::CommsManager;
use defmt::error;
use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::Peri;
use embassy_stm32::bind_interrupts;
use embassy_stm32::dma;
use embassy_stm32::gpio;
use embassy_stm32::mode::Async;
use embassy_stm32::peripherals;
use embassy_stm32::usart;
use embassy_stm32::usart::Uart;
use embassy_stm32::usart::UartRx;
use embassy_stm32::usart::UartTx;
use embassy_time::Timer;

use {defmt_rtt as _, panic_probe as _};

// const SENSOR_NUMBER: usize = 3;

bind_interrupts!(struct Irqs {
    USART1 => usart::InterruptHandler<peripherals::USART1>;
    DMA2_STREAM7 => dma::InterruptHandler<peripherals::DMA2_CH7>;
    DMA2_STREAM5 => dma::InterruptHandler<peripherals::DMA2_CH5>;
});

static mut DMA_BUF: [u8; 4096] = [0u8; 4096];

// mod actuators;
mod managers;
// mod sensors;

// #[embassy_executor::task]
// async fn sensor_manager_thread() {
//     let mut imu = sensors::imu::Imu::new();
//
//     let mut sensor_manager = managers::sensor::SensorManager::new();
//     sensor_manager.add_sensor(&mut imu);
//
//     loop {}
// }

// #[embassy_executor::task]
// async fn actuator_manager_thread() {
//     let mut _actuator_manager = managers::actuator::ActuatorManager::new();
//
//     loop {}
// }

struct CommsManagerPeripherals {
    reset_pin: Peri<'static, peripherals::PA8>,
    usart_channel: Peri<'static, peripherals::USART1>,
    rx_pin: Peri<'static, peripherals::PA10>,
    tx_pin: Peri<'static, peripherals::PA9>,
    tx_dma: Peri<'static, peripherals::DMA2_CH7>,
    rx_dma: Peri<'static, peripherals::DMA2_CH5>,
}

async fn init_modem_uart(
    p: CommsManagerPeripherals,
) -> (UartTx<'static, Async>, UartRx<'static, Async>) {
    let mut reset = gpio::Output::new(p.reset_pin, gpio::Level::High, gpio::Speed::Low);

    reset.toggle();
    Timer::after_millis(20).await;
    reset.toggle();

    Timer::after_secs(3).await;

    Uart::new(
        p.usart_channel,
        p.rx_pin,
        p.tx_pin,
        p.tx_dma,
        p.rx_dma,
        Irqs,
        usart::Config::default(),
    )
    .unwrap()
    .split()
}

#[embassy_executor::task]
async fn comms_manager_thread(p: CommsManagerPeripherals) {
    let (tx, rx) = init_modem_uart(p).await;
    let mut manager = CommsManager::new(tx, rx);

    let mut response = [0u8; 128];
    match manager.send_at(ATCommand::ATE0, &mut response).await {
        ATResponse::BufferFull => error!("Buffer full"),
        ATResponse::SendError(e) => error!("Send Error for ATE0: {}", e),
        ATResponse::ReceiveError(e) => error!("Receive Error for AT: {}", e),
        ATResponse::Ok(bytes) => info!(
            "response to ATE0: {}",
            from_utf8(&response[..bytes]).unwrap().trim_ascii()
        ),
        ATResponse::Error(bytes) => error!(
            "error response to ATE0: {}",
            from_utf8(&response[..bytes]).unwrap().trim_ascii()
        ),
        ATResponse::Timeout => {
            error!("timeout on ATE0");
            return;
        }
    };

    response.fill(0);
    match manager.send_at(ATCommand::AT, &mut response).await {
        ATResponse::BufferFull => error!("Buffer full"),
        ATResponse::SendError(e) => error!("Send Error for AT: {}", e),
        ATResponse::ReceiveError(e) => error!("Receive Error for AT: {}", e),
        ATResponse::Ok(bytes) => info!(
            "response to AT: {}",
            from_utf8(&response[..bytes]).unwrap().trim_ascii()
        ),
        ATResponse::Error(bytes) => error!(
            "error response to AT: {}",
            from_utf8(&response[..bytes]).unwrap().trim_ascii()
        ),
        ATResponse::Timeout => {
            error!("timeout on AT");
        }
    };
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Default::default());

    let _ = Timer::after_secs(1).await;

    info!("Submarine booting...");

    let comms_peripherals = CommsManagerPeripherals {
        reset_pin: p.PA8,
        usart_channel: p.USART1,
        rx_pin: p.PA10,
        tx_pin: p.PA9,
        tx_dma: p.DMA2_CH7,
        rx_dma: p.DMA2_CH5,
    };

    spawner.spawn(comms_manager_thread(comms_peripherals).unwrap());
    // spawner.spawn(sensor_manager_thread().unwrap());
    // spawner.spawn(actuator_manager_thread().unwrap());

    loop {
        Timer::after_secs(10).await;
        defmt::info!("tick");
    }
}
