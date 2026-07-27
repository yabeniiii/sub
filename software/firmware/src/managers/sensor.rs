use heapless::Vec;

pub trait Sensor {
    fn read(&self) -> Option<i64> {
        Some(0)
    }

    fn has_data(&self) -> bool;
}

pub struct SensorManager<'a> {
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
