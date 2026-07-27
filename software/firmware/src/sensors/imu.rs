use crate::managers::sensor::Sensor;
use heapless::Vec;

struct IMUData {
    acc_x: i64,
    acc_y: i64,
}

const BUFFER_SIZE: usize = 20;

pub struct Imu<'a> {
    name: &'a str,
    buffer: Vec<IMUData, BUFFER_SIZE, u16>,
}

impl Imu<'_> {
    pub fn new() -> Self {
        Self {
            name: "IMU",
            buffer: Vec::new(),
        }
    }
}

impl Sensor for Imu<'_> {
    fn has_data(&self) -> bool {
        return !self.buffer.is_empty();
    }
}
