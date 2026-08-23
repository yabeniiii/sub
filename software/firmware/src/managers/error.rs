use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
struct ErrorMessage {}
static ERROR_CHANNEL: Channel<CriticalSectionRawMutex, ErrorMessage, 16> = Channel::new();
