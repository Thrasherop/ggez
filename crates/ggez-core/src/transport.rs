//! How bytes reach the mouse. [`MockTransport`] records packets so the whole
//! stack can be tested, and dry-run, without hardware.

use std::collections::VecDeque;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("no supported mouse found (is the Aerox 9 plugged in or its dongle connected?)")]
    NotFound,
    #[error("HID error: {0}")]
    Hid(String),
}

pub trait Transport {
    /// Write one output report. `packet[0]` is the HID report ID.
    fn write(&mut self, packet: &[u8]) -> Result<(), TransportError>;
    /// Read one input report, waiting up to `timeout_ms`. Returns the number
    /// of bytes read; 0 means nothing arrived in time.
    fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, TransportError>;
}

/// Records every written packet and replays queued responses.
#[derive(Debug, Default)]
pub struct MockTransport {
    pub written: Vec<Vec<u8>>,
    pub responses: VecDeque<Vec<u8>>,
    /// Acknowledge every write as accepted, the way the dongle does.
    pub auto_ack: bool,
}

impl MockTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn accepting() -> Self {
        Self {
            auto_ack: true,
            ..Self::default()
        }
    }
}

impl Transport for MockTransport {
    fn write(&mut self, packet: &[u8]) -> Result<(), TransportError> {
        self.written.push(packet.to_vec());
        if self.auto_ack {
            if let Some(&command) = packet.get(1) {
                self.responses.push_back(vec![command, 0x00]);
            }
        }
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, TransportError> {
        // Queued responses model replies that arrive after a write, so a
        // non-blocking read (used to drain stale input) sees nothing.
        if timeout_ms == 0 {
            return Ok(0);
        }
        match self.responses.pop_front() {
            Some(r) => {
                let n = r.len().min(buf.len());
                buf[..n].copy_from_slice(&r[..n]);
                Ok(n)
            }
            None => Ok(0),
        }
    }
}

#[cfg(feature = "hid")]
pub use hid::{list_steelseries, listen, HidInterface, HidTransport};

#[cfg(feature = "hid")]
mod hid {
    use std::{thread, time::Duration};

    use hidapi::HidApi;

    use super::{Transport, TransportError};
    use crate::protocol::{Link, CONFIG_INTERFACE, VENDOR_ID};

    /// The mouse needs a moment to process each command.
    const COMMAND_DELAY: Duration = Duration::from_millis(50);

    impl From<hidapi::HidError> for TransportError {
        fn from(e: hidapi::HidError) -> Self {
            TransportError::Hid(e.to_string())
        }
    }

    /// One HID interface of a SteelSeries device, for diagnostics.
    #[derive(Debug, Clone)]
    pub struct HidInterface {
        pub product_id: u16,
        pub interface: i32,
        pub usage_page: u16,
        pub usage: u16,
        pub product: String,
        pub path: String,
        /// Set if this is a mouse this crate can configure.
        pub link: Option<Link>,
    }

    /// Every HID interface exposed by any SteelSeries device.
    pub fn list_steelseries() -> Result<Vec<HidInterface>, TransportError> {
        let api = HidApi::new()?;
        Ok(api
            .device_list()
            .filter(|d| d.vendor_id() == VENDOR_ID)
            .map(|d| HidInterface {
                product_id: d.product_id(),
                interface: d.interface_number(),
                usage_page: d.usage_page(),
                usage: d.usage(),
                product: d.product_string().unwrap_or("").to_string(),
                path: d.path().to_string_lossy().into_owned(),
                link: Link::from_product_id(d.product_id()),
            })
            .collect())
    }

    /// Print-style listener for protocol research: calls `on_report` with the
    /// interface number and bytes of every input report that arrives on any
    /// vendor-defined interface of a supported mouse, for `duration`.
    pub fn listen(
        duration: Duration,
        mut on_report: impl FnMut(i32, &[u8]),
    ) -> Result<(), TransportError> {
        let api = HidApi::new()?;
        let mut devices = Vec::new();
        for d in api.device_list() {
            let supported = d.vendor_id() == VENDOR_ID && Link::from_product_id(d.product_id()).is_some();
            if supported && d.usage_page() >= 0xFF00 {
                let device = d.open_device(&api)?;
                device.set_blocking_mode(false)?;
                devices.push((d.interface_number(), device));
            }
        }
        if devices.is_empty() {
            return Err(TransportError::NotFound);
        }
        let end = std::time::Instant::now() + duration;
        let mut buf = [0u8; 64];
        while std::time::Instant::now() < end {
            let mut idle = true;
            for (interface, device) in &devices {
                let n = device.read(&mut buf)?;
                if n > 0 {
                    idle = false;
                    on_report(*interface, &buf[..n]);
                }
            }
            if idle {
                thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    }

    pub struct HidTransport {
        device: hidapi::HidDevice,
    }

    impl HidTransport {
        /// Open the first supported mouse. A cabled mouse is preferred over a
        /// dongle, since the dongle stays plugged in while the mouse charges.
        pub fn open() -> Result<(Self, Link), TransportError> {
            let api = HidApi::new()?;
            let mut best = None;
            for d in api.device_list() {
                if d.vendor_id() != VENDOR_ID || d.interface_number() != CONFIG_INTERFACE {
                    continue;
                }
                match Link::from_product_id(d.product_id()) {
                    Some(Link::Wired) => {
                        best = Some((d, Link::Wired));
                        break;
                    }
                    Some(Link::Wireless) if best.is_none() => best = Some((d, Link::Wireless)),
                    _ => {}
                }
            }
            let (info, link) = best.ok_or(TransportError::NotFound)?;
            let device = info.open_device(&api)?;
            Ok((Self { device }, link))
        }
    }

    impl Transport for HidTransport {
        fn write(&mut self, packet: &[u8]) -> Result<(), TransportError> {
            self.device.write(packet)?;
            thread::sleep(COMMAND_DELAY);
            Ok(())
        }

        fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize, TransportError> {
            Ok(self.device.read_timeout(buf, timeout_ms)?)
        }
    }
}
