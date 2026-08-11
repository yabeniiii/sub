use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;

struct Actuate {}
static ACTUATE_CHANNEL: Channel<CriticalSectionRawMutex, Actuate, 16> = Channel::new();

pub struct ActuatorManager {}

impl ActuatorManager {
    fn new() -> Self {
        ActuatorManager {}
    }
}

#[embassy_executor::task]
pub async fn actuator_manager_thread() {
    loop {}
}
