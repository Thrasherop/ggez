//! Device layer for the SteelSeries Aerox 9 Wireless.
//!
//! - [`protocol`]: pure packet encoding/decoding, no I/O.
//! - [`transport`]: the [`transport::Transport`] trait, a mock for tests and
//!   dry runs, and (with the `hid` feature) a real hidapi transport.
//! - [`device`]: the high-level [`device::Aerox9`] API.
//!
//! Protocol knowledge comes from the rivalcfg project
//! (<https://github.com/flozz/rivalcfg>).

pub mod device;
mod dpi_table;
pub mod protocol;
pub mod transport;

pub use device::Aerox9;
pub use protocol::{Battery, DefaultLighting, Link, PollingRate, ProtoError, Rgb, Zone};
pub use transport::{MockTransport, Transport, TransportError};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Proto(#[from] ProtoError),
    #[error(transparent)]
    Transport(#[from] TransportError),
}

pub type Result<T> = std::result::Result<T, Error>;
