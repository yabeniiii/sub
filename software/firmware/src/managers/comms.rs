use crate::DMA_BUF;
use crate::Irqs;
use core::str::from_utf8;
use defmt::error;
use defmt::info;
use defmt::panic;
use defmt::warn;
use embassy_stm32::Peri;
use embassy_stm32::gpio;
use embassy_stm32::mode::Async;
use embassy_stm32::peripherals;
use embassy_stm32::usart;
use embassy_stm32::usart::RingBufferedUartRx;
use embassy_stm32::usart::Uart;
use embassy_stm32::usart::UartRx;
use embassy_stm32::usart::UartTx;
use embassy_time::Duration;
use embassy_time::Timer;
use embassy_time::with_timeout;

// enum Message<'a> {
//     Error(heapless::String<64>),
//     Info(heapless::String<64>),
//     Warn(heapless::String<64>),
// }

pub struct CommsManagerPeripherals {
    pub reset_pin: Peri<'static, peripherals::PA8>,
    pub usart_channel: Peri<'static, peripherals::USART1>,
    pub rx_pin: Peri<'static, peripherals::PA10>,
    pub tx_pin: Peri<'static, peripherals::PA9>,
    pub tx_dma: Peri<'static, peripherals::DMA2_CH7>,
    pub rx_dma: Peri<'static, peripherals::DMA2_CH5>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, defmt::Format)]
enum AtCommand {
    Ping,
    DisableEcho,
    APMode,
    StartAP,
}

impl AtCommand {
    fn encode(&self) -> &'static [u8] {
        match self {
            AtCommand::Ping => b"AT\r\n",
            AtCommand::DisableEcho => b"ATE0\r\n",
            AtCommand::APMode => b"AT+CWMODE=2\r\n",
            AtCommand::StartAP => b"AT+CWSAP=\"ESP_AP\",\"12345678\",5,3\r\n",
        }
    }

    fn timeout(&self) -> Duration {
        match self {
            AtCommand::Ping => Duration::from_millis(100),
            AtCommand::DisableEcho => Duration::from_millis(100),
            AtCommand::APMode => Duration::from_secs(3),
            AtCommand::StartAP => Duration::from_secs(3),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
enum AtError {
    BufferFull,
    SendError(usart::Error),
    ReceiveError(usart::Error),
    CommandError,
    Timeout,
}

struct CommsManager<'a> {
    uart_transmitter: UartTx<'a, Async>,
    uart_receiver: RingBufferedUartRx<'a>,
}

impl<'a> CommsManager<'a> {
    fn new(tx: UartTx<'a, Async>, rx: RingBufferedUartRx<'a>) -> Self {
        CommsManager {
            uart_transmitter: tx,
            uart_receiver: rx,
        }
    }

    // fn message(&mut self, message: Message<'a>) {
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

    async fn send_at<'b>(
        &mut self,
        command: AtCommand,
        response_buffer: &'b mut [u8],
    ) -> Result<&'b [u8], AtError> {
        let timeout = command.timeout();

        self.uart_transmitter
            .write(command.encode())
            .await
            .map_err(AtError::SendError)?;

        let length = match with_timeout(timeout, async {
            let mut index = 0;
            loop {
                if index >= response_buffer.len() {
                    warn!("Buffer full on AT Response read: {}", command);
                    return Err(AtError::BufferFull);
                }

                let n = self
                    .uart_receiver
                    .read(&mut response_buffer[index..])
                    .await
                    .map_err(AtError::ReceiveError)?;

                index += n;

                let received = &response_buffer[..index];

                if received.ends_with(b"\r\nOK\r\n") || received.ends_with(b">") {
                    match from_utf8(received) {
                        Ok(_) => info!("{} returned OK", command),
                        Err(_) => error!("{} returned non-UTF8 response", command),
                    }
                    return Ok(index);
                }

                if received.ends_with(b"\r\nERROR\r\n") {
                    match from_utf8(received) {
                        Ok(resp) => error!("{} returned ERROR: {}", command, resp.trim_ascii()),
                        Err(_) => error!("{} returned non-UTF8 response", command),
                    }

                    return Err(AtError::CommandError);
                }
            }
        })
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                error!("timeout on AT command: {}", command);
                return Err(AtError::Timeout);
            }
        };

        Ok(&response_buffer[..length])
    }

    /// Configure ESP modem:
    /// 1. Disable echo
    /// 2. Verify responsiveness
    /// 3. Enter AP mode
    /// 4. Configure AP
    async fn configure_modem(&mut self) -> Result<(), AtError> {
        self.uart_receiver.start_uart();

        let commands = [
            AtCommand::DisableEcho,
            AtCommand::Ping,
            AtCommand::APMode,
            AtCommand::StartAP,
        ];

        let mut response_buf = [0u8; 128];

        for command in commands {
            let _ = self.send_at(command, &mut response_buf).await?;
        }

        info!("Modem Configured");

        Ok(())
    }
}

async fn init_uart(p: CommsManagerPeripherals) -> (UartTx<'static, Async>, UartRx<'static, Async>) {
    let mut reset = gpio::Output::new(p.reset_pin, gpio::Level::High, gpio::Speed::Low);

    reset.set_low();
    Timer::after_millis(20).await;
    reset.set_high();

    Timer::after_secs(3).await;

    match Uart::new(
        p.usart_channel,
        p.rx_pin,
        p.tx_pin,
        p.tx_dma,
        p.rx_dma,
        Irqs,
        usart::Config::default(),
    ) {
        Ok(uart) => {
            info!("Uart initialised.");
            uart.split()
        }
        Err(e) => panic!("USART initialisation failed: {}", e),
    }
}

#[embassy_executor::task]
pub async fn comms_manager_thread(p: CommsManagerPeripherals) {
    info!("Starting communication manager...");
    let dma_buf = DMA_BUF.init([0; 4096]);
    let (tx, rx) = init_uart(p).await;
    let mut manager = CommsManager::new(tx, rx.into_ring_buffered(dma_buf));

    if let Err(e) = manager.configure_modem().await {
        panic!("Modem configuration failed: {}", e);
    }
}
