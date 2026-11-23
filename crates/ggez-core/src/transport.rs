//! How bytes reach the mouse. [`MockTransport`] records packets so the whole
//! stack can be tested, and dry-run, without hardware.

use std::collections::VecDeque;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("no supported mouse found (is the Aerox 9 plugged in or its dongle connected?)")]
    NotFound,
    #[error("the mouse was found, but this computer would not open it: {0}")]
    Blocked(String),
    #[error("HID error: {0}")]
    Hid(String),
}

/// How sure it is that a HID interface is the one settings are sent to,
/// from what the system says about it: higher is surer, `None` is not it.
///
/// Windows and Linux number a device's interfaces. macOS does not always
/// (hidapi then reports -1), so the usage page has to be enough there.
pub fn config_match(interface: i32, usage_page: u16) -> Option<u8> {
    use crate::protocol::{CONFIG_INTERFACE, CONFIG_USAGE_PAGE};
    match (
        interface == CONFIG_INTERFACE,
        usage_page == CONFIG_USAGE_PAGE,
    ) {
        (true, true) => Some(2),
        (true, false) => Some(1),
        (false, true) if interface < 0 => Some(1),
        _ => None,
    }
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

    use super::{config_match, Transport, TransportError};
    use crate::protocol::{Link, VENDOR_ID};

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
        /// For the interface settings would be sent to: whether it opens,
        /// and why not if it does not. `None` for every other interface.
        pub opens: Option<Result<(), String>>,
    }

    /// Every HID interface exposed by any SteelSeries device.
    pub fn list_steelseries() -> Result<Vec<HidInterface>, TransportError> {
        let api = HidApi::new()?;
        Ok(api
            .device_list()
            .filter(|d| d.vendor_id() == VENDOR_ID)
            .map(|d| {
                let link = Link::from_product_id(d.product_id());
                let settings =
                    link.is_some() && config_match(d.interface_number(), d.usage_page()).is_some();
                HidInterface {
                    product_id: d.product_id(),
                    interface: d.interface_number(),
                    usage_page: d.usage_page(),
                    usage: d.usage(),
                    product: d.product_string().unwrap_or("").to_string(),
                    path: d.path().to_string_lossy().into_owned(),
                    link,
                    opens: settings
                        .then(|| d.open_device(&api).map(|_| ()).map_err(|e| e.to_string())),
                }
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
            let supported =
                d.vendor_id() == VENDOR_ID && Link::from_product_id(d.product_id()).is_some();
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
            let mut found = Vec::new();
            for d in api.device_list() {
                if d.vendor_id() != VENDOR_ID {
                    continue;
                }
                let Some(link) = Link::from_product_id(d.product_id()) else {
                    continue;
                };
                if let Some(sure) = config_match(d.interface_number(), d.usage_page()) {
                    found.push((link == Link::Wired, sure, d, link));
                }
            }
            // The cable first, then whichever the system is surest about.
            found.sort_by_key(|(wired, sure, ..)| std::cmp::Reverse((*wired, *sure)));
            let mut refused = None;
            for (_, _, info, link) in found {
                match info.open_device(&api) {
                    Ok(device) => return Ok((Self { device }, link)),
                    // Keep the first reason: it is about the likeliest one.
                    Err(e) => refused = refused.or(Some(e.to_string())),
                }
            }
            Err(match refused {
                Some(reason) => TransportError::Blocked(reason),
                None => TransportError::NotFound,
            })
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

#[cfg(test)]
mod tests {
    use super::config_match;

    #[test]
    fn the_settings_interface_is_known_by_number_and_usage_page() {
        // Windows and Linux: interface 3, usage page FFC0.
        assert_eq!(config_match(3, 0xFFC0), Some(2));
        // A system that does not report usage pages.
        assert_eq!(config_match(3, 0), Some(1));
    }

    #[test]
    fn a_system_that_does_not_number_interfaces_goes_by_usage_page() {
        assert_eq!(config_match(-1, 0xFFC0), Some(1));
        // The other vendor interface, and the ordinary mouse and keyboard.
        assert_eq!(config_match(-1, 0xFFC1), None);
        assert_eq!(config_match(-1, 0x0001), None);
    }

    #[test]
    fn other_interfaces_are_not_it() {
        assert_eq!(config_match(0, 0x0001), None);
        assert_eq!(config_match(4, 0xFFC1), None);
        // A numbered interface is believed over a usage page.
        assert_eq!(config_match(4, 0xFFC0), None);
    }
}
