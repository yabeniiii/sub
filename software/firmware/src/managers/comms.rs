use crate::Irqs;
use crate::UART_DMA_BUF;
use crate::UART_RING_BUFFER_SIZE;
use core::str::from_utf8;
use defmt::error;
use defmt::info;
use defmt::panic;
use embassy_stm32::Peri;
use embassy_stm32::gpio;
use embassy_stm32::mode::Async;
use embassy_stm32::peripherals;
use embassy_stm32::usart;
use embassy_stm32::usart::RingBufferedUartRx;
use embassy_stm32::usart::Uart;
use embassy_stm32::usart::UartRx;
use embassy_stm32::usart::UartTx;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Duration;
use embassy_time::Timer;
use embassy_time::with_timeout;
use embedded_io::ReadReady;

#[embassy_executor::task]
pub async fn comms_manager_thread(p: CommsManagerPeripherals) {
    info!("Starting communication manager...");
    let dma_buf = UART_DMA_BUF.init([0; UART_RING_BUFFER_SIZE]);
    let (tx, rx) = init_uart(p).await;
    let mut manager = CommsManager::new(tx, rx.into_ring_buffered(dma_buf));

    if let Err(e) = manager.configure_modem().await {
        panic!("Modem configuration failed: {}", e);
    }

    loop {
        if let Err(e) = manager.check_uart().await {
            panic!("UART fetch failed: {}", e);
        }
        if let Err(e) = manager.send_sensor_data().await {
            panic!("UART fetch failed: {}", e);
        }
        if let Err(e) = manager.send_log().await {
            panic!("UART fetch faile: {}", e);
        }

        Timer::after_millis(10).await;
    }
}

pub static COMMS_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();

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
    EnableSAP,
    FixDhcpRange,
    CheckIP,
    SingleConn,
    StartConnection,
}

impl AtCommand {
    fn encode(&self) -> &'static [u8] {
        match self {
            AtCommand::Ping => b"AT\r\n",
            AtCommand::DisableEcho => b"ATE0\r\n",
            AtCommand::APMode => b"AT+CWMODE=2\r\n",
            AtCommand::EnableSAP => b"AT+CWSAP=\"ESP_AP\",\"12345678\",5,3\r\n",
            AtCommand::FixDhcpRange => b"AT+CWDHCPS_CUR=1,3,\"192.168.4.10\",\"192.168.4.10\"\r\n",
            AtCommand::CheckIP => b"AT+CIFSR\r\n",
            AtCommand::SingleConn => b"AT+CIPMUX=0\r\n",
            AtCommand::StartConnection => b"AT+CIPSTART=\"TCP\",\"192.168.4.10\",5000\r\n",
        }
    }

    fn timeout(&self) -> Duration {
        match self {
            AtCommand::APMode | AtCommand::EnableSAP | AtCommand::FixDhcpRange => {
                Duration::from_millis(1_000)
            }
            AtCommand::StartConnection => Duration::from_millis(5_000),
            _ => Duration::from_millis(100),
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

enum WriteType {
    Sensor,
    Log,
}

struct CommsManager<'a> {
    uart_transmitter: UartTx<'a, Async>,
    uart_receiver: RingBufferedUartRx<'a>,
    tcp_write_ready: bool,
    tcp_write_type: WriteType,
    parse_buf: [u8; UART_RING_BUFFER_SIZE],
    parse_len: usize,
}

impl<'a> CommsManager<'a> {
    fn new(tx: UartTx<'a, Async>, rx: RingBufferedUartRx<'a>) -> Self {
        CommsManager {
            uart_transmitter: tx,
            uart_receiver: rx,
            tcp_write_ready: false,
            tcp_write_type: WriteType::Log,
            parse_buf: [0u8; UART_RING_BUFFER_SIZE],
            parse_len: 0,
        }
    }

    async fn send_at(
        &mut self,
        command: &mut AtCommand,
        response_buf: &mut [u8],
    ) -> Result<(), AtError> {
        info!("Sending AT Command: {}", command);
        self.uart_transmitter
            .write(command.encode())
            .await
            .map_err(AtError::SendError)?;

        let _ = match with_timeout(command.timeout(), async {
            let mut index = 0;
            loop {
                if index >= response_buf.len() {
                    error!("Buffer full on AT Response read: {}", command);
                    return Err(AtError::BufferFull);
                }

                let n = self
                    .uart_receiver
                    .read(&mut response_buf[index..])
                    .await
                    .map_err(AtError::ReceiveError)?;

                index += n;

                let received = &response_buf[..index];

                if received.ends_with(b"\r\nOK\r\n") {
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
        Ok(())
    }

    async fn configure_modem(&mut self) -> Result<(), AtError> {
        info!("Configuring modem...");
        self.uart_receiver.start_uart();

        let mut commands = [
            AtCommand::DisableEcho,  // 0. Disable echo
            AtCommand::Ping,         // 1. Verify responsiveness
            AtCommand::APMode,       // 2. Enter AP mode
            AtCommand::EnableSAP,    // 3. Configure AP
            AtCommand::FixDhcpRange, // 4. Restrict DHCP to one known IP
            AtCommand::CheckIP,      // 5. Get AP's own IP
            AtCommand::SingleConn,   // 6. Single-connection mode, ESP as client
        ];

        let mut response_buf = [0u8; 128];

        for command in &mut commands {
            if let Err(e) = self.send_at(command, &mut response_buf).await {
                error!("Error sending AT command: {}, {}", command, e);
            }
        }

        info!("Modem Configured");

        Ok(())
    }

    async fn check_uart(&mut self) -> Result<(), usart::Error> {
        if !self.uart_receiver.read_ready()? {
            return Ok(());
        }

        let n = self
            .uart_receiver
            .read(&mut self.parse_buf[self.parse_len..])
            .await?;
        self.parse_len += n;

        if self.parse_len >= self.parse_buf.len() {
            error!("parse buffer full, resetting");
            self.parse_len = 0;
            return Err(usart::Error::Overrun); // or a dedicated variant
        }

        loop {
            let data = &self.parse_buf[..self.parse_len];
            if data.is_empty() {
                break;
            }

            let consumed = if data[0] == b'>' {
                self.tcp_write_ready = true;
                1
            } else if data.starts_with(b"+IPD,") {
                let Some(colon) = data.iter().position(|&b| b == b':') else {
                    break;
                };
                let Some(len) = from_utf8(&data[5..colon])
                    .ok()
                    .and_then(|s| s.rsplit(',').next()?.parse::<usize>().ok())
                else {
                    break;
                };
                let end = colon + 1 + len;
                if data.len() < end {
                    break;
                }
                // dispatch_tcp_data(&data[colon + 1..end]);
                end
            } else if let Some(eol) = data.windows(2).position(|w| w == b"\r\n") {
                // dispatch_line(&data[..eol]);
                match from_utf8(&data[..eol]) {
                    Ok(str) => info!("Non-TCP URC arrived: {}", str),
                    Err(_) => info!(
                        "Non-TCP URC arrived, failed UTF-8 conversion: {}",
                        data[..eol]
                    ),
                }
                if data.starts_with(b"+STA_CONNECTED") {
                    let mut response_buf = [0u8; 128];
                    if let Err(e) = self
                        .send_at(&mut AtCommand::StartConnection, &mut response_buf)
                        .await
                    {
                        error!("Connection to server failed: {}", e);
                        panic!("Server connection failed, aborting boot...");
                    }
                    COMMS_READY.signal(());
                }
                eol + 2
            } else {
                break;
            };

            self.parse_buf.copy_within(consumed..self.parse_len, 0);
            self.parse_len -= consumed;
        }

        Ok(())
    }

    async fn send_sensor_data(&mut self) -> Result<(), usart::Error> {
        Ok(())
    }

    async fn send_log(&mut self) -> Result<(), usart::Error> {
        Ok(())
    }
}

async fn init_uart(p: CommsManagerPeripherals) -> (UartTx<'static, Async>, UartRx<'static, Async>) {
    info!("Initialising UART...");
    let mut reset = gpio::Output::new(p.reset_pin, gpio::Level::High, gpio::Speed::Low);

    reset.set_low();
    Timer::after_millis(20).await;
    reset.set_high();

    Timer::after_millis(500).await;

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
            info!("Uart initialised");
            uart.split()
        }
        Err(e) => panic!("USART initialisation failed: {}", e),
    }
}
