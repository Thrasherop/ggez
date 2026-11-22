//! High-level API for one connected mouse.

use std::time::{Duration, Instant};

use crate::protocol::{self, Battery, DefaultLighting, Link, PollingRate, Rgb, Zone};
use crate::transport::Transport;
use crate::{Error, Result};

const READ_TIMEOUT_MS: i32 = 200;
/// How long to wait for a reply before resending the command. Saving to
/// onboard memory takes the mouse up to half a second to acknowledge.
const REPLY_WAIT: Duration = Duration::from_millis(1000);
const ATTEMPTS: usize = 4;
/// Upper bound on stale input reports discarded before a command.
const MAX_STALE_REPORTS: usize = 256;

pub struct Aerox9<T: Transport> {
    transport: T,
    link: Link,
}

#[cfg(feature = "hid")]
impl Aerox9<crate::transport::HidTransport> {
    /// Open the first supported mouse found on this machine.
    pub fn open() -> Result<Self> {
        let (transport, link) = crate::transport::HidTransport::open()?;
        Ok(Self::new(transport, link))
    }
}

impl<T: Transport> Aerox9<T> {
    pub fn new(transport: T, link: Link) -> Self {
        Self { transport, link }
    }

    pub fn link(&self) -> Link {
        self.link
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Discard input reports that queued up before our command. The OS keeps
    /// only a small queue per device, so if stale replies (GG's, typically)
    /// fill it, the reply we are about to wait for gets dropped.
    fn drain(&mut self) -> Result<()> {
        let mut buf = [0u8; 64];
        for _ in 0..MAX_STALE_REPORTS {
            if self.transport.read(&mut buf, 0)? == 0 {
                break;
            }
        }
        Ok(())
    }

    fn write(&mut self, payload: Vec<u8>) -> Result<()> {
        let mut packet = Vec::with_capacity(payload.len() + 1);
        packet.push(0x00); // HID report ID
        packet.extend(payload);
        self.transport.write(&packet)?;
        Ok(())
    }

    /// Send a settings command. Over the dongle, each one is answered with
    /// `[command, status]` where status 0 means accepted; wait for it so a
    /// rejected or lost command is reported instead of silently ignored.
    /// The cable sends no acknowledgement.
    fn send(&mut self, payload: Vec<u8>) -> Result<()> {
        if self.link == Link::Wired {
            return self.write(payload);
        }
        let command = payload[0];
        match self.request(payload)?.get(1) {
            Some(0) => Ok(()),
            status => Err(Error::Rejected {
                command,
                status: status.copied().unwrap_or(0xFF),
            }),
        }
    }

    /// Send a command and wait for the reply that echoes its command byte.
    ///
    /// Other software (SteelSeries GG, if running) shares this interface and
    /// can crowd our reply out of the OS's small input queue, so replies that
    /// are not ours are skipped and the command is resent if none arrives.
    /// Every command here is idempotent, so resending is safe.
    fn request(&mut self, payload: Vec<u8>) -> Result<Vec<u8>> {
        let command = payload[0];
        let mut buf = [0u8; 64];
        let mut link_down = false;
        for _ in 0..ATTEMPTS {
            self.drain()?;
            self.write(payload.clone())?;
            let deadline = Instant::now() + REPLY_WAIT;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                let n = self.transport.read(&mut buf, left.as_millis().max(1) as i32)?;
                if n == 0 {
                    break;
                }
                if buf[0] == command {
                    return Ok(buf[..n].to_vec());
                }
                // The dongle answers `40 ff` itself when it has no radio link
                // to the mouse (switched off, asleep or out of range).
                if n >= 2 && buf[..2] == [0x40, 0xFF] {
                    link_down = true;
                }
            }
        }
        Err(if link_down {
            Error::Offline
        } else {
            Error::NoAck(command)
        })
    }

    /// Send `payload` exactly as given (no wireless flag is added) and collect
    /// up to `max_replies` input reports. For protocol research only: the
    /// mouse will act on whatever it is sent.
    pub fn raw(&mut self, payload: Vec<u8>, max_replies: usize) -> Result<Vec<Vec<u8>>> {
        self.write(payload)?;
        let mut replies = Vec::new();
        for _ in 0..max_replies {
            let mut buf = [0u8; 64];
            let n = self.transport.read(&mut buf, READ_TIMEOUT_MS)?;
            if n == 0 {
                break;
            }
            replies.push(buf[..n].to_vec());
        }
        Ok(replies)
    }

    /// The mouse's firmware version, e.g. `1.2.4`.
    pub fn firmware_version(&mut self) -> Result<String> {
        let query = protocol::firmware_query(self.link);
        let command = query[0];
        let mut buf = [0u8; 64];
        for _ in 0..ATTEMPTS {
            self.drain()?;
            self.write(query.clone())?;
            let deadline = Instant::now() + REPLY_WAIT;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break;
                }
                let n = self.transport.read(&mut buf, left.as_millis().max(1) as i32)?;
                if n == 0 {
                    break;
                }
                if let Some(version) = protocol::parse_firmware(&buf[..n]) {
                    return Ok(version);
                }
            }
        }
        Err(Error::NoAck(command))
    }

    pub fn battery(&mut self) -> Result<Battery> {
        let reply = self.request(protocol::battery_query(self.link))?;
        Ok(protocol::parse_battery(&reply)?)
    }

    pub fn set_dpi(&mut self, presets: &[u32], selected: usize) -> Result<()> {
        self.send(protocol::set_dpi(self.link, presets, selected)?)
    }

    pub fn set_polling_rate(&mut self, rate: PollingRate) -> Result<()> {
        self.send(protocol::set_polling_rate(self.link, rate))
    }

    pub fn set_zone_color(&mut self, zone: Zone, color: Rgb) -> Result<()> {
        self.send(protocol::set_zone_color(self.link, zone, color))
    }

    pub fn set_reactive_color(&mut self, color: Option<Rgb>) -> Result<()> {
        self.send(protocol::set_reactive_color(self.link, color))
    }

    pub fn set_sleep_timer(&mut self, minutes: u32) -> Result<()> {
        self.send(protocol::set_sleep_timer(self.link, minutes)?)
    }

    pub fn set_dim_timer(&mut self, seconds: u32) -> Result<()> {
        self.send(protocol::set_dim_timer(self.link, seconds)?)
    }

    pub fn rainbow_effect(&mut self) -> Result<()> {
        self.send(protocol::rainbow_effect(self.link))
    }

    pub fn set_default_lighting(&mut self, mode: DefaultLighting) -> Result<()> {
        self.send(protocol::set_default_lighting(self.link, mode))
    }

    /// Replace every button binding with `map`.
    pub fn set_buttons(&mut self, map: &crate::buttons::ButtonMap) -> Result<()> {
        for packet in map.packets(self.link)? {
            self.send(packet)?;
        }
        Ok(())
    }

    /// Persist current settings to onboard memory. Without this, settings are
    /// lost when the mouse powers off.
    pub fn save(&mut self) -> Result<()> {
        self.send(protocol::save(self.link))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::MockTransport;

    #[test]
    fn packets_get_report_id_prefix() {
        let mut m = Aerox9::new(MockTransport::new(), Link::Wired);
        m.set_polling_rate(PollingRate::Hz500).unwrap();
        m.save().unwrap();
        assert_eq!(
            m.transport().written,
            vec![vec![0x00, 0x2B, 0x01], vec![0x00, 0x11, 0x00]]
        );
    }

    #[test]
    fn wireless_ack_is_not_mistaken_for_battery_reply() {
        let mut t = MockTransport::new();
        t.responses.push_back(vec![0x61, 0x00]); // reply to someone else
        t.responses.push_back(vec![0x6B, 0x00]); // ack for polling rate
        t.responses.push_back(vec![0xD2, 0x8B]); // battery: 50%, charging
        let mut m = Aerox9::new(t, Link::Wireless);
        m.set_polling_rate(PollingRate::Hz1000).unwrap();
        let b = m.battery().unwrap();
        assert_eq!((b.percent, b.charging), (50, true));
        assert_eq!(m.transport().written[1], vec![0x00, 0xD2]);
    }

    #[test]
    fn wireless_reports_missing_and_rejected_acks() {
        let mut m = Aerox9::new(MockTransport::new(), Link::Wireless);
        assert!(matches!(m.save(), Err(Error::NoAck(0x51))));

        let mut t = MockTransport::new();
        t.responses.push_back(vec![0x51, 0xFF]);
        let mut m = Aerox9::new(t, Link::Wireless);
        assert!(matches!(
            m.save(),
            Err(Error::Rejected { command: 0x51, status: 0xFF })
        ));
    }

    #[test]
    fn dongle_without_mouse_reports_offline() {
        let mut t = MockTransport::new();
        t.responses.push_back(vec![0x40, 0xFF, 0x00]);
        let mut m = Aerox9::new(t, Link::Wireless);
        assert!(matches!(m.battery(), Err(Error::Offline)));
    }

    #[test]
    fn firmware_version_skips_other_replies() {
        let mut t = MockTransport::new();
        t.responses.push_back(vec![0x61, 0x00, 0x32, 0x2E, 0x34]);
        t.responses.push_back(vec![0x31, 0x2E, 0x32, 0x2E, 0x34, 0x00, 0x00]);
        let mut m = Aerox9::new(t, Link::Wireless);
        assert_eq!(m.firmware_version().unwrap(), "1.2.4");
        assert_eq!(m.transport().written[0], vec![0x00, 0xD0]);
    }

    #[test]
    fn battery_skips_replies_meant_for_other_software() {
        let mut t = MockTransport::new();
        t.responses.push_back(vec![0x61, 0x00, 0x32]);
        t.responses.push_back(vec![0xD2, 0x0B]);
        let mut m = Aerox9::new(t, Link::Wireless);
        assert_eq!(m.battery().unwrap().percent, 50);
    }

    #[test]
    fn battery_with_no_reply_is_an_error() {
        let mut m = Aerox9::new(MockTransport::new(), Link::Wired);
        assert!(m.battery().is_err());
    }

    #[test]
    fn invalid_settings_send_nothing() {
        let mut m = Aerox9::new(MockTransport::new(), Link::Wired);
        assert!(m.set_dpi(&[50], 0).is_err());
        assert!(m.transport().written.is_empty());
    }
}
