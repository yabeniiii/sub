#![no_std]
#![no_main]

use core::str::from_utf8;

use crate::managers::comms::ATCommand;
use crate::managers::comms::ATResponse;
use crate::managers::comms::CommsManager;
use crate::managers::comms::CommsManagerPeripherals;
use defmt::error;
use defmt::info;
use embassy_executor::Spawner;
use embassy_time::Timer;

use {defmt_rtt as _, panic_probe as _};

const SENSOR_NUMBER: usize = 3;

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

#[embassy_executor::task]
async fn comms_manager_thread(p: CommsManagerPeripherals) {
    let mut manager = CommsManager::new(p).await;

    let mut response = [0u8; 128];
    match manager.send_at(ATCommand::ATE0, &mut response).await {
        ATResponse::SendError(e) => error!("Send Error for ATE0: {}", e),
        ATResponse::ReceiveError(e) => error!("Receive Error for AT: {}", e),
        ATResponse::Ok { bytes } => info!(
            "response to ATE0: {}",
            from_utf8(&response[..bytes]).unwrap().trim_ascii()
        ),
        ATResponse::Error { bytes } => error!(
            "error response to ATE0: {}",
            from_utf8(&response[..bytes]).unwrap().trim_ascii()
        ),
        ATResponse::Timeout => {
            error!("timeout on ATE0");
            return;
        }
    };

    match manager.send_at(ATCommand::AT, &mut response).await {
        ATResponse::SendError(e) => error!("Send Error for AT: {}", e),
        ATResponse::ReceiveError(e) => error!("Receive Error for AT: {}", e),
        ATResponse::Ok { bytes } => info!(
            "response to AT: {}",
            from_utf8(&response[..bytes]).unwrap().trim_ascii()
        ),
        ATResponse::Error { bytes } => error!(
            "error response to AT: {}",
            from_utf8(&response[..bytes]).unwrap().trim_ascii()
        ),
        ATResponse::Timeout => {
            error!("timeout on AT");
            return;
        }
    };
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Default::default());

    let _ = Timer::after_secs(1).await;

    info!("Submarine booting...");

    spawner.spawn(
        comms_manager_thread(CommsManagerPeripherals {
            reset_pin: p.PA8,
            usart_channel: p.USART1,
            rx_pin: p.PA10,
            tx_pin: p.PA9,
            tx_dma: p.DMA2_CH7,
            rx_dma: p.DMA2_CH5,
        })
        .unwrap(),
    );
    // spawner.spawn(sensor_manager_thread().unwrap());
    // spawner.spawn(actuator_manager_thread().unwrap());

    loop {
        Timer::after_secs(10).await;
        defmt::info!("tick");
    }
}
