#![no_std]
#![no_main]

use embassy_executor::Spawner;
use {defmt_rtt as _, panic_probe as _};

const SENSOR_NUMBER: usize = 3;

mod actuators;
mod managers;
mod sensors;

#[embassy_executor::task]
async fn sensor_manager_thread() {
    let mut imu = sensors::imu::Imu::new();

    let mut sensor_manager = managers::sensor::SensorManager::new();
    sensor_manager.add_sensor(&mut imu);

    loop {}
}

#[embassy_executor::task]
async fn actuator_manager_thread() {
    let mut _actuator_manager = managers::actuator::ActuatorManager::new();

    loop {}
}

#[embassy_executor::task]
async fn comms_manager_thread() {
    let mut _comms_manager = managers::comms::CommsManager::new();

    loop {}
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let p = embassy_stm32::init(Default::default());

    loop {}
}
