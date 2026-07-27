#![no_std]
#![no_main]

use crate::managers::comms;
use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::bind_interrupts;
use embassy_stm32::dma;
use embassy_stm32::peripherals;
use embassy_stm32::usart;
use embassy_time::Timer;
use static_cell::StaticCell;

use {defmt_rtt as _, panic_probe as _};

// const SENSOR_NUMBER: usize = 3;

bind_interrupts!(struct Irqs {
    USART1 => usart::InterruptHandler<peripherals::USART1>;
    DMA2_STREAM7 => dma::InterruptHandler<peripherals::DMA2_CH7>;
    DMA2_STREAM5 => dma::InterruptHandler<peripherals::DMA2_CH5>;
});

static DMA_BUF: StaticCell<[u8; 4096]> = StaticCell::new();

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

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Default::default());

    let _ = Timer::after_secs(1).await;

    info!("Submarine booting...");

    let comms_peripherals = comms::CommsManagerPeripherals {
        reset_pin: p.PA8,
        usart_channel: p.USART1,
        rx_pin: p.PA10,
        tx_pin: p.PA9,
        tx_dma: p.DMA2_CH7,
        rx_dma: p.DMA2_CH5,
    };

    spawner.spawn(comms::comms_manager_thread(comms_peripherals).unwrap());
    // spawner.spawn(sensor_manager_thread().unwrap());
    // spawner.spawn(actuator_manager_thread().unwrap());

    loop {
        Timer::after_secs(10).await;
        defmt::info!("tick");
    }
}
