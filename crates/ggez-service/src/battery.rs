//! When a battery reading is worth interrupting someone for.

/// At or below this level, off the charger, the user is told once.
pub const LOW_PERCENT: u8 = 15;
/// The low warning is armed again once the level is back above this, so a
/// reading that hovers around the threshold does not warn repeatedly.
const REARM_PERCENT: u8 = 25;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryNotice {
    /// The battery is low; the level in percent.
    Low(u8),
    /// A charge that was seen in progress has finished.
    Full,
}

/// Follows battery readings and says when to notify. The mouse reports its
/// level in steps of 5%.
#[derive(Debug, Default)]
pub struct BatteryWatch {
    warned_low: bool,
    /// The mouse has been seen charging below 100%, so reaching 100% is news.
    charging_seen: bool,
}

impl BatteryWatch {
    pub fn update(&mut self, percent: u8, charging: bool) -> Option<BatteryNotice> {
        if charging {
            self.warned_low = false;
            if percent < 100 {
                self.charging_seen = true;
            } else if std::mem::take(&mut self.charging_seen) {
                return Some(BatteryNotice::Full);
            }
            return None;
        }
        self.charging_seen = false;
        if percent >= REARM_PERCENT {
            self.warned_low = false;
        }
        if percent <= LOW_PERCENT && !self.warned_low {
            self.warned_low = true;
            return Some(BatteryNotice::Low(percent));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn low_battery_is_announced_once_per_discharge() {
        let mut watch = BatteryWatch::default();
        assert_eq!(watch.update(40, false), None);
        assert_eq!(watch.update(15, false), Some(BatteryNotice::Low(15)));
        assert_eq!(watch.update(10, false), None);
        // Hovering just above the threshold does not arm it again.
        assert_eq!(watch.update(20, false), None);
        assert_eq!(watch.update(15, false), None);
        // A charge does.
        assert_eq!(watch.update(15, true), None);
        assert_eq!(watch.update(10, false), Some(BatteryNotice::Low(10)));
    }

    #[test]
    fn a_finished_charge_is_announced_only_if_it_was_seen_in_progress() {
        let mut watch = BatteryWatch::default();
        // Already full when first seen: nothing to say.
        assert_eq!(watch.update(100, true), None);
        assert_eq!(watch.update(100, false), None);
        assert_eq!(watch.update(60, true), None);
        assert_eq!(watch.update(95, true), None);
        assert_eq!(watch.update(100, true), Some(BatteryNotice::Full));
        assert_eq!(watch.update(100, true), None);
    }

    #[test]
    fn a_mouse_that_starts_out_low_is_announced() {
        let mut watch = BatteryWatch::default();
        assert_eq!(watch.update(5, false), Some(BatteryNotice::Low(5)));
    }
}
