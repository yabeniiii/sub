use crate::DMA_BUF;

use defmt::error;
use defmt::info;
use defmt::warn;
use embassy_stm32::mode::Async;
use embassy_stm32::usart;
use embassy_stm32::usart::RingBufferedUartRx;
use embassy_stm32::usart::UartRx;
use embassy_stm32::usart::UartTx;
use embassy_time::Duration;
use embassy_time::with_timeout;

// pub enum Message<'a> {
//     Error(heapless::String<64>),
//     Info(heapless::String<64>),
//     Warn(heapless::String<64>),
// }

pub enum ATCommand {
    AT,
    ATE0,
}

impl ATCommand {
    fn bytes(&self) -> &'static [u8] {
        match self {
            ATCommand::AT => b"AT\r\n",
            ATCommand::ATE0 => b"ATE0\r\n",
        }
    }

    fn timeout(&self) -> Duration {
        match self {
            ATCommand::AT => Duration::from_millis(100),
            ATCommand::ATE0 => Duration::from_millis(100),
        }
    }
}

pub enum ATResponse {
    BufferFull,
    SendError(usart::Error),
    ReceiveError(usart::Error),
    Ok(usize),
    Error(usize),
    Timeout,
}

impl ATResponse {
    fn is_complete(buffer: &[u8]) -> Option<Self> {
        if buffer.windows(6).any(|w| w == b"\r\nOK\r\n") {
            Some(Self::Ok(buffer.len()))
        } else if buffer.windows(9).any(|w| w == b"\r\nERROR\r\n") {
            Some(Self::Error(buffer.len()))
        } else {
            None
        }
    }
}

pub struct CommsManager<'a> {
    uart_transmitter: UartTx<'a, Async>,
    uart_receiver: RingBufferedUartRx<'a>,
}

impl<'a> CommsManager<'a> {
    pub fn new(tx: UartTx<'a, Async>, rx: UartRx<'a, Async>) -> Self {
        let mut rx = rx.into_ring_buffered(unsafe { &mut *core::ptr::addr_of_mut!(DMA_BUF) });
        rx.start_uart();

        CommsManager {
            uart_transmitter: tx,
            uart_receiver: rx,
        }
    }

    // pub fn message(&mut self, message: Message<'a>) {
    //     match message {
    //         Message::Error(message) => {
    //             error!("{}", message);
    //         }
    //         Message::Info(message) => {
    //             info!("{}", message);
    //         }
    //         Message::Warn(message) => {
    //             warn!("{}", message);
    //         }
    //     }
    // }

    pub async fn drain_buffer(&mut self) {}

    pub async fn send_at(&mut self, command: ATCommand, response_buffer: &mut [u8]) -> ATResponse {
        let timeout = command.timeout();

        if let Err(e) = self.uart_transmitter.write(command.bytes()).await {
            return ATResponse::SendError(e);
        }

        let mut index = 0;
        loop {
            if index >= response_buffer.len() {
                return ATResponse::BufferFull;
            }

            let n = match with_timeout(
                timeout,
                self.uart_receiver.read(&mut response_buffer[index..]),
            )
            .await
            {
                Ok(Ok(n)) => n,
                Ok(Err(e)) => return ATResponse::ReceiveError(e),
                Err(_) => {
                    error!("timeout on AT command");
                    return ATResponse::Timeout;
                }
            };
            index += n;

            if let Some(response) = ATResponse::is_complete(&response_buffer[..index]) {
                return response;
            }
        }
    }
}
