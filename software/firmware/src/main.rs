#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_stm32::bind_interrupts;
use embassy_stm32::peripherals;
use embassy_stm32::usart::{self, BufferedUart};
use embassy_time::Timer;
use embedded_io::Read;
use embedded_io::ReadReady;
use embedded_io_async::Write;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

const SENSOR_NUMBER: usize = 3;

mod actuators;
mod managers;
mod sensors;

bind_interrupts!(struct Irqs {
    USART1 => usart::BufferedInterruptHandler<peripherals::USART1>;
});

static TX_BUF: StaticCell<[u8; 64]> = StaticCell::new();
static RX_BUF: StaticCell<[u8; 64]> = StaticCell::new();

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
async fn comms_manager_thread(mut usart: BufferedUart<'static>) {
    let mut _comms_manager = managers::comms::CommsManager::new();

    let _ = usart.write(b"AT\r\n").await;

    defmt::info!("AT written");

    while !usart.read_ready().unwrap() {}

    let mut buf = [0u8; 1];

    usart.read(&mut buf).unwrap();

    defmt::info!("{}", buf);
    loop {}
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    defmt::info!("boot");
    let p = embassy_stm32::init(Default::default());

    let config = usart::Config::default();

    let tx_buf = TX_BUF.init([0u8; 64]);
    let rx_buf = RX_BUF.init([0u8; 64]);
    let modem_uart =
        BufferedUart::new(p.USART1, p.PA10, p.PA9, tx_buf, rx_buf, Irqs, config).unwrap();

    spawner.spawn(comms_manager_thread(modem_uart).unwrap());
    // spawner.spawn(sensor_manager_thread().unwrap());
    // spawner.spawn(actuator_manager_thread().unwrap());

    loop {
        Timer::after_secs(1).await;
        defmt::info!("tick");
    }
}
