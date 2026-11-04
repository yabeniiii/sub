use crate::Irqs;
use crate::UART_DMA_BUF;
use crate::UART_RING_BUFFER_SIZE;
use crate::managers::error::CRITICAL_ERROR_CHANNEL;
use crate::managers::error::CriticalError;
use crate::managers::error::ErrorWrap;
use crate::managers::error::RECOVERABLE_ERROR_CHANNEL;
use crate::managers::error::RecoverableError;
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
use embassy_sync::channel::Channel;
use embassy_sync::signal::Signal;
use embassy_time::Duration;
use embassy_time::Timer;
use embassy_time::with_timeout;
use embedded_io::ReadReady;

// TODO:
// Dual defmt/tcp logging.
//  Need to make it so that any info!, warn!, error!, writes not only to defmt,
//  but also sends over tcp as a log. obviously i can't see the jtag logging when
//  it's underwater. Less important in this file

pub static COMMS_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub static SENSOR_MESSAGE_QUEUE: Channel<CriticalSectionRawMutex, protocol::SensorData, 16> =
    Channel::new();
pub static ERROR_MESSAGE_QUEUE: Channel<CriticalSectionRawMutex, protocol::Log, 16> =
    Channel::new();
pub static LOG_MESSAGE_QUEUE: Channel<CriticalSectionRawMutex, protocol::Log, 16> = Channel::new();
pub static COMMAND_QUEUE: Channel<CriticalSectionRawMutex, protocol::Command, 16> = Channel::new();

const MAX_PAYLOAD_SIZE: usize = 1024;

#[embassy_executor::task]
pub async fn comms_manager_thread(p: CommsManagerPeripherals) {
    // TODO: switch all panics to use the error manager when that's implemented,
    // all panics NEED to trigger ascension
    // also the errors in loop probably shouldn't panic
    info!("Starting communication manager...");
    let dma_buf = UART_DMA_BUF.init([0; UART_RING_BUFFER_SIZE]);
    let (tx, rx) = init_uart(p).await;
    let mut manager = CommsManager::new(tx, rx.into_ring_buffered(dma_buf));

    if let Err(e) = manager.configure_modem().await {
        CRITICAL_ERROR_CHANNEL
            .send(CriticalError {
                message: "Modem configuration failed",
                error: ErrorWrap::At(e),
            })
            .await;
        core::future::pending::<()>().await;
    }

    loop {
        if let Err(e) = manager.check_uart().await {
            panic!("UART fetch failed: {}", e);
        }

        if !COMMS_READY.signaled() {
            continue;
        };

        defmt::dbg!("comms loop tick");

        if let Err(e) = manager.send_data(manager.write_type).await {
            panic!(
                "Sending data failed, on data type: {}, with error: {}",
                manager.write_type, e
            );
        }
        if manager.pending_write.is_none() {
            manager.write_type.next();
        }

        Timer::after_millis(10).await;
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
pub enum AtError {
    BufferFull,
    SendError(usart::Error),
    ReceiveError(usart::Error),
    CommandError,
    Timeout,
}

#[derive(PartialEq)]
enum WritePhase {
    AwaitingPrompt,
    PromptReceived,
}

#[derive(Clone, Copy, PartialEq, defmt::Format)]
enum WriteType {
    Sensor,
    Log,
    Error,
}

impl WriteType {
    fn next(&mut self) {
        match self {
            WriteType::Sensor => *self = WriteType::Log,
            WriteType::Log => *self = WriteType::Error,
            WriteType::Error => *self = WriteType::Sensor,
        }
    }
}

struct PendingWrite {
    kind: WriteType,
    phase: WritePhase,
    payload: [u8; MAX_PAYLOAD_SIZE],
    payload_length: usize,
}

struct CommsManager<'a> {
    uart_transmitter: UartTx<'a, Async>,
    uart_receiver: RingBufferedUartRx<'a>,
    pending_write: Option<PendingWrite>,
    write_type: WriteType,
    parse_buf: [u8; UART_RING_BUFFER_SIZE],
    parse_len: usize,
}

impl<'a> CommsManager<'a> {
    fn new(tx: UartTx<'a, Async>, rx: RingBufferedUartRx<'a>) -> Self {
        CommsManager {
            uart_transmitter: tx,
            uart_receiver: rx,
            pending_write: None,
            write_type: WriteType::Sensor,
            parse_buf: [0u8; UART_RING_BUFFER_SIZE],
            parse_len: 0,
        }
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
            if let Err(e) = self.send_at(*command, &mut response_buf).await {
                error!("Error sending AT command: {}, {}", command, e);
                return Err(e);
            }
        }

        info!("Modem Configured");

        Ok(())
    }

    // NOTE: this just blocks until a response arrives. if any URC arrives in
    // the meantime it will just drop it. be *very* careful where you use this
    // after initial configuration. at least once, for StartConnection, is
    // pretty much necessary, but be careful
    async fn send_at(
        &mut self,
        command: AtCommand,
        response_buf: &mut [u8],
    ) -> Result<(), AtError> {
        info!("Sending AT Command: {}", command);
        self.uart_transmitter
            .write(command.encode())
            .await
            .map_err(AtError::SendError)?;

        match with_timeout(
            command.timeout(),
            self.read_at_response(command, response_buf),
        )
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

    async fn read_at_response(
        &mut self,
        command: AtCommand,
        response_buf: &mut [u8],
    ) -> Result<usize, AtError> {
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
            } else if received.ends_with(b"\r\nERROR\r\n") {
                match from_utf8(received) {
                    Ok(resp) => error!("{} returned ERROR: {}", command, resp.trim_ascii()),
                    Err(_) => error!("{} returned non-UTF8 response", command),
                }

                return Err(AtError::CommandError);
            }
        }
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
            return Err(usart::Error::Overrun);
        }

        loop {
            let data = &self.parse_buf[..self.parse_len];
            if data.is_empty() {
                break;
            }

            let consumed = if data[0] == b'>' {
                if let Some(pw) = &mut self.pending_write {
                    pw.phase = WritePhase::PromptReceived;
                } else {
                    error!("unexpected '>' with no pending write");
                }

                1
            } else if data.starts_with(b"+IPD,") {
                // TODO: check length stuff, might overrun parse_buf
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
                // TODO: handle incoming tcp data
                // dispatch_tcp_data(&data[colon + 1..end]);
                end
            } else if let Some(eol) = data.windows(2).position(|w| w == b"\r\n") {
                // TODO: probably move some of this logic elsewhere

                match from_utf8(&data[..eol]) {
                    Ok(str) => info!("Non-TCP URC arrived: {}", str),
                    Err(_) => info!(
                        "Non-TCP URC arrived, failed UTF-8 conversion: {}",
                        data[..eol]
                    ),
                }

                if data.starts_with(b"+STA_CONNECTED") {
                    info!("Device connected to AP");
                    let mut response_buf = [0u8; 128];
                    if let Err(e) = self
                        .send_at(AtCommand::StartConnection, &mut response_buf)
                        .await
                    {
                        error!("Connection to server failed: {}", e);
                        // panic!("Server connection failed, aborting boot...");
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

    async fn send_data(&mut self, kind: WriteType) -> Result<(), usart::Error> {
        match &self.pending_write {
            Some(pw) if pw.kind != kind => return Ok(()),
            Some(pw) if pw.phase != WritePhase::PromptReceived => return Ok(()),
            Some(_) => {
                self.send_bytes().await?;
            }
            None => {
                // grab data from channel if available
                enum WriteData<'a> {
                    Sensor(protocol::SensorData),
                    Log(protocol::Log<'a>),
                    Error(protocol::Log<'a>),
                }

                let data = match kind {
                    WriteType::Sensor => match SENSOR_MESSAGE_QUEUE.try_receive() {
                        Ok(d) => WriteData::Sensor(d),
                        Err(_) => return Ok(()),
                    },
                    WriteType::Log => match LOG_MESSAGE_QUEUE.try_receive() {
                        Ok(d) => WriteData::Log(d),
                        Err(_) => return Ok(()),
                    },
                    WriteType::Error => match ERROR_MESSAGE_QUEUE.try_receive() {
                        Ok(d) => WriteData::Error(d),
                        Err(_) => return Ok(()),
                    },
                };
                // TODO: make sure if the channel is empty, just return

                // TODO: serialise data

                // TODO: init TCP write with cipsend AT command and payload_length

                self.pending_write = Some(PendingWrite {
                    kind,
                    phase: WritePhase::AwaitingPrompt,
                    payload: [0u8; MAX_PAYLOAD_SIZE], // TODO: put serialised data in here
                    payload_length: 0,                // TODO: and data length here
                });
            }
        }

        Ok(())
    }

    async fn send_bytes(&mut self) -> Result<(), usart::Error> {
        let Some(pw) = &self.pending_write else {
            return Ok(());
        };

        if pw.phase != WritePhase::PromptReceived {
            return Ok(()); // still waiting on '>'
        }

        self.uart_transmitter
            .write(&pw.payload[..pw.payload_length])
            .await?;

        self.pending_write = None;

        Ok(())
    }
}
