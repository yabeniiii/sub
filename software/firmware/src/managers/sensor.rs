use crate::sensors;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use heapless::Vec;

struct SensorData {}

static SENSOR_CHANNEL: Channel<CriticalSectionRawMutex, SensorData, 16> = Channel::new();

pub trait Sensor {
    fn read(&self) -> Option<i64> {
        Some(0)
    }

    fn has_data(&self) -> bool;
}

struct SensorManager<'a> {
    sensors: Vec<&'a mut dyn Sensor, { crate::SENSOR_NUMBER }, u8>,
}

impl<'a> SensorManager<'a> {
    pub fn new() -> Self {
        Self {
            sensors: Vec::new(),
        }
    }

    pub fn add_sensor(&mut self, sensor: &'a mut dyn Sensor) {
        if let Err(e) = self.sensors.push(sensor) {
            defmt::error!(
                "Sensor vector (size: {}) full; could not add sensor: . Increase sensor capacity",
                crate::SENSOR_NUMBER,
            );
        }
    }
}

#[embassy_executor::task]
pub async fn sensor_manager_thread() {
    let mut imu = sensors::imu::Imu::new();

    let mut sensor_manager = SensorManager::new();
    sensor_manager.add_sensor(&mut imu);

    loop {}
}
