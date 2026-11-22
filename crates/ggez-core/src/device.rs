//! High-level API for one connected mouse.

use std::time::{Duration, Instant};

use crate::protocol::{self, Battery, DefaultLighting, Link, PollingRate, Rgb, Zone};
use crate::transport::Transport;
use crate::{Error, Result};

const READ_TIMEOUT_MS: i32 = 200;
/// How long to wait for a reply before each resend, in milliseconds. Most
/// replies arrive within 20 ms, so early resends are quick: with SteelSeries
/// GG streaming commands through the same dongle, well over half of ours
/// can go unanswered. Saving to onboard memory takes up to half a second to
/// acknowledge; a late reply to an earlier send still counts.
const REPLY_WAITS_MS: [u64; 12] = [150, 150, 150, 150, 150, 150, 300, 300, 300, 700, 700, 700];
/// Upper bound on stale input reports discarded before a command.
const MAX_STALE_REPORTS: usize = 256;
/// Changing the polling rate makes the mouse drop its radio link and join
/// the dongle again. Measured: 2 seconds beside the dongle, more than 9
/// through two walls. The command that follows waits this long for it.
const RELINK_WAIT: Duration = Duration::from_secs(20);

pub struct Aerox9<T: Transport> {
    transport: T,
    link: Link,
    /// The last command made the mouse rejoin the dongle, so the next one
    /// should outwait "no link" answers instead of reporting them.
    relinking: bool,
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
        Self {
            transport,
            link,
            relinking: false,
        }
    }

    pub fn link(&self) -> Link {
        self.link
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
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

    /// Send a settings command. Each one is answered with `[command, status]`
    /// where status 0 means accepted, over the cable as well as the dongle;
    /// wait for it so a rejected or lost command is reported instead of
    /// silently ignored.
    fn send(&mut self, payload: Vec<u8>) -> Result<()> {
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
    fn request(&mut self, payload: Vec<u8>) -> Result<Vec<u8>> {
        let command = payload[0];
        self.exchange(payload, |reply| reply[0] == command)
    }

    /// Send a command and wait for the reply that `is_reply` recognises.
    ///
    /// Other software (SteelSeries GG, if running) shares this interface and
    /// can crowd our reply out of the OS's small input queue, so replies that
    /// are not ours are skipped and the command is resent if none arrives.
    /// Every command here is idempotent, so resending is safe.
    fn exchange(&mut self, payload: Vec<u8>, is_reply: impl Fn(&[u8]) -> bool) -> Result<Vec<u8>> {
        let command = payload[0];
        let mut buf = [0u8; 64];
        let mut link_down = false;
        let patient = std::mem::take(&mut self.relinking);
        let give_up = Instant::now() + RELINK_WAIT;
        self.drain()?;
        loop {
            for wait in REPLY_WAITS_MS {
                self.write(payload.clone())?;
                let deadline = Instant::now() + Duration::from_millis(wait);
                loop {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    let n = self
                        .transport
                        .read(&mut buf, left.as_millis().max(1) as i32)?;
                    if n == 0 {
                        break;
                    }
                    if is_reply(&buf[..n]) {
                        return Ok(buf[..n].to_vec());
                    }
                    // The dongle answers `40 ff` itself when it has no radio
                    // link to the mouse (switched off, asleep, out of range,
                    // or rejoining after a polling rate change).
                    if n >= 2 && buf[..2] == [0x40, 0xFF] {
                        link_down = true;
                    }
                }
            }
            // Only a link we know is coming back is worth waiting for.
            if !(patient && link_down && Instant::now() < give_up) {
                break;
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

    /// Whether the mouse can be reached right now. Over the dongle this asks
    /// for the radio link state, which the dongle answers without waking the
    /// mouse. A cabled mouse is asked for its battery level: any answer
    /// means it is there.
    pub fn is_online(&mut self) -> Result<bool> {
        if self.link == Link::Wired {
            return match self.request(protocol::battery_query(self.link)) {
                Ok(_) => Ok(true),
                Err(Error::NoAck(_)) => Ok(false),
                Err(e) => Err(e),
            };
        }
        let reply = self.request(protocol::link_status_query())?;
        Ok(reply.get(1) == Some(&1))
    }

    /// The mouse's firmware version, e.g. `1.2.4`.
    pub fn firmware_version(&mut self) -> Result<String> {
        // This reply is the version text itself; it does not echo the command.
        let query = protocol::firmware_query(self.link);
        let command = query[0];
        let reply = self.exchange(query, |reply| protocol::parse_firmware(reply).is_some())?;
        protocol::parse_firmware(&reply).ok_or(Error::NoAck(command))
    }

    pub fn battery(&mut self) -> Result<Battery> {
        let reply = self.request(protocol::battery_query(self.link))?;
        Ok(protocol::parse_battery(&reply)?)
    }

    pub fn set_dpi(&mut self, presets: &[u32], selected: usize) -> Result<()> {
        self.send(protocol::set_dpi(self.link, presets, selected)?)
    }

    pub fn set_polling_rate(&mut self, rate: PollingRate) -> Result<()> {
        self.send(protocol::set_polling_rate(self.link, rate))?;
        self.relinking = self.link == Link::Wireless;
        Ok(())
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

    pub fn set_illumination(&mut self, lights: protocol::Illumination) -> Result<()> {
        self.send(protocol::set_illumination(self.link, lights)?)
    }

    pub fn set_high_efficiency(&mut self, on: bool) -> Result<()> {
        self.send(protocol::set_high_efficiency(self.link, on))
    }

    pub fn rainbow_effect(&mut self) -> Result<()> {
        self.send(protocol::rainbow_effect(self.link))
    }

    pub fn set_default_lighting(&mut self, mode: DefaultLighting) -> Result<()> {
        self.send(protocol::set_default_lighting(self.link, mode))
    }

    /// Replace every button binding with `map`.
    pub fn set_buttons(&mut self, map: &crate::buttons::ButtonMap) -> Result<()> {
        for packet in map.packets(self.link) {
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
        let mut m = Aerox9::new(MockTransport::accepting(), Link::Wired);
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
            Err(Error::Rejected {
                command: 0x51,
                status: 0xFF
            })
        ));
    }

    /// A dongle that answers "no link" to the first `down_for` commands sent
    /// after a polling rate change, as the real one does while the mouse
    /// rejoins it.
    struct Rejoining {
        down_for: usize,
        reply: Option<Vec<u8>>,
    }

    impl Transport for Rejoining {
        fn write(&mut self, packet: &[u8]) -> std::result::Result<(), crate::TransportError> {
            let command = packet[1];
            self.reply = Some(if command == 0x6B || self.down_for == 0 {
                vec![command, 0x00]
            } else {
                self.down_for -= 1;
                vec![0x40, 0xFF]
            });
            Ok(())
        }

        fn read(
            &mut self,
            buf: &mut [u8],
            timeout_ms: i32,
        ) -> std::result::Result<usize, crate::TransportError> {
            if timeout_ms == 0 {
                return Ok(0);
            }
            Ok(self.reply.take().map_or(0, |r| {
                buf[..r.len()].copy_from_slice(&r);
                r.len()
            }))
        }
    }

    #[test]
    fn the_command_after_a_polling_change_waits_for_the_mouse_to_rejoin() {
        // More "no link" answers than one round of resends covers.
        let down_for = REPLY_WAITS_MS.len() + 5;
        let rejoining = |down_for| Rejoining {
            down_for,
            reply: None,
        };
        let mut m = Aerox9::new(rejoining(down_for), Link::Wireless);
        m.set_polling_rate(PollingRate::Hz500).unwrap();
        m.save().unwrap();

        // Without a polling change the same silence means the mouse is away.
        let mut m = Aerox9::new(rejoining(down_for), Link::Wireless);
        assert!(matches!(m.save(), Err(Error::Offline)));

        // Over the cable there is no radio link to wait for.
        let mut m = Aerox9::new(rejoining(0), Link::Wired);
        m.set_polling_rate(PollingRate::Hz500).unwrap();
        assert!(!m.relinking);
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
        t.responses
            .push_back(vec![0x31, 0x2E, 0x32, 0x2E, 0x34, 0x00, 0x00]);
        let mut m = Aerox9::new(t, Link::Wireless);
        assert_eq!(m.firmware_version().unwrap(), "1.2.4");
        assert_eq!(m.transport().written[0], vec![0x00, 0xD0]);
    }

    #[test]
    fn dongle_reports_link_state() {
        let mut t = MockTransport::new();
        t.responses.push_back(vec![0xBC, 0x00]);
        t.responses.push_back(vec![0xBC, 0x01]);
        let mut m = Aerox9::new(t, Link::Wireless);
        assert!(!m.is_online().unwrap());
        assert!(m.is_online().unwrap());
        assert_eq!(m.transport().written[0], vec![0x00, 0xBC]);
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
