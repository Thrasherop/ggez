//! Runs the real `ggez` program in dry-run mode, where it prints the bytes
//! it would send instead of touching a mouse.

use std::process::Command;

/// Standard output of `ggez <args>`, one line per packet. Panics if the
/// command fails.
fn ggez(args: &[&str]) -> Vec<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_ggez"))
        .args(args)
        .output()
        .expect("ggez runs");
    assert!(
        output.status.success(),
        "ggez {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Standard error of a `ggez` invocation that is expected to fail.
fn ggez_error(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_ggez"))
        .args(args)
        .output()
        .expect("ggez runs");
    assert!(!output.status.success(), "ggez {args:?} should have failed");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn settings_are_followed_by_a_save() {
    assert_eq!(
        ggez(&["--dry-run", "wired", "polling", "500"]),
        ["00 2b 01", "00 11 00"]
    );
    assert_eq!(
        ggez(&["--dry-run", "wireless", "polling", "500"]),
        ["00 6b 01", "00 51 00"]
    );
}

#[test]
fn no_save_leaves_the_save_out() {
    assert_eq!(
        ggez(&["--dry-run", "wired", "--no-save", "sleep", "5"]),
        ["00 29 e0 93 04"]
    );
}

#[test]
fn dpi_matches_what_gg_sends() {
    // Captured from SteelSeries GG for these five levels, level 3 active.
    let packets = ggez(&[
        "--dry-run",
        "wireless",
        "--no-save",
        "dpi",
        "400,800,1400,2400,3200",
        "--select",
        "3",
    ]);
    assert_eq!(packets, ["00 6d 05 02 04 09 10 1b 26"]);
}

#[test]
fn each_zone_gets_its_own_color() {
    let packets = ggez(&[
        "--dry-run",
        "wired",
        "--no-save",
        "color",
        "--all",
        "#102030",
        "--bottom",
        "blue",
    ]);
    assert_eq!(
        packets,
        [
            "00 21 01 00 10 20 30",
            "00 21 01 01 10 20 30",
            "00 21 01 02 00 00 ff"
        ]
    );
}

#[test]
fn buttons_send_three_table_packets() {
    let packets = ggez(&[
        "--dry-run",
        "wired",
        "--no-save",
        "buttons",
        "g12=key:ctrl+v",
    ]);
    assert_eq!(packets.len(), 3);
    assert!(packets[0].starts_with("00 2a 00 01 00 00 00 00 02"));
    // Side button 12 is the fourth slot of the second packet.
    assert!(
        packets[1].starts_with("00 2a 01 51 20 00 00 00 51 23 00 00 00 51 26 00 00 00 51 e0 19"),
        "{}",
        packets[1]
    );
}

#[test]
fn a_profile_file_is_applied_in_one_go() {
    let dir = std::env::temp_dir().join(format!("ggez-cli-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("profile.toml");
    std::fs::write(&file, "polling = 250\nsleep_minutes = 1\n").unwrap();
    let packets = ggez(&["--dry-run", "wired", "apply", file.to_str().unwrap()]);
    assert_eq!(packets, ["00 2b 02", "00 29 60 ea 00", "00 11 00"]);
}

#[test]
fn the_example_profile_is_valid() {
    let example = concat!(env!("CARGO_MANIFEST_DIR"), "/../../profile.example.toml");
    let packets = ggez(&["--dry-run", "wireless", "apply", example]);
    assert!(packets.len() > 8, "{packets:?}");
}

#[test]
fn mistakes_are_explained_and_send_nothing() {
    assert!(
        ggez_error(&["--dry-run", "wired", "polling", "777"]).contains("unsupported polling rate")
    );
    assert!(ggez_error(&["--dry-run", "wired", "dpi", "50"]).contains("out of range"));
    assert!(ggez_error(&["--dry-run", "wired", "buttons", "g13=left"]).contains("unknown button"));
    assert!(ggez_error(&["--dry-run", "wired", "buttons", "g1=fly"]).contains("unknown action"));
    assert!(ggez_error(&["--dry-run", "wired", "buttons", "g1"]).contains("button=action"));
    assert!(ggez_error(&["--dry-run", "wired", "battery"]).contains("cannot be dry-run"));
}
