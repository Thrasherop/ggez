//! Finding and installing a newer ggez.
//!
//! Releases are on GitHub. Each has one download per system and a
//! `SHA256SUMS.txt` that lists them with their checksums, and the file
//! names carry the version. So one small request, for the latest release's
//! checksum list, says what the newest version is, which file suits this
//! computer, and what that file has to hash to.
//!
//! What this trusts: GitHub, reached over HTTPS, to hand out the files the
//! release workflow put there. The checksum catches a download that was cut
//! short or damaged on the way. It does not prove who made the file, since
//! it comes from the same place; signed releases would, and ggez does not
//! have those yet.
//!
//! Nothing here runs by itself. Looking is one call, and installing is
//! another that only a person's click or command makes.

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const REPOSITORY: &str = "https://github.com/Thrasherop/ggez";
/// How long ggez goes without looking again by itself.
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// No release is anywhere near this big; a longer download is not one.
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;
const MAX_LIST: u64 = 64 * 1024;

/// A version of ggez: three numbers, as in `0.5.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    /// The version of the program that is running.
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).expect("the version in Cargo.toml is three numbers")
    }

    /// Exactly three plain numbers with dots between them. Anything else,
    /// such as `1.0.0-beta`, is not a version ggez would move to.
    pub fn parse(text: &str) -> Option<Self> {
        let mut numbers = text.split('.').map(|part| {
            let plain = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
            plain.then(|| part.parse::<u32>().ok()).flatten()
        });
        let version = Self {
            major: numbers.next()??,
            minor: numbers.next()??,
            patch: numbers.next()??,
        };
        numbers.next().is_none().then_some(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The page that says what changed in a version.
pub fn notes_url(version: Version) -> String {
    format!("{REPOSITORY}/releases/tag/v{version}")
}

/// The page with the newest downloads, for when ggez cannot install one
/// itself.
pub fn downloads_url() -> String {
    format!("{REPOSITORY}/releases/latest")
}

/// How the download for this kind of computer ends, after `ggez-<version>`.
/// `None` where no download is made.
pub fn download_suffix() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("-windows-x64-setup.exe")
    } else if cfg!(target_os = "macos") {
        Some("-macos.dmg")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("-linux-x64.tar.gz")
    } else {
        None
    }
}

/// A release, as far as one computer cares: its version, and the file for
/// that computer with what it must hash to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub file: String,
    /// SHA-256 of the file, as 64 lowercase hexadecimal digits.
    pub sha256: String,
}

impl Release {
    /// Find the download ending in `suffix` in a release's checksum list:
    /// lines of a checksum, spaces, and a file name.
    pub fn find(checksums: &str, suffix: &str) -> Result<Self> {
        for line in checksums.lines() {
            let mut words = line.split_whitespace();
            let (Some(sum), Some(name), None) = (words.next(), words.next(), words.next()) else {
                continue;
            };
            // `sha256sum` marks a file read in binary mode with a star.
            let name = name.trim_start_matches('*');
            let version = name
                .strip_prefix("ggez-")
                .and_then(|rest| rest.strip_suffix(suffix))
                .and_then(Version::parse);
            let Some(version) = version else {
                continue;
            };
            if sum.len() != 64 || !sum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                bail!("the checksum listed for {name} is not a SHA-256 checksum");
            }
            return Ok(Self {
                version,
                // Put together again from the version, so that nothing a
                // list says can name a file anywhere but in its release.
                file: format!("ggez-{version}{suffix}"),
                sha256: sum.to_ascii_lowercase(),
            });
        }
        bail!("the latest release has no download for this kind of computer")
    }

    pub fn download_url(&self) -> String {
        format!(
            "{REPOSITORY}/releases/download/v{}/{}",
            self.version, self.file
        )
    }
}

/// The few things ggez asks of the internet, so that everything else here
/// can be tested without it.
pub trait Web {
    /// A small text file.
    fn text(&self, url: &str) -> Result<String>;
    /// A file, written to `to` as it arrives. `progress` is told how many
    /// bytes have arrived so far and, if the server said, how many there
    /// will be.
    fn download(
        &self,
        url: &str,
        to: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()>;
}

/// The real internet, over HTTPS only.
pub struct Https;

impl Https {
    fn agent(whole_request: Duration) -> ureq::Agent {
        ureq::Agent::config_builder()
            .https_only(true)
            .user_agent(concat!("ggez/", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_global(Some(whole_request))
            .build()
            .into()
    }
}

impl Web for Https {
    fn text(&self, url: &str) -> Result<String> {
        let mut response = Self::agent(Duration::from_secs(45)).get(url).call()?;
        Ok(response
            .body_mut()
            .with_config()
            .limit(MAX_LIST)
            .read_to_string()?)
    }

    fn download(
        &self,
        url: &str,
        to: &mut dyn Write,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()> {
        let mut response = Self::agent(Duration::from_secs(30 * 60)).get(url).call()?;
        let total = response.body().content_length();
        let mut reader = response
            .body_mut()
            .with_config()
            .limit(MAX_DOWNLOAD)
            .reader();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut received = 0u64;
        loop {
            let count = std::io::Read::read(&mut reader, &mut buffer)?;
            if count == 0 {
                return Ok(());
            }
            to.write_all(&buffer[..count])?;
            received += count as u64;
            progress(received, total);
        }
    }
}

/// The newest release, for this kind of computer.
pub fn latest(web: &dyn Web) -> Result<Release> {
    let suffix = download_suffix()
        .context("there are no ggez downloads for this kind of computer; build it from source")?;
    let list = web
        .text(&format!(
            "{REPOSITORY}/releases/latest/download/SHA256SUMS.txt"
        ))
        .context("could not reach the ggez releases on github.com")?;
    Release::find(&list, suffix)
}

/// Writes to a file and works out its checksum on the way.
struct Hashing<'a> {
    file: &'a mut std::fs::File,
    hash: Sha256,
}

impl Write for Hashing<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let written = self.file.write(bytes)?;
        self.hash.update(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Download a release into `folder` and check it against its checksum.
/// Returns where the file is. A file that does not match is deleted.
pub fn fetch(
    web: &dyn Web,
    release: &Release,
    folder: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf> {
    // Earlier downloads are of no more use.
    let _ = std::fs::remove_dir_all(folder);
    std::fs::create_dir_all(folder)
        .with_context(|| format!("could not create {}", folder.display()))?;
    let partial = folder.join(format!("{}.part", release.file));
    let mut file = std::fs::File::create(&partial)
        .with_context(|| format!("could not write {}", partial.display()))?;
    let mut hashing = Hashing {
        file: &mut file,
        hash: Sha256::new(),
    };
    let downloaded = web
        .download(&release.download_url(), &mut hashing, progress)
        .with_context(|| format!("could not download {}", release.file));
    let sum = format!("{:x}", hashing.hash.finalize());
    let saved = downloaded.and_then(|()| file.sync_all().context("could not finish the file"));
    drop(file);
    let checked = saved.and_then(|()| {
        if sum != release.sha256 {
            bail!(
                "{} did not arrive whole (its checksum is wrong), so it was thrown away",
                release.file
            );
        }
        let whole = folder.join(&release.file);
        std::fs::rename(&partial, &whole).context("could not keep the download")?;
        Ok(whole)
    });
    if checked.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    checked
}

/// What ggez remembers about updates between runs, kept in `updates.toml`
/// beside the profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    /// Look for a newer version now and then without being asked.
    pub check: bool,
    /// When ggez last looked and got an answer, in seconds since 1970.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked: Option<u64>,
    /// The newest version it found then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
}

impl Default for Record {
    fn default() -> Self {
        Self {
            check: true,
            checked: None,
            latest: None,
        }
    }
}

fn seconds(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

impl Record {
    /// Where the record is kept.
    pub fn path() -> Result<PathBuf> {
        Ok(crate::paths::config_dir()?.join("updates.toml"))
    }

    /// The record in a file. A missing or unreadable one is the same as
    /// never having looked.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .with_context(|| format!("could not create {}", folder.display()))?;
        }
        let text = toml::to_string(self).context("could not write down the update record")?;
        std::fs::write(path, text).with_context(|| format!("could not write {}", path.display()))
    }

    /// Whether it is time to look again without being asked.
    pub fn due(&self, now: SystemTime) -> bool {
        self.check
            && self.checked.is_none_or(|checked| {
                // A clock set back must not put the next look years away.
                let since = seconds(now).checked_sub(checked);
                since.is_none_or(|since| since >= CHECK_EVERY.as_secs())
            })
    }

    /// The version to offer: the newest one found, if it is newer than
    /// `running`.
    pub fn newer_than(&self, running: Version) -> Option<Version> {
        self.latest
            .as_deref()
            .and_then(Version::parse)
            .filter(|latest| *latest > running)
    }

    /// Note that a look just found `latest` to be the newest version.
    pub fn found(&mut self, latest: Version, now: SystemTime) {
        self.checked = Some(seconds(now));
        self.latest = Some(latest.to_string());
    }
}

/// Where a download is kept while it is installed.
pub fn download_folder() -> Result<PathBuf> {
    Ok(crate::paths::cache_dir()?.join("update"))
}

/// What to start once this program has closed, to finish an update and
/// bring ggez back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relaunch {
    pub program: PathBuf,
    pub arguments: Vec<String>,
    /// Starting it is what installs the update, as with the Windows
    /// installer. Otherwise the update is already in place and this only
    /// opens the new ggez.
    pub installs: bool,
}

impl Relaunch {
    pub fn start(&self) -> Result<()> {
        std::process::Command::new(&self.program)
            .args(&self.arguments)
            .spawn()
            .map(|_| ())
            .with_context(|| format!("could not start {}", self.program.display()))
    }
}

/// Whether this copy of ggez can replace itself with a newer one. If not,
/// the reason is one to show the person, who can still fetch the download
/// by hand.
pub fn can_install() -> Result<(), String> {
    system::place().map(|_| ())
}

/// Install a downloaded release over the copy of ggez that is running, as
/// far as that can be done while it runs, and say what to start once it
/// has closed. `work` is a folder to unpack things in.
///
/// On Windows nothing is changed here: what is started later is the
/// installer, which does it all. On a Mac and on Linux the programs are
/// replaced now, which is safe while they run, and what is started later
/// is the new ggez.
pub fn install(download: &Path, work: &Path) -> Result<Relaunch> {
    let place = system::place().map_err(anyhow::Error::msg)?;
    system::install(download, work, &place)
}

/// Everything a click on "Update" does short of closing ggez: find the
/// newest release, download and check it, and install it as far as that
/// can be done while ggez runs. Returns the version and what to start once
/// ggez has closed.
pub fn download_and_install(
    web: &dyn Web,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<(Version, Relaunch)> {
    can_install().map_err(anyhow::Error::msg)?;
    let release = latest(web)?;
    let running = Version::current();
    if release.version <= running {
        bail!("ggez {running} is already the newest version");
    }
    let folder = download_folder()?;
    let file = fetch(web, &release, &folder.join("download"), progress)?;
    let relaunch = install(&file, &folder.join("work"))?;
    Ok((release.version, relaunch))
}

/// Whether files can be made in a folder, found out by making one.
#[cfg_attr(windows, allow(dead_code))]
fn writable(folder: &Path) -> bool {
    let probe = folder.join(format!(".ggez-update-{}", std::process::id()));
    let made = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    made
}

#[cfg_attr(windows, allow(dead_code))]
fn run(program: &str, arguments: &[&std::ffi::OsStr]) -> Result<()> {
    let output = std::process::Command::new(program)
        .args(arguments)
        .output()
        .with_context(|| format!("could not run {program}"))?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        bail!("{program} failed: {}", said.trim());
    }
    Ok(())
}

/// Put a copy of the folder `source` where the folder `target` is. The
/// copy is made beside `target` first, so `target` is only ever the old
/// folder or the complete new one; if the new one cannot be moved into
/// place, the old one is put back.
#[cfg_attr(not(any(test, target_os = "macos")), allow(dead_code))]
fn replace_folder(
    source: &Path,
    target: &Path,
    copy: impl Fn(&Path, &Path) -> Result<()>,
) -> Result<()> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .context("the folder to replace has no name")?;
    let fresh = target.with_file_name(format!(".{name}.new"));
    let old = target.with_file_name(format!(".{name}.old"));
    let _ = std::fs::remove_dir_all(&fresh);
    let _ = std::fs::remove_dir_all(&old);
    if let Err(problem) = copy(source, &fresh) {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(problem.context("could not copy the new ggez into place"));
    }
    if let Err(problem) = std::fs::rename(target, &old) {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(problem).context("could not move the old ggez aside");
    }
    if let Err(problem) = std::fs::rename(&fresh, target) {
        let _ = std::fs::rename(&old, target);
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(problem).context("could not move the new ggez into place");
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Put a copy of the file `source` where the file `target` is, or would
/// be. The copy is made beside it and moved over it in one step, which
/// also works for a program that is running.
#[cfg_attr(not(any(test, target_os = "linux")), allow(dead_code))]
fn replace_file(source: &Path, target: &Path) -> Result<()> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .context("the file to replace has no name")?;
    let fresh = target.with_file_name(format!(".{name}.new"));
    let placed = std::fs::copy(source, &fresh)
        .map(|_| ())
        .and_then(|()| std::fs::rename(&fresh, target));
    if placed.is_err() {
        let _ = std::fs::remove_file(&fresh);
    }
    placed.with_context(|| format!("could not replace {}", target.display()))
}

#[cfg(windows)]
mod system {
    use super::*;

    /// The folder the installer put ggez in.
    pub fn place() -> Result<PathBuf, String> {
        let folder = std::env::current_exe()
            .ok()
            .and_then(|program| program.parent().map(Path::to_path_buf))
            .ok_or("ggez could not find where it is installed.")?;
        // The installer leaves its uninstaller beside the programs.
        if folder.join("unins000.exe").exists() {
            Ok(folder)
        } else {
            Err(
                "This copy of ggez was not put here by its installer, so it cannot replace itself."
                    .into(),
            )
        }
    }

    pub fn install(download: &Path, _work: &Path, _place: &Path) -> Result<Relaunch> {
        Ok(Relaunch {
            program: download.to_path_buf(),
            // Show progress and any error, ask nothing, close a ggez that
            // is still running, and open ggez again at the end.
            arguments: [
                "/SILENT",
                "/NORESTART",
                "/CLOSEAPPLICATIONS",
                "/FORCECLOSEAPPLICATIONS",
                "/RELAUNCH=1",
            ]
            .map(String::from)
            .to_vec(),
            installs: true,
        })
    }
}

#[cfg(target_os = "macos")]
mod system {
    use super::*;

    /// `ggez.app`, wherever it has been put.
    pub fn place() -> Result<PathBuf, String> {
        let bundle = std::env::current_exe()
            .ok()
            .and_then(|program| bundle_of(&program))
            .ok_or("This copy of ggez is not an app, so it cannot replace itself.")?;
        // An app opened straight from the downloaded disk image is run by
        // macOS from a hidden read-only copy.
        if bundle
            .components()
            .any(|part| part.as_os_str() == "AppTranslocation")
        {
            return Err(
                "macOS is running ggez from a temporary copy. Drag ggez to Applications, open it from there, and update again."
                    .into(),
            );
        }
        match bundle.parent() {
            Some(folder) if writable(folder) => Ok(bundle),
            _ => Err(format!(
                "ggez cannot change {}. Drag ggez to Applications, open it from there, and update again.",
                bundle.display()
            )),
        }
    }

    /// The app a program inside it belongs to: `X.app/Contents/MacOS/program`.
    pub fn bundle_of(program: &Path) -> Option<PathBuf> {
        let folder = program.parent()?;
        let contents = folder.parent()?;
        let bundle = contents.parent()?;
        let is_app = bundle
            .extension()
            .is_some_and(|extension| extension == "app");
        (folder.ends_with("MacOS") && contents.ends_with("Contents") && is_app)
            .then(|| bundle.to_path_buf())
    }

    /// Detaches a disk image when it goes out of scope.
    struct Mounted<'a>(&'a Path);

    impl Drop for Mounted<'_> {
        fn drop(&mut self) {
            let _ = run(
                "hdiutil",
                &["detach".as_ref(), self.0.as_os_str(), "-force".as_ref()],
            );
        }
    }

    pub fn install(download: &Path, work: &Path, bundle: &Path) -> Result<Relaunch> {
        replace_bundle(download, work, bundle)?;
        Ok(Relaunch {
            program: "/usr/bin/open".into(),
            // A new instance even though this one is still closing.
            arguments: vec!["-n".into(), bundle.to_string_lossy().into_owned()],
            installs: false,
        })
    }

    /// Replace the app at `bundle` with the `ggez.app` in a disk image.
    pub fn replace_bundle(image: &Path, work: &Path, bundle: &Path) -> Result<()> {
        let mount = work.join("mount");
        std::fs::create_dir_all(&mount)
            .with_context(|| format!("could not create {}", mount.display()))?;
        run(
            "hdiutil",
            &[
                "attach".as_ref(),
                "-nobrowse".as_ref(),
                "-readonly".as_ref(),
                "-noautoopen".as_ref(),
                "-mountpoint".as_ref(),
                mount.as_os_str(),
                image.as_os_str(),
            ],
        )
        .context("could not open the downloaded disk image")?;
        let _mounted = Mounted(&mount);
        let source = mount.join("ggez.app");
        if !source.join("Contents/MacOS/ggez-gui").exists() {
            bail!("the downloaded disk image does not hold ggez");
        }
        // `ditto` copies an app as the Finder would: signature, links and
        // all.
        replace_folder(&source, bundle, |from, to| {
            run("ditto", &[from.as_os_str(), to.as_os_str()])
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_program_knows_the_app_it_is_in() {
            assert_eq!(
                bundle_of(Path::new("/Applications/ggez.app/Contents/MacOS/ggez-gui")),
                Some(PathBuf::from("/Applications/ggez.app"))
            );
            assert_eq!(bundle_of(Path::new("/usr/local/bin/ggez-gui")), None);
            assert_eq!(bundle_of(Path::new("/tmp/Contents/MacOS/ggez-gui")), None);
        }

        /// The whole of it against the real tools: a disk image is made
        /// as the release makes one, and an installed app is replaced
        /// from it.
        #[test]
        fn an_app_is_replaced_from_a_disk_image() {
            let scratch =
                std::env::temp_dir().join(format!("ggez-update-mac-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&scratch);
            let app = |root: &Path, says: &str| {
                let programs = root.join("ggez.app/Contents/MacOS");
                std::fs::create_dir_all(&programs).unwrap();
                std::fs::write(programs.join("ggez-gui"), says).unwrap();
                std::fs::write(programs.join("ggez"), says).unwrap();
                std::fs::write(root.join("ggez.app/Contents/Info.plist"), says).unwrap();
            };
            // The new version, in a disk image.
            let staging = scratch.join("staging");
            app(&staging, "new");
            let image = scratch.join("ggez-9.9.9-macos.dmg");
            run(
                "hdiutil",
                &[
                    "create".as_ref(),
                    "-volname".as_ref(),
                    "ggez".as_ref(),
                    "-srcfolder".as_ref(),
                    staging.as_os_str(),
                    "-ov".as_ref(),
                    "-format".as_ref(),
                    "UDZO".as_ref(),
                    image.as_os_str(),
                ],
            )
            .unwrap();
            // The old version, installed, with something of its own in it.
            let installed = scratch.join("Applications");
            app(&installed, "old");
            let bundle = installed.join("ggez.app");
            std::fs::write(bundle.join("Contents/leftover"), "old").unwrap();

            replace_bundle(&image, &scratch.join("work"), &bundle).unwrap();

            let read = |file: &str| std::fs::read_to_string(bundle.join(file)).unwrap();
            assert_eq!(read("Contents/MacOS/ggez-gui"), "new");
            assert_eq!(read("Contents/MacOS/ggez"), "new");
            assert!(
                !bundle.join("Contents/leftover").exists(),
                "replaced, not merged"
            );
            let beside: Vec<_> = std::fs::read_dir(&installed)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert_eq!(beside, ["ggez.app"], "nothing is left lying beside it");
            assert!(
                !scratch.join("work/mount/ggez.app").exists(),
                "the disk image is detached again"
            );

            // Something that is not a ggez disk image changes nothing.
            let other = scratch.join("other");
            std::fs::create_dir_all(&other).unwrap();
            std::fs::write(other.join("readme"), "not an app").unwrap();
            let wrong = scratch.join("wrong.dmg");
            run(
                "hdiutil",
                &[
                    "create".as_ref(),
                    "-srcfolder".as_ref(),
                    other.as_os_str(),
                    "-ov".as_ref(),
                    wrong.as_os_str(),
                ],
            )
            .unwrap();
            let refused = replace_bundle(&wrong, &scratch.join("work"), &bundle).unwrap_err();
            assert!(
                format!("{refused:#}").contains("does not hold ggez"),
                "{refused:#}"
            );
            assert_eq!(read("Contents/MacOS/ggez-gui"), "new");
            let _ = std::fs::remove_dir_all(&scratch);
        }
    }
}

#[cfg(target_os = "linux")]
mod system {
    use super::*;

    /// The folder the programs were unpacked into.
    pub fn place() -> Result<PathBuf, String> {
        let folder = std::env::current_exe()
            .ok()
            .and_then(|program| program.parent().map(Path::to_path_buf))
            .ok_or("ggez could not find where it is installed.")?;
        if writable(&folder) {
            Ok(folder)
        } else {
            Err(format!(
                "ggez cannot change the folder it is in ({}), so it cannot replace itself.",
                folder.display()
            ))
        }
    }

    pub fn install(download: &Path, work: &Path, folder: &Path) -> Result<Relaunch> {
        replace_programs(download, work, folder)?;
        Ok(Relaunch {
            program: folder.join("ggez-gui"),
            arguments: Vec::new(),
            installs: false,
        })
    }

    /// Replace the programs in `folder` with those in a release archive.
    /// The other files of a release (the README and so on) are replaced
    /// where `folder` already has them, and not added where it does not:
    /// the programs may have been moved somewhere of their own.
    pub fn replace_programs(archive: &Path, work: &Path, folder: &Path) -> Result<()> {
        let unpacked = work.join("unpacked");
        let _ = std::fs::remove_dir_all(&unpacked);
        std::fs::create_dir_all(&unpacked)
            .with_context(|| format!("could not create {}", unpacked.display()))?;
        run(
            "tar",
            &[
                "-xzf".as_ref(),
                archive.as_os_str(),
                "-C".as_ref(),
                unpacked.as_os_str(),
            ],
        )
        .context("could not unpack the download")?;
        // A release archive holds one folder with everything in it.
        let inside = std::fs::read_dir(&unpacked)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|path| path.join("ggez-gui").is_file() && path.join("ggez").is_file())
            .context("the download does not hold the ggez programs")?;
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&inside)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                names.push(entry.file_name());
            }
        }
        for name in names {
            let program = name == "ggez" || name == "ggez-gui";
            if program || folder.join(&name).is_file() {
                replace_file(&inside.join(&name), &folder.join(&name))?;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        /// Against the real `tar`: an archive is made as the release
        /// makes one, and the installed programs are replaced from it.
        #[test]
        fn the_programs_are_replaced_from_an_archive() {
            let scratch =
                std::env::temp_dir().join(format!("ggez-update-linux-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&scratch);
            let release = scratch.join("dist/ggez-9.9.9-linux-x64");
            std::fs::create_dir_all(&release).unwrap();
            for name in ["ggez", "ggez-gui", "README.md", "LICENSE"] {
                std::fs::write(release.join(name), format!("new {name}")).unwrap();
            }
            for program in ["ggez", "ggez-gui"] {
                let mut permissions = std::fs::metadata(release.join(program))
                    .unwrap()
                    .permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(release.join(program), permissions).unwrap();
            }
            let archive = scratch.join("ggez-9.9.9-linux-x64.tar.gz");
            run(
                "tar",
                &[
                    "-C".as_ref(),
                    scratch.join("dist").as_os_str(),
                    "-czf".as_ref(),
                    archive.as_os_str(),
                    "ggez-9.9.9-linux-x64".as_ref(),
                ],
            )
            .unwrap();
            // Installed: both programs and the README, but no LICENSE.
            let installed = scratch.join("bin");
            std::fs::create_dir_all(&installed).unwrap();
            for name in ["ggez", "ggez-gui", "README.md", "notes.txt"] {
                std::fs::write(installed.join(name), "old").unwrap();
            }

            replace_programs(&archive, &scratch.join("work"), &installed).unwrap();

            let read = |name: &str| std::fs::read_to_string(installed.join(name)).unwrap();
            assert_eq!(read("ggez"), "new ggez");
            assert_eq!(read("ggez-gui"), "new ggez-gui");
            assert_eq!(read("README.md"), "new README.md");
            assert_eq!(read("notes.txt"), "old", "what is not ggez's is left alone");
            assert!(!installed.join("LICENSE").exists(), "nothing is added");
            let mode = std::fs::metadata(installed.join("ggez-gui"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "still a program that can be run");
            let mut beside: Vec<_> = std::fs::read_dir(&installed)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            beside.sort();
            assert_eq!(beside, ["README.md", "ggez", "ggez-gui", "notes.txt"]);

            // An archive of something else changes nothing.
            let other = scratch.join("dist/other");
            std::fs::create_dir_all(&other).unwrap();
            std::fs::write(other.join("readme"), "not ggez").unwrap();
            let wrong = scratch.join("wrong.tar.gz");
            run(
                "tar",
                &[
                    "-C".as_ref(),
                    scratch.join("dist").as_os_str(),
                    "-czf".as_ref(),
                    wrong.as_os_str(),
                    "other".as_ref(),
                ],
            )
            .unwrap();
            let refused = replace_programs(&wrong, &scratch.join("work"), &installed).unwrap_err();
            assert!(
                format!("{refused:#}").contains("does not hold"),
                "{refused:#}"
            );
            assert_eq!(read("ggez"), "new ggez");
            let _ = std::fs::remove_dir_all(&scratch);
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod system {
    use super::*;

    pub fn place() -> Result<PathBuf, String> {
        Err("ggez cannot replace itself on this system.".into())
    }

    pub fn install(_download: &Path, _work: &Path, _place: &Path) -> Result<Relaunch> {
        bail!("ggez cannot replace itself on this system")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// A pretend internet: what each address holds, and what was asked for.
    #[derive(Default)]
    struct Pages {
        pages: HashMap<String, Vec<u8>>,
        asked: RefCell<Vec<String>>,
    }

    impl Web for Pages {
        fn text(&self, url: &str) -> Result<String> {
            self.asked.borrow_mut().push(url.to_string());
            let page = self.pages.get(url).context("http status: 404")?;
            Ok(String::from_utf8(page.clone())?)
        }

        fn download(
            &self,
            url: &str,
            to: &mut dyn Write,
            progress: &mut dyn FnMut(u64, Option<u64>),
        ) -> Result<()> {
            self.asked.borrow_mut().push(url.to_string());
            let page = self.pages.get(url).context("http status: 404")?;
            for (index, piece) in page.chunks(4).enumerate() {
                to.write_all(piece)?;
                let so_far = (index * 4 + piece.len()) as u64;
                progress(so_far, Some(page.len() as u64));
            }
            Ok(())
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("ggez-update-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    /// The checksum list of release 0.4.0, as GitHub serves it.
    const LIST: &str = "\
1f0c4a7bb6a1f0f2f0a3c1d9d4a6f5b1f7a2d3c4b5a69788796a5b4c3d2e1f00  ggez-0.4.0-linux-x64.tar.gz
2e1d3c4b5a69788796a5b4c3d2e1f001f0c4a7bb6a1f0f2f0a3c1d9d4a6f5b1f  ggez-0.4.0-macos.dmg
ABCDEF4b5a69788796a5b4c3d2e1f001f0c4a7bb6a1f0f2f0a3c1d9d4a6f5b1f  ggez-0.4.0-windows-x64-setup.exe
";

    #[test]
    fn versions_are_three_plain_numbers_and_compare_as_numbers() {
        assert_eq!(version("0.4.0").to_string(), "0.4.0");
        assert!(version("0.10.0") > version("0.9.9"));
        assert!(version("1.0.0") > version("0.99.99"));
        assert!(version("0.4.1") > version("0.4.0"));
        assert_eq!(version("0.4.0"), version("0.4.0"));
        for not_one in [
            "",
            "0.4",
            "0.4.0.1",
            "v0.4.0",
            "0.4.0-beta",
            "0.4.x",
            "0..0",
            "+1.0.0",
            "1.0.0 ",
            "99999999999.0.0",
        ] {
            assert_eq!(Version::parse(not_one), None, "{not_one:?}");
        }
        // The running program has one.
        assert_eq!(Version::current().to_string(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn the_download_for_each_system_is_found_in_the_list() {
        let windows = Release::find(LIST, "-windows-x64-setup.exe").unwrap();
        assert_eq!(windows.version, version("0.4.0"));
        assert_eq!(windows.file, "ggez-0.4.0-windows-x64-setup.exe");
        assert!(windows.sha256.starts_with("abcdef4b"), "lowercase");
        assert_eq!(
            windows.download_url(),
            "https://github.com/Thrasherop/ggez/releases/download/v0.4.0/ggez-0.4.0-windows-x64-setup.exe"
        );
        let mac = Release::find(LIST, "-macos.dmg").unwrap();
        assert!(mac.sha256.starts_with("2e1d"));
        assert_eq!(
            Release::find(LIST, "-linux-x64.tar.gz").unwrap().file,
            "ggez-0.4.0-linux-x64.tar.gz"
        );
        // Written by `sha256sum` in binary mode.
        let starred = LIST.replace("  ggez", " *ggez");
        assert_eq!(Release::find(&starred, "-macos.dmg").unwrap(), mac);
    }

    #[test]
    fn a_list_that_is_not_one_is_refused() {
        let fails = |list: &str, expected: &str| {
            let problem = Release::find(list, "-macos.dmg").unwrap_err().to_string();
            assert!(problem.contains(expected), "{problem:?} lacks {expected:?}");
        };
        fails("", "no download for this kind of computer");
        fails("<html>Not Found</html>", "no download");
        fails(&LIST.replace("macos", "amiga"), "no download");
        fails("abc123  ggez-0.4.0-macos.dmg", "not a SHA-256 checksum");
        fails(
            &format!("{}  ggez-0.4.0-macos.dmg", "z".repeat(64)),
            "not a SHA-256 checksum",
        );
        // A name that tries to be somewhere else is not a version.
        fails(
            &format!("{}  ggez-../../evil-macos.dmg", "a".repeat(64)),
            "no download",
        );
        fails(
            &format!("{}  ggez-1.0.0-beta-macos.dmg", "a".repeat(64)),
            "no download",
        );
    }

    /// A release with one small file in it, served by the pretend internet.
    fn served(contents: &[u8], listed_as: &[u8]) -> (Pages, Release) {
        let sum = format!("{:x}", Sha256::digest(listed_as));
        let suffix = "-macos.dmg";
        let list = format!("{sum}  ggez-9.9.9{suffix}\n");
        let release = Release::find(&list, suffix).unwrap();
        let mut pages = Pages::default();
        pages
            .pages
            .insert(release.download_url(), contents.to_vec());
        pages.pages.insert(
            "https://github.com/Thrasherop/ggez/releases/latest/download/SHA256SUMS.txt".into(),
            list.into_bytes(),
        );
        (pages, release)
    }

    #[test]
    fn a_download_is_kept_only_if_its_checksum_is_right() {
        let folder = scratch("fetch");
        let (pages, release) = served(b"the new ggez", b"the new ggez");
        let mut seen = Vec::new();
        let file = fetch(&pages, &release, &folder, &mut |received, total| {
            seen.push((received, total))
        })
        .unwrap();
        assert_eq!(file, folder.join("ggez-9.9.9-macos.dmg"));
        assert_eq!(std::fs::read(&file).unwrap(), b"the new ggez");
        assert_eq!(seen, [(4, Some(12)), (8, Some(12)), (12, Some(12))]);
        assert_eq!(
            *pages.asked.borrow(),
            ["https://github.com/Thrasherop/ggez/releases/download/v9.9.9/ggez-9.9.9-macos.dmg"]
        );

        // Tampered with, or cut short: thrown away, and the earlier
        // download with it.
        let (pages, release) = served(b"something else", b"the new ggez");
        let problem = fetch(&pages, &release, &folder, &mut |_, _| {}).unwrap_err();
        assert!(
            format!("{problem:#}").contains("checksum is wrong"),
            "{problem:#}"
        );
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);

        // Not there at all.
        let (mut pages, release) = served(b"the new ggez", b"the new ggez");
        pages.pages.remove(&release.download_url());
        let problem = fetch(&pages, &release, &folder, &mut |_, _| {}).unwrap_err();
        assert!(
            format!("{problem:#}").contains("could not download"),
            "{problem:#}"
        );
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
    }

    /// Against the real releases page: the newest release is found, and
    /// its download for this system arrives whole. It needs the internet,
    /// so it runs only when asked for:
    /// `cargo test -p ggez-service real_release -- --ignored`
    #[test]
    #[ignore = "needs the internet"]
    fn the_real_release_downloads_and_matches_its_checksum() {
        let release = latest(&Https).unwrap();
        let folder = scratch("real");
        let mut last = (0, None);
        let file = fetch(&Https, &release, &folder, &mut |received, total| {
            last = (received, total)
        })
        .unwrap();
        let size = std::fs::metadata(&file).unwrap().len();
        println!(
            "{} is {size} bytes; the server said {:?}",
            release.file, last.1
        );
        assert!(size > 1_000_000, "{size} bytes is too small to be ggez");
        assert_eq!(last.0, size, "progress ends at the whole file");
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn looking_asks_for_one_small_file() {
        let Some(suffix) = download_suffix() else {
            return;
        };
        let list = LIST.replace("-macos.dmg", suffix);
        let mut pages = Pages::default();
        pages.pages.insert(
            "https://github.com/Thrasherop/ggez/releases/latest/download/SHA256SUMS.txt".into(),
            list.into_bytes(),
        );
        assert_eq!(latest(&pages).unwrap().version, version("0.4.0"));
        assert_eq!(pages.asked.borrow().len(), 1);

        let offline = Pages::default();
        let problem = latest(&offline).unwrap_err();
        assert!(
            format!("{problem:#}").contains("could not reach"),
            "{problem:#}"
        );
    }

    #[test]
    fn the_record_says_when_to_look_and_what_to_offer() {
        let at = |seconds: u64| UNIX_EPOCH + Duration::from_secs(seconds);
        let day = CHECK_EVERY.as_secs();
        let mut record = Record::default();
        assert!(record.check, "looking is on unless switched off");
        assert!(record.due(at(1_000_000)), "never looked");
        assert_eq!(record.newer_than(version("0.4.0")), None);

        record.found(version("0.5.0"), at(1_000_000));
        assert!(!record.due(at(1_000_000 + day - 1)));
        assert!(record.due(at(1_000_000 + day)));
        assert!(record.due(at(500)), "the clock was set back");
        assert_eq!(record.newer_than(version("0.4.0")), Some(version("0.5.0")));
        assert_eq!(record.newer_than(version("0.5.0")), None, "already there");
        assert_eq!(record.newer_than(version("0.6.0")), None, "ahead of it");

        record.check = false;
        assert!(!record.due(at(9_000_000)), "switched off");
        // What was found before is still offered.
        assert_eq!(record.newer_than(version("0.4.0")), Some(version("0.5.0")));
    }

    #[test]
    fn the_record_survives_being_saved_and_a_damaged_file_is_forgotten() {
        let path = scratch("record").join("deeper/updates.toml");
        assert_eq!(Record::load(&path), Record::default(), "no file yet");
        let mut record = Record {
            check: false,
            ..Record::default()
        };
        record.found(version("0.5.0"), UNIX_EPOCH + Duration::from_secs(77));
        record.save(&path).unwrap();
        assert_eq!(Record::load(&path), record);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "check = false\nchecked = 77\nlatest = \"0.5.0\"\n");

        // Written by a later ggez that knows more, or by hand.
        std::fs::write(&path, "check = false\nsomething_new = 1\n").unwrap();
        assert!(!Record::load(&path).check);
        std::fs::write(&path, "not toml at all [").unwrap();
        assert_eq!(Record::load(&path), Record::default());
        // A version that is not one is not offered.
        std::fs::write(&path, "latest = \"9.9.9-evil\"\n").unwrap();
        assert_eq!(Record::load(&path).newer_than(version("0.1.0")), None);
    }

    #[test]
    fn a_folder_is_replaced_whole_or_not_at_all() {
        let root = scratch("folder");
        let plain_copy = |from: &Path, to: &Path| -> Result<()> {
            std::fs::create_dir_all(to)?;
            for entry in std::fs::read_dir(from)? {
                let entry = entry?;
                std::fs::copy(entry.path(), to.join(entry.file_name()))?;
            }
            Ok(())
        };
        let (source, target) = (root.join("source"), root.join("ggez.app"));
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("program"), "new").unwrap();
        std::fs::write(target.join("program"), "old").unwrap();
        std::fs::write(target.join("leftover"), "old").unwrap();
        // Rubbish from an update that was interrupted is no obstacle.
        std::fs::create_dir_all(root.join(".ggez.app.new/half")).unwrap();

        replace_folder(&source, &target, plain_copy).unwrap();
        assert_eq!(
            std::fs::read_to_string(target.join("program")).unwrap(),
            "new"
        );
        assert!(!target.join("leftover").exists());
        let mut beside: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        beside.sort();
        assert_eq!(beside, ["ggez.app", "source"]);

        // A copy that fails leaves what was there.
        let failed = replace_folder(&source, &target, |_, to| {
            std::fs::create_dir_all(to)?;
            bail!("the disk is full")
        })
        .unwrap_err();
        assert!(format!("{failed:#}").contains("the disk is full"));
        assert_eq!(
            std::fs::read_to_string(target.join("program")).unwrap(),
            "new"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
    }

    #[test]
    fn a_file_is_replaced_in_one_step() {
        let root = scratch("file");
        std::fs::write(root.join("new"), "new").unwrap();
        std::fs::write(root.join("ggez"), "old").unwrap();
        replace_file(&root.join("new"), &root.join("ggez")).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("ggez")).unwrap(), "new");
        // One that was not there before is put there.
        replace_file(&root.join("new"), &root.join("ggez-gui")).unwrap();
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 3);
        // A source that is missing changes nothing.
        assert!(replace_file(&root.join("missing"), &root.join("ggez")).is_err());
        assert_eq!(std::fs::read_to_string(root.join("ggez")).unwrap(), "new");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 3);
    }
}
