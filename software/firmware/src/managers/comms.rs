use core::str::FromStr;

use defmt::error;
use defmt::info;
use defmt::warn;
use embassy_stm32::Peri;
use embassy_stm32::bind_interrupts;
use embassy_stm32::dma;
use embassy_stm32::gpio;
use embassy_stm32::mode::Async;
use embassy_stm32::peripherals;
use embassy_stm32::usart;
use embassy_stm32::usart::RingBufferedUartRx;
use embassy_stm32::usart::Uart;
use embassy_stm32::usart::UartTx;
use embassy_time::Duration;
use embassy_time::Timer;
use embassy_time::with_timeout;

bind_interrupts!(struct Irqs {
    USART1 => usart::InterruptHandler<peripherals::USART1>;
    DMA2_STREAM7 => dma::InterruptHandler<peripherals::DMA2_CH7>;
    DMA2_STREAM5 => dma::InterruptHandler<peripherals::DMA2_CH5>;
});

static mut DMA_BUF: [u8; 4096] = [0u8; 4096];

// pub enum Message<'a> {
//     Error(heapless::String<64>),
//     Info(heapless::String<64>),
//     Warn(heapless::String<64>),
// }

pub enum ATCommand {
    AT,
    ATE0,
}

pub enum ATResponse {
    SendError(usart::Error),
    ReceiveError(usart::Error),
    Ok { bytes: usize },
    Error { bytes: usize },
    Timeout,
}

pub struct CommsManagerPeripherals {
    pub reset_pin: Peri<'static, peripherals::PA8>,
    pub usart_channel: Peri<'static, peripherals::USART1>,
    pub rx_pin: Peri<'static, peripherals::PA10>,
    pub tx_pin: Peri<'static, peripherals::PA9>,
    pub tx_dma: Peri<'static, peripherals::DMA2_CH7>,
    pub rx_dma: Peri<'static, peripherals::DMA2_CH5>,
}

pub struct CommsManager<'a> {
    uart_reset: gpio::Output<'a>,
    uart_transmitter: UartTx<'a, Async>,
    uart_receiver: RingBufferedUartRx<'a>,
}

impl<'a> CommsManager<'a> {
    pub async fn new(p: CommsManagerPeripherals) -> Self {
        let config = usart::Config::default(); // 115200 baud, 8N1  

        let mut reset = gpio::Output::new(p.reset_pin, gpio::Level::High, gpio::Speed::Low);
        reset.toggle();
        Timer::after_millis(20).await;
        reset.toggle();

        Timer::after_secs(3).await;

        let usart = Uart::new(
            p.usart_channel,
            p.rx_pin, // RX
            p.tx_pin, // TX
            p.tx_dma, // TX DMA
            p.rx_dma, // RX DMA
            Irqs,
            config,
        )
        .unwrap();

        let (tx, rx) = usart.split();
        let mut rx = rx.into_ring_buffered(unsafe { &mut *core::ptr::addr_of_mut!(DMA_BUF) });
        rx.start_uart();

        CommsManager {
            uart_reset: reset,
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

    pub async fn send_at(&mut self, command: ATCommand, response_buffer: &mut [u8]) -> ATResponse {
        let timeout = match command {
            ATCommand::AT => {
                if let Err(e) = self.uart_transmitter.write(b"AT\r\n").await {
                    return ATResponse::SendError(e);
                }
                Duration::from_millis(100)
            }
            ATCommand::ATE0 => {
                if let Err(e) = self.uart_transmitter.write(b"ATE0\r\n").await {
                    return ATResponse::SendError(e);
                }
                Duration::from_millis(100)
            }
        };

        let mut index = 0;
        loop {
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

            if response_buffer[..index]
                .windows(6)
                .any(|w| w == b"\r\nOK\r\n")
            {
                return ATResponse::Ok { bytes: index };
            } else if response_buffer[..index]
                .windows(9)
                .any(|w| w == b"\r\nERROR\r\n")
            {
                return ATResponse::Error { bytes: index };
            }
        }
    }
}
