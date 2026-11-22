//! High-level API for one connected mouse.

use crate::protocol::{self, Battery, DefaultLighting, Link, PollingRate, Rgb, Zone};
use crate::transport::Transport;
use crate::Result;

const READ_TIMEOUT_MS: i32 = 200;
const MAX_FOREIGN_REPLIES: usize = 16;

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

    fn write(&mut self, payload: Vec<u8>) -> Result<()> {
        let mut packet = Vec::with_capacity(payload.len() + 1);
        packet.push(0x00); // HID report ID
        packet.extend(payload);
        self.transport.write(&packet)?;
        Ok(())
    }

    /// Send a settings command. The dongle answers each one with a 64-byte
    /// report that has to be consumed, or it is mistaken for the reply to the
    /// next query.
    fn send(&mut self, payload: Vec<u8>) -> Result<()> {
        self.write(payload)?;
        if self.link == Link::Wireless {
            let mut ack = [0u8; 64];
            self.transport.read(&mut ack, READ_TIMEOUT_MS)?;
        }
        Ok(())
    }

    pub fn battery(&mut self) -> Result<Battery> {
        let query = protocol::battery_query(self.link);
        let command = query[0];
        self.write(query)?;
        // Other software (SteelSeries GG, if running) shares this interface,
        // so replies to its commands can arrive first. Replies echo the
        // command byte; skip the ones that are not ours.
        let mut buf = [0u8; 64];
        let mut n = 0;
        for _ in 0..MAX_FOREIGN_REPLIES {
            n = self.transport.read(&mut buf, READ_TIMEOUT_MS)?;
            if n == 0 || buf[0] == command {
                break;
            }
        }
        Ok(protocol::parse_battery(&buf[..n])?)
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
        t.responses.push_back(vec![0x6B; 64]); // ack for polling rate
        t.responses.push_back(vec![0xD2, 0x8B]); // battery: 50%, charging
        let mut m = Aerox9::new(t, Link::Wireless);
        m.set_polling_rate(PollingRate::Hz1000).unwrap();
        let b = m.battery().unwrap();
        assert_eq!((b.percent, b.charging), (50, true));
        assert_eq!(m.transport().written[1], vec![0x00, 0xD2]);
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
