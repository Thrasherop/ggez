use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use ggez_core::transport::{list_steelseries, listen, HidTransport};
use ggez_core::{
    Aerox9, Button, ButtonMap, DefaultLighting, Link, MockTransport, PollingRate, Rgb, Transport,
    Zone,
};
use ggez_service::{autostart, helper, paths, Profile};

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

#[derive(Clone, Copy, ValueEnum)]
enum AutostartArg {
    Enable,
    Disable,
    Status,
}

#[derive(Subcommand)]
enum Command {
    /// List every SteelSeries HID interface on this machine.
    List,
    /// Show battery level and charging state.
    Battery,
    /// Show the mouse's firmware version.
    Firmware,
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
    /// Rebind buttons, e.g. `ggez buttons g1=key:f13 g2=key:ctrl+shift+k g3=disabled`.
    ///
    /// Buttons: left, right, middle, tilt-left, tilt-right, dpi, g1-g12 (side
    /// grid), scroll-up, scroll-down.
    /// Actions: disabled, left, right, middle, back, forward, dpi, scroll-up,
    /// scroll-down, key:<name> (or a combination such as key:ctrl+shift+k),
    /// media:<name>.
    ///
    /// The mouse cannot report its bindings, so this sets ALL of them: any
    /// button not listed goes back to its factory binding. With no arguments,
    /// every button is reset. Close SteelSeries GG
    /// first: while it runs it performs its own bindings on top of these.
    Buttons {
        /// `button=action` pairs.
        bindings: Vec<String>,
    },
    /// Apply every setting in a profile file (TOML). See the README.
    /// Without a file, applies the profile the background helper uses.
    Apply { file: Option<std::path::PathBuf> },
    /// Run the background helper: re-applies colors whenever the mouse
    /// connects or wakes, and applies the helper profile when it changes.
    Daemon {
        /// Use this profile file instead of the default one.
        #[arg(long)]
        profile: Option<std::path::PathBuf>,
    },
    /// Start the background helper at login: `enable`, `disable` or `status`.
    Autostart { action: AutostartArg },
    /// Print where the helper profile lives.
    ProfilePath,
    /// Save current settings to the mouse's onboard memory.
    Save,
    /// Protocol research: print every report the mouse sends on its
    /// vendor interfaces, e.g. while you press buttons.
    Listen {
        #[arg(long, default_value_t = 30)]
        seconds: u64,
    },
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
    match &cli.command {
        Command::Daemon { profile } => return daemon(profile.clone()),
        Command::Autostart { action } => return autostart_command(*action),
        Command::ProfilePath => {
            println!("{}", paths::profile_path()?.display());
            return Ok(());
        }
        _ => {}
    }
    if let Command::Listen { seconds } = cli.command {
        let start = std::time::Instant::now();
        listen(
            std::time::Duration::from_secs(seconds),
            |interface, report| {
                // SteelSeries GG, if running, streams lighting commands; hide the
                // mouse's acknowledgements of those.
                if report.starts_with(&[0x61, 0x00]) {
                    return;
                }
                let len = report.iter().rposition(|&b| b != 0).map_or(1, |i| i + 1);
                println!(
                    "{:8.3}  if{}  {}",
                    start.elapsed().as_secs_f64(),
                    interface,
                    hex(&report[..len])
                );
            },
        )?;
        return Ok(());
    }

    match cli.dry_run {
        Some(link) => {
            let link = match link {
                LinkArg::Wired => Link::Wired,
                LinkArg::Wireless => Link::Wireless,
            };
            let mut mouse = Aerox9::new(MockTransport::accepting(), link);
            if let Command::Battery | Command::Firmware = cli.command {
                bail!("this command cannot be dry-run: it needs a reply from the mouse");
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

fn daemon(profile: Option<std::path::PathBuf>) -> Result<()> {
    let Some(_lock) = helper::InstanceLock::acquire()? else {
        bail!("the ggez helper is already running");
    };
    let profile = match profile {
        Some(profile) => profile,
        None => paths::profile_path()?,
    };
    println!("ggez helper running; profile: {}", profile.display());
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    helper::run(&profile, stop, |event| println!("{event:?}"));
    Ok(())
}

fn autostart_command(action: AutostartArg) -> Result<()> {
    match action {
        AutostartArg::Enable => {
            let launch = autostart::Launch::for_current_exe()?;
            autostart::enable(&launch)?;
            println!(
                "The helper will start at login: {}",
                launch.program.display()
            );
        }
        AutostartArg::Disable => {
            autostart::disable()?;
            println!("The helper will no longer start at login.");
        }
        AutostartArg::Status => match autostart::location()? {
            Some(location) => println!("enabled ({location})"),
            None => println!("disabled"),
        },
    }
    Ok(())
}

fn run<T: Transport>(mouse: &mut Aerox9<T>, cli: &Cli) -> Result<()> {
    match &cli.command {
        Command::List
        | Command::Listen { .. }
        | Command::Daemon { .. }
        | Command::Autostart { .. }
        | Command::ProfilePath => unreachable!("handled in main"),
        Command::Battery => {
            let b = mouse
                .battery()
                .context("could not read battery (is the mouse switched on?)")?;
            let state = if b.charging { ", charging" } else { "" };
            println!("{}%{}", b.percent, state);
            return Ok(());
        }
        Command::Firmware => {
            println!("{}", mouse.firmware_version()?);
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
            let rate = PollingRate::from_hz(*hz).with_context(|| {
                format!("unsupported polling rate {hz} (use 125, 250, 500, 1000)")
            })?;
            mouse.set_polling_rate(rate)?;
        }
        Command::Color {
            all,
            top,
            middle,
            bottom,
        } => {
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
        Command::Buttons { bindings } => {
            let mut map = ButtonMap::default();
            for binding in bindings {
                let (button, action) = binding
                    .split_once('=')
                    .with_context(|| format!("expected button=action, got {binding:?}"))?;
                map.set(button.parse::<Button>()?, action.parse()?);
            }
            mouse.set_buttons(&map)?;
        }
        Command::Apply { file } => {
            let file = match file {
                Some(file) => file.clone(),
                None => paths::profile_path()?,
            };
            if !Profile::load(&file)?.apply(mouse)? {
                bail!("{} sets nothing", file.display());
            }
        }
        Command::Sleep { minutes } => mouse.set_sleep_timer(*minutes)?,
        Command::Dim { seconds } => mouse.set_dim_timer(*seconds)?,
    }
    if !cli.no_save {
        mouse.save()?;
    }
    Ok(())
}
