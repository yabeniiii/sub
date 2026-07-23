#![no_std]

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Ping {
    pub id: u32,
}
