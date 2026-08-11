#![no_std]
#![no_main]

use crate::managers::comms;
use defmt::info;
use embassy_executor::Spawner;
use embassy_stm32::bind_interrupts;
use embassy_stm32::dma;
use embassy_stm32::peripherals;
use embassy_stm32::usart;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::Timer;
use static_cell::StaticCell;

use {defmt_rtt as _, panic_probe as _};

const SENSOR_NUMBER: usize = 3;
const UART_RING_BUFFER_SIZE: usize = 4096;

bind_interrupts!(struct Irqs {
    USART1 => usart::InterruptHandler<peripherals::USART1>;
    DMA2_STREAM7 => dma::InterruptHandler<peripherals::DMA2_CH7>;
    DMA2_STREAM5 => dma::InterruptHandler<peripherals::DMA2_CH5>;
});

static UART_DMA_BUF: StaticCell<[u8; UART_RING_BUFFER_SIZE]> = StaticCell::new();

static COMMAND_CHANNEL: Channel<CriticalSectionRawMutex, protocol::Command, 16> = Channel::new();

mod actuators;
mod managers;
mod sensors;

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Default::default());

    info!("Submarine booting...");

    let comms_peripherals = managers::comms::CommsManagerPeripherals {
        reset_pin: p.PA8,
        usart_channel: p.USART1,
        rx_pin: p.PA10,
        tx_pin: p.PA9,
        tx_dma: p.DMA2_CH7,
        rx_dma: p.DMA2_CH5,
    };

    spawner.spawn(managers::comms::comms_manager_thread(comms_peripherals).unwrap());
    // spawner.spawn(managers::sensor::sensor_manager_thread().unwrap());
    // spawner.spawn(managers::actuator::actuator_manager_thread().unwrap());

    comms::COMMS_READY.wait().await;

    info!("Starting main loop...");
    loop {
        Timer::after_secs(10).await;
        // defmt::info!("tick");
    }
}
