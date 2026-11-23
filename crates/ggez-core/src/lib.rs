//! Device layer for the SteelSeries Aerox 9 Wireless.
//!
//! - [`protocol`]: pure packet encoding/decoding, no I/O.
//! - [`transport`]: the [`transport::Transport`] trait, a mock for tests and
//!   dry runs, and (with the `hid` feature) a real hidapi transport.
//! - [`device`]: the high-level [`device::Aerox9`] API.
//!
//! Protocol knowledge comes from the rivalcfg project
//! (<https://github.com/flozz/rivalcfg>).

pub mod buttons;
pub mod device;
mod dpi_table;
pub mod protocol;
pub mod transport;

pub use buttons::{key_names, media_names, Action, Button, ButtonMap, Host, PRIMARY};
pub use device::Aerox9;
pub use protocol::{Battery, DefaultLighting, Link, PollingRate, ProtoError, Rgb, Zone};
pub use transport::{MockTransport, Transport, TransportError};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Proto(#[from] ProtoError),
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error("the mouse did not acknowledge command {0:#04x} (is it switched on and in range? Closing SteelSeries GG also helps)")]
    NoAck(u8),
    #[error("the dongle has no link to the mouse (is it switched on, awake and in range?)")]
    Offline,
    #[error("the mouse rejected command {command:#04x} with status {status:#04x}")]
    Rejected { command: u8, status: u8 },
}

pub type Result<T> = std::result::Result<T, Error>;
