//! Everything ggez does beyond single commands: profile files, the
//! background helper that keeps a profile applied, and starting that helper
//! at login. Shared by the command-line tool and the graphical app.

pub mod autostart;
pub mod battery;
pub mod gg;
pub mod helper;
pub mod paths;
pub mod profile;
pub mod profiles;

pub use profile::Profile;
