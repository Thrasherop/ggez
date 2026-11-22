use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use ggez_core::transport::{list_steelseries, HidTransport};
use ggez_core::{Aerox9, DefaultLighting, Link, MockTransport, PollingRate, Rgb, Transport, Zone};

/// Configure a SteelSeries Aerox 9 Wireless without SteelSeries GG.
#[derive(Parser)]
#[command(name = "ggez", version)]
struct Cli {
    /// Print the packets that would be sent instead of touching hardware.
    #[arg(long, global = true, value_name = "LINK")]
    dry_run: Option<LinkArg>,

    /// Apply the setting for now only; it is lost when the mouse powers off.
    #[arg(long, global = true)]
    no_save: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum LinkArg {
    Wired,
    Wireless,
}

#[derive(Clone, Copy, ValueEnum)]
enum LightingArg {
    Off,
    Reactive,
    Rainbow,
    ReactiveRainbow,
}

#[derive(Subcommand)]
enum Command {
    /// List every SteelSeries HID interface on this machine.
    List,
    /// Show battery level and charging state.
    Battery,
    /// Set DPI presets, e.g. `ggez dpi 800,1600,3200 --select 2`.
    Dpi {
        /// 1 to 5 values from 100 to 18000, comma separated.
        #[arg(value_delimiter = ',', required = true)]
        presets: Vec<u32>,
        /// Which preset to make active (1 = first).
        #[arg(long, default_value_t = 1)]
        select: usize,
    },
    /// Set the polling rate in Hz: 125, 250, 500 or 1000.
    Polling { hz: u32 },
    /// Set LED zone colors (#rrggbb or a color name).
    Color {
        /// Color for all three zones.
        #[arg(long)]
        all: Option<Rgb>,
        #[arg(long)]
        top: Option<Rgb>,
        #[arg(long)]
        middle: Option<Rgb>,
        #[arg(long)]
        bottom: Option<Rgb>,
    },
    /// Set the color flashed on click, or `off`.
    Reactive { color: String },
    /// Turn on the rainbow effect.
    Rainbow,
    /// Set the lighting shown when the mouse powers on.
    DefaultLighting { mode: LightingArg },
    /// Minutes of idle before the mouse sleeps (0-20, 0 = never).
    Sleep { minutes: u32 },
    /// Seconds of idle before the LEDs dim (0-1200, 0 = never).
    Dim { seconds: u32 },
    /// Save current settings to the mouse's onboard memory.
    Save,
    /// Protocol research: send raw hex bytes and print the replies.
    /// The mouse acts on whatever it receives, so only send known commands.
    Raw {
        /// Payload bytes in hex, e.g. `d2` or `6b 00`.
        #[arg(required = true)]
        bytes: Vec<String>,
        /// How many replies to wait for.
        #[arg(long, default_value_t = 4)]
        replies: usize,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Command::List = cli.command {
        return list();
    }

    match cli.dry_run {
        Some(link) => {
            let link = match link {
                LinkArg::Wired => Link::Wired,
                LinkArg::Wireless => Link::Wireless,
            };
            let mut mouse = Aerox9::new(MockTransport::accepting(), link);
            if let Command::Battery = cli.command {
                bail!("battery cannot be dry-run: it needs a reply from the mouse");
            }
            run(&mut mouse, &cli)?;
            for packet in &mouse.transport().written {
                println!("{}", hex(packet));
            }
        }
        None => {
            let (transport, link) = HidTransport::open()?;
            let mut mouse = Aerox9::new(transport, link);
            run(&mut mouse, &cli)?;
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let parts: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    parts.join(" ")
}

fn list() -> Result<()> {
    let interfaces = list_steelseries()?;
    if interfaces.is_empty() {
        println!("No SteelSeries HID devices found.");
    }
    for i in interfaces {
        let support = match i.link {
            Some(Link::Wired) => "Aerox 9 (wired)",
            Some(Link::Wireless) => "Aerox 9 (2.4 GHz dongle)",
            None => "unsupported",
        };
        println!(
            "1038:{:04x} if={} usage={:04x}:{:04x} {:?} [{}]\n    {}",
            i.product_id, i.interface, i.usage_page, i.usage, i.product, support, i.path
        );
    }
    Ok(())
}

fn run<T: Transport>(mouse: &mut Aerox9<T>, cli: &Cli) -> Result<()> {
    match &cli.command {
        Command::List => unreachable!("handled in main"),
        Command::Battery => {
            let b = mouse
                .battery()
                .context("could not read battery (is the mouse switched on?)")?;
            let state = if b.charging { ", charging" } else { "" };
            println!("{}%{}", b.percent, state);
            return Ok(());
        }
        Command::Save => return Ok(mouse.save()?),
        Command::Raw { bytes, replies } => {
            let payload = bytes
                .iter()
                .map(|b| u8::from_str_radix(b, 16).with_context(|| format!("bad hex byte {b:?}")))
                .collect::<Result<Vec<u8>>>()?;
            for reply in mouse.raw(payload, *replies)? {
                // Trailing zero padding carries no information.
                let len = reply.iter().rposition(|&b| b != 0).map_or(1, |i| i + 1);
                println!("{}", hex(&reply[..len]));
            }
            return Ok(());
        }
        Command::Dpi { presets, select } => {
            if *select == 0 {
                bail!("--select counts from 1");
            }
            mouse.set_dpi(presets, select - 1)?;
        }
        Command::Polling { hz } => {
            let rate = PollingRate::from_hz(*hz)
                .with_context(|| format!("unsupported polling rate {hz} (use 125, 250, 500, 1000)"))?;
            mouse.set_polling_rate(rate)?;
        }
        Command::Color { all, top, middle, bottom } => {
            let zones = [
                (Zone::Top, top.or(*all)),
                (Zone::Middle, middle.or(*all)),
                (Zone::Bottom, bottom.or(*all)),
            ];
            if zones.iter().all(|(_, c)| c.is_none()) {
                bail!("give at least one of --all, --top, --middle, --bottom");
            }
            for (zone, color) in zones {
                if let Some(c) = color {
                    mouse.set_zone_color(zone, c)?;
                }
            }
        }
        Command::Reactive { color } => {
            let color = match color.to_ascii_lowercase().as_str() {
                "off" | "none" => None,
                _ => Some(color.parse::<Rgb>()?),
            };
            mouse.set_reactive_color(color)?;
        }
        Command::Rainbow => mouse.rainbow_effect()?,
        Command::DefaultLighting { mode } => mouse.set_default_lighting(match mode {
            LightingArg::Off => DefaultLighting::Off,
            LightingArg::Reactive => DefaultLighting::Reactive,
            LightingArg::Rainbow => DefaultLighting::Rainbow,
            LightingArg::ReactiveRainbow => DefaultLighting::ReactiveRainbow,
        })?,
        Command::Sleep { minutes } => mouse.set_sleep_timer(*minutes)?,
        Command::Dim { seconds } => mouse.set_dim_timer(*seconds)?,
    }
    if !cli.no_save {
        mouse.save()?;
    }
    Ok(())
}
