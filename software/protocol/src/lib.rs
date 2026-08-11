#![no_std]

use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
pub enum Commands {
    AscendDescend,
    AheadAstern,
    Rudder,
    Tilt,
}

#[derive(Serialize, Deserialize)]
pub struct Command {
    pub command: Commands,
    pub value: i8,
}

#[derive(Serialize, Deserialize)]
pub struct SensorData {
    mock_sens: u32,
}

#[derive(Serialize, Deserialize)]
pub enum Log<'a> {
    Info(&'a str),
    Warn(&'a str),
    Error(&'a str),
    Panic(&'a str),
}
