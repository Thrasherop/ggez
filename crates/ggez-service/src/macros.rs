//! Macros: a button that runs a list of steps on the computer.
//!
//! The mouse can hold up to four keys for as long as a button is held, and
//! nothing more. Anything with an order or a pause in it (press A then B,
//! type a word, click again and again while held) has to be done by the
//! helper, on the computer.
//!
//! The helper has to know when the button goes down and comes up. It does
//! not read the mouse for that: on Windows the mouse's own reports are out
//! of reach without a kernel driver. Instead the button is bound, on the
//! mouse, to a key that no keyboard has (F13 to F24). The mouse sends that
//! key for exactly as long as the button is held, as it does for any key
//! binding; the helper catches the key before any program sees it and runs
//! the macro. That gives twelve macro buttons, one per spare key.
//!
//! This module is the part with no operating system in it: what a macro
//! is, how a profile describes one, and the engine that turns presses and
//! the passing of time into keys to send. [`crate::macro_host`] connects it
//! to the real keyboard.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use ggez_core::{Action, Button};
use serde::{Deserialize, Serialize};

/// The keys the mouse sends to say a macro button is held: F13 to F24, as
/// USB HID keyboard usage codes.
pub const TRIGGER_KEYS: std::ops::RangeInclusive<u8> = 0x68..=0x73;
/// How many buttons can have a macro: one per trigger key.
pub const MAX_MACROS: usize = 12;
const MAX_STEPS: usize = 200;
const MAX_WAIT: Duration = Duration::from_secs(60);
const MAX_TEXT: usize = 500;
/// How long a tapped key or a click is held down, and the pause after each
/// step, so that programs see the steps in order.
const TAP: Duration = Duration::from_millis(12);
/// A repeating macro starts over no sooner than this after it last
/// started, however short its steps are.
const SHORTEST_PASS: Duration = Duration::from_millis(25);

/// A macro as a profile file describes it.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MacroSpec {
    /// `once` (the default), `repeat` or `toggle`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// The steps in order: `key:ctrl+c`, `down:shift`, `up:shift`,
    /// `text:hello`, `click:left`, `wait:50`.
    pub steps: Vec<String>,
}

/// When a macro runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Once for each press of the button.
    #[default]
    Once,
    /// Again and again for as long as the button is held.
    Repeat,
    /// Again and again from one press of the button until the next.
    Toggle,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Once, Mode::Repeat, Mode::Toggle];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Once => "once",
            Mode::Repeat => "repeat",
            Mode::Toggle => "toggle",
        }
    }

    fn parse(name: &str) -> Result<Self> {
        Mode::ALL
            .into_iter()
            .find(|mode| mode.name() == name)
            .with_context(|| format!("unknown macro mode {name:?} (use once, repeat or toggle)"))
    }
}

/// One thing the computer is asked to do. Keys are USB HID keyboard usage
/// codes; mouse buttons are 1 (left) to 5 (forward).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    KeyDown(u8),
    KeyUp(u8),
    MouseDown(u8),
    MouseUp(u8),
    Text(String),
}

/// What a macro is made of once its steps are spelled out: a tapped key is
/// a press, a pause and a release.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Send(Output),
    Wait(Duration),
}

/// A macro ready to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Macro {
    pub mode: Mode,
    ops: Vec<Op>,
}

/// Parse one step as written in a profile. Returns what went wrong in
/// words meant for the person who wrote it.
pub fn check_step(step: &str) -> Result<()> {
    parse_step(step).map(|_| ())
}

fn key(name: &str) -> Result<u8> {
    match format!("key:{name}").parse::<Action>() {
        Ok(Action::Keys([code, 0, 0, 0])) => typeable(code),
        _ => bail!("{name:?} is not one key"),
    }
}

/// Only keys the computer can be made to press are allowed in a macro.
fn typeable(code: u8) -> Result<u8> {
    if scan_code(code).is_none() {
        bail!("ggez cannot press the key with code {code:#04x}");
    }
    if TRIGGER_KEYS.contains(&code) {
        bail!("F13 to F24 are how the mouse tells ggez a macro button is held, so a macro cannot press them");
    }
    Ok(code)
}

fn parse_step(written: &str) -> Result<Vec<Op>> {
    let step = written.trim();
    let (kind, rest) = step.split_once(':').with_context(|| {
        format!("{step:?} is not a step (try key:ctrl+c, text:hello or wait:50)")
    })?;
    let send = |output| Op::Send(output);
    Ok(match kind.to_ascii_lowercase().as_str() {
        "key" => {
            let Ok(Action::Keys(codes)) = format!("key:{rest}").parse::<Action>() else {
                bail!("{rest:?} is not a key or a shortcut");
            };
            let codes: Vec<u8> = codes.into_iter().filter(|&code| code != 0).collect();
            let mut ops = Vec::new();
            for &code in &codes {
                ops.push(send(Output::KeyDown(typeable(code)?)));
            }
            ops.push(Op::Wait(TAP));
            ops.extend(codes.iter().rev().map(|&code| send(Output::KeyUp(code))));
            ops.push(Op::Wait(TAP));
            ops
        }
        "down" => vec![send(Output::KeyDown(key(rest)?)), Op::Wait(TAP)],
        "up" => vec![send(Output::KeyUp(key(rest)?)), Op::Wait(TAP)],
        "click" => {
            let button = match rest.to_ascii_lowercase().as_str() {
                "left" => 1,
                "right" => 2,
                "middle" => 3,
                "back" => 4,
                "forward" => 5,
                other => {
                    bail!("{other:?} is not a mouse button (left, right, middle, back, forward)")
                }
            };
            vec![
                send(Output::MouseDown(button)),
                Op::Wait(TAP),
                send(Output::MouseUp(button)),
                Op::Wait(TAP),
            ]
        }
        "text" => {
            // Not trimmed: spaces at the end of what is typed can matter.
            let text = written.split_once(':').map_or("", |(_, text)| text);
            if text.is_empty() {
                bail!("text: needs something to type");
            }
            if text.chars().count() > MAX_TEXT {
                bail!("text: can type at most {MAX_TEXT} characters in one step");
            }
            vec![send(Output::Text(text.to_string())), Op::Wait(TAP)]
        }
        "wait" => {
            let millis: u64 = rest
                .trim()
                .parse()
                .with_context(|| format!("wait: needs a number of milliseconds, not {rest:?}"))?;
            let wait = Duration::from_millis(millis);
            if wait > MAX_WAIT {
                bail!("wait: can be at most {} seconds", MAX_WAIT.as_secs());
            }
            vec![Op::Wait(wait)]
        }
        other => bail!("{other:?} is not a kind of step (key, down, up, text, click, wait)"),
    })
}

impl Macro {
    pub fn parse(spec: &MacroSpec) -> Result<Self> {
        let mode = spec.mode.as_deref().map(Mode::parse).transpose()?;
        if spec.steps.is_empty() {
            bail!("a macro needs at least one step");
        }
        if spec.steps.len() > MAX_STEPS {
            bail!("a macro can have at most {MAX_STEPS} steps");
        }
        let mut ops = Vec::new();
        for (index, step) in spec.steps.iter().enumerate() {
            ops.extend(parse_step(step).with_context(|| format!("step {}", index + 1))?);
        }
        Ok(Self {
            mode: mode.unwrap_or_default(),
            ops,
        })
    }
}

/// Every macro in a profile, each with the key the mouse sends for it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub macros: Vec<(Button, u8, Macro)>,
}

impl Plan {
    /// Work out the plan for a profile's `[macros]` table. Buttons get
    /// their trigger keys in the order they sit on the mouse, so the same
    /// profile always gives the same plan.
    pub fn new(specs: Option<&BTreeMap<String, MacroSpec>>) -> Result<Self> {
        let mut parsed = Vec::new();
        for (name, spec) in specs.into_iter().flatten() {
            let button: Button = name.parse()?;
            let made = Macro::parse(spec).with_context(|| format!("the macro on {name}"))?;
            parsed.push((button, made));
        }
        if parsed.len() > MAX_MACROS {
            bail!(
                "{} buttons have a macro, but at most {MAX_MACROS} can (one for each of the keys F13 to F24)",
                parsed.len()
            );
        }
        let place = |button: &Button| Button::ALL.iter().position(|b| b == button);
        parsed.sort_by_key(|(button, _)| place(button));
        Ok(Self {
            macros: parsed
                .into_iter()
                .zip(TRIGGER_KEYS)
                .map(|((button, made), trigger)| (button, trigger, made))
                .collect(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.macros.is_empty()
    }
}

/// A macro in the middle of running.
#[derive(Debug)]
struct Run {
    trigger: u8,
    /// The next op to do.
    at: usize,
    /// When to do it.
    due: Instant,
    /// When this pass through the steps began.
    began: Instant,
    /// Keys and mouse buttons this run has put down and not let go of.
    keys: Vec<u8>,
    buttons: Vec<u8>,
    /// Stop at the next pause instead of going on.
    stopping: bool,
}

/// Turns presses of macro buttons and the passing of time into things for
/// the computer to do. It never looks at a clock or a keyboard itself, so
/// it can be tested exactly.
#[derive(Debug, Default)]
pub struct Engine {
    macros: Vec<(u8, Macro)>,
    /// Trigger keys that are down. A held key repeats, and the repeats
    /// must not start the macro again.
    held: Vec<u8>,
    runs: Vec<Run>,
}

impl Engine {
    /// Use a new set of macros. Anything running is stopped; returns what
    /// has to be sent to let go of the keys it was holding.
    pub fn set_plan(&mut self, plan: &Plan) -> Vec<Output> {
        let released = self.stop_all();
        self.macros = plan
            .macros
            .iter()
            .map(|(_, trigger, made)| (*trigger, made.clone()))
            .collect();
        released
    }

    /// The trigger keys to catch.
    pub fn triggers(&self) -> Vec<u8> {
        self.macros.iter().map(|(trigger, _)| *trigger).collect()
    }

    /// Stop everything; returns what lets go of every held key.
    pub fn stop_all(&mut self) -> Vec<Output> {
        self.held.clear();
        let mut out = Vec::new();
        for run in std::mem::take(&mut self.runs) {
            release(run, &mut out);
        }
        out
    }

    /// A trigger key went down or came up.
    pub fn trigger(&mut self, key: u8, down: bool, now: Instant) {
        let Some(mode) = self
            .macros
            .iter()
            .find(|(trigger, _)| *trigger == key)
            .map(|(_, made)| made.mode)
        else {
            return;
        };
        let was_held = self.held.contains(&key);
        self.held.retain(|held| *held != key);
        if down {
            self.held.push(key);
        }
        let running = self.runs.iter().position(|run| run.trigger == key);
        match (down, was_held, mode, running) {
            // A held key repeating itself.
            (true, true, ..) => {}
            // A second press ends a toggled macro.
            (true, false, Mode::Toggle, Some(run)) => self.runs[run].stopping = true,
            // Pressed again before the macro has finished: let it finish.
            (true, false, _, Some(run)) => self.runs[run].stopping = false,
            (true, false, _, None) => self.runs.push(Run {
                trigger: key,
                at: 0,
                due: now,
                began: now,
                keys: Vec::new(),
                buttons: Vec::new(),
                stopping: false,
            }),
            // Letting go ends a macro that repeats while held.
            (false, _, Mode::Repeat, Some(run)) => self.runs[run].stopping = true,
            (false, ..) => {}
        }
    }

    /// When [`Engine::advance`] next has something to do, if ever.
    pub fn next_due(&self) -> Option<Instant> {
        self.runs.iter().map(|run| run.due).min()
    }

    /// Do everything that is due by `now`.
    pub fn advance(&mut self, now: Instant) -> Vec<Output> {
        let mut out = Vec::new();
        let mut finished = Vec::new();
        for (index, run) in self.runs.iter_mut().enumerate() {
            let Some((_, made)) = self
                .macros
                .iter()
                .find(|(trigger, _)| *trigger == run.trigger)
            else {
                finished.push(index);
                continue;
            };
            while run.due <= now {
                if run.at == made.ops.len() {
                    // The end of a pass. Go round again only for a macro
                    // that repeats and has not been told to stop.
                    if made.mode == Mode::Once || run.stopping {
                        finished.push(index);
                        break;
                    }
                    run.at = 0;
                    run.due = run.due.max(run.began + SHORTEST_PASS);
                    run.began = run.due;
                    continue;
                }
                match &made.ops[run.at] {
                    Op::Wait(wait) => {
                        if run.stopping && made.mode != Mode::Once {
                            finished.push(index);
                            break;
                        }
                        run.due += *wait;
                    }
                    Op::Send(output) => {
                        match output {
                            Output::KeyDown(key) => run.keys.push(*key),
                            Output::KeyUp(key) => run.keys.retain(|held| held != key),
                            Output::MouseDown(button) => run.buttons.push(*button),
                            Output::MouseUp(button) => run.buttons.retain(|held| held != button),
                            Output::Text(_) => {}
                        }
                        out.push(output.clone());
                    }
                }
                run.at += 1;
            }
        }
        for index in finished.into_iter().rev() {
            release(self.runs.remove(index), &mut out);
        }
        out
    }
}

/// Let go of whatever a run still holds, last pressed first.
fn release(run: Run, out: &mut Vec<Output>) {
    out.extend(run.buttons.into_iter().rev().map(Output::MouseUp));
    out.extend(run.keys.into_iter().rev().map(Output::KeyUp));
}

/// The scan code a PC keyboard sends for a key, and whether it is one of
/// the "extended" keys, given the key's USB HID usage code. Programs that
/// read the keyboard directly, as many games do, go by scan codes.
pub fn scan_code(usage: u8) -> Option<(u16, bool)> {
    const LETTERS: [u16; 26] = [
        0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18,
        0x19, 0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
    ];
    const KEYPAD_DIGITS: [u16; 9] = [0x4F, 0x50, 0x51, 0x4B, 0x4C, 0x4D, 0x47, 0x48, 0x49];
    const HIGH_F_KEYS: [u16; 12] = [
        0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x76,
    ];
    let plain = |code: u16| Some((code, false));
    let extended = |code: u16| Some((code, true));
    match usage {
        0x04..=0x1D => plain(LETTERS[usize::from(usage - 0x04)]),
        0x1E..=0x27 => plain(0x02 + u16::from(usage - 0x1E)),
        0x28 => plain(0x1C),
        0x29 => plain(0x01),
        0x2A => plain(0x0E),
        0x2B => plain(0x0F),
        0x2C => plain(0x39),
        0x2D => plain(0x0C),
        0x2E => plain(0x0D),
        0x2F => plain(0x1A),
        0x30 => plain(0x1B),
        0x31 => plain(0x2B),
        0x33 => plain(0x27),
        0x34 => plain(0x28),
        0x35 => plain(0x29),
        0x36 => plain(0x33),
        0x37 => plain(0x34),
        0x38 => plain(0x35),
        0x39 => plain(0x3A),
        0x3A..=0x43 => plain(0x3B + u16::from(usage - 0x3A)),
        0x44 => plain(0x57),
        0x45 => plain(0x58),
        0x46 => extended(0x37),
        0x47 => plain(0x46),
        0x49 => extended(0x52),
        0x4A => extended(0x47),
        0x4B => extended(0x49),
        0x4C => extended(0x53),
        0x4D => extended(0x4F),
        0x4E => extended(0x51),
        0x4F => extended(0x4D),
        0x50 => extended(0x4B),
        0x51 => extended(0x50),
        0x52 => extended(0x48),
        0x53 => plain(0x45),
        0x54 => extended(0x35),
        0x55 => plain(0x37),
        0x56 => plain(0x4A),
        0x57 => plain(0x4E),
        0x58 => extended(0x1C),
        0x59..=0x61 => plain(KEYPAD_DIGITS[usize::from(usage - 0x59)]),
        0x62 => plain(0x52),
        0x63 => plain(0x53),
        0x68..=0x73 => plain(HIGH_F_KEYS[usize::from(usage - 0x68)]),
        0xE0 => plain(0x1D),
        0xE1 => plain(0x2A),
        0xE2 => plain(0x38),
        0xE3 => extended(0x5B),
        0xE4 => extended(0x1D),
        0xE5 => plain(0x36),
        0xE6 => extended(0x38),
        0xE7 => extended(0x5C),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(mode: Option<&str>, steps: &[&str]) -> MacroSpec {
        MacroSpec {
            mode: mode.map(str::to_string),
            steps: steps.iter().map(|step| step.to_string()).collect(),
        }
    }

    fn plan(mode: Option<&str>, steps: &[&str]) -> Plan {
        let specs = BTreeMap::from([("g7".to_string(), spec(mode, steps))]);
        Plan::new(Some(&specs)).unwrap()
    }

    const F13: u8 = 0x68;
    const A: u8 = 0x04;
    const B: u8 = 0x05;
    const CTRL: u8 = 0xE0;

    /// Run the engine forward in 1 ms steps and collect what it sends, with
    /// the millisecond each thing was sent in.
    fn play(engine: &mut Engine, start: Instant, from: u64, to: u64) -> Vec<(u64, Output)> {
        let mut seen = Vec::new();
        for ms in from..=to {
            for output in engine.advance(start + Duration::from_millis(ms)) {
                seen.push((ms, output));
            }
        }
        seen
    }

    fn outputs(seen: &[(u64, Output)]) -> Vec<Output> {
        seen.iter().map(|(_, output)| output.clone()).collect()
    }

    #[test]
    fn a_shortcut_goes_down_in_order_and_comes_up_in_reverse() {
        let mut engine = Engine::default();
        engine.set_plan(&plan(None, &["key:ctrl+a"]));
        let start = Instant::now();
        engine.trigger(F13, true, start);
        let seen = play(&mut engine, start, 0, 100);
        assert_eq!(
            seen,
            [
                (0, Output::KeyDown(CTRL)),
                (0, Output::KeyDown(A)),
                (12, Output::KeyUp(A)),
                (12, Output::KeyUp(CTRL)),
            ]
        );
        assert_eq!(engine.next_due(), None, "finished");
    }

    #[test]
    fn steps_run_in_order_with_their_waits() {
        let mut engine = Engine::default();
        engine.set_plan(&plan(None, &["key:a", "wait:100", "text:hi", "click:left"]));
        let start = Instant::now();
        engine.trigger(F13, true, start);
        engine.trigger(F13, false, start + Duration::from_millis(5));
        let seen = play(&mut engine, start, 0, 400);
        assert_eq!(
            seen,
            [
                (0, Output::KeyDown(A)),
                (12, Output::KeyUp(A)),
                (124, Output::Text("hi".into())),
                (136, Output::MouseDown(1)),
                (148, Output::MouseUp(1)),
            ]
        );
    }

    #[test]
    fn once_runs_once_however_long_the_button_is_held() {
        let mut engine = Engine::default();
        engine.set_plan(&plan(None, &["key:a"]));
        let start = Instant::now();
        engine.trigger(F13, true, start);
        // The keyboard repeats a held key.
        for ms in [300, 330, 360] {
            engine.trigger(F13, true, start + Duration::from_millis(ms));
        }
        let seen = play(&mut engine, start, 0, 500);
        assert_eq!(outputs(&seen), [Output::KeyDown(A), Output::KeyUp(A)]);

        // A new press runs it again.
        engine.trigger(F13, false, start + Duration::from_millis(501));
        engine.trigger(F13, true, start + Duration::from_millis(502));
        assert_eq!(play(&mut engine, start, 502, 600).len(), 2);
    }

    #[test]
    fn repeat_goes_round_while_held_and_stops_when_let_go() {
        let mut engine = Engine::default();
        engine.set_plan(&plan(Some("repeat"), &["click:left", "wait:76"]));
        let start = Instant::now();
        engine.trigger(F13, true, start);
        let seen = play(&mut engine, start, 0, 250);
        let downs: Vec<u64> = seen
            .iter()
            .filter(|(_, output)| *output == Output::MouseDown(1))
            .map(|(ms, _)| *ms)
            .collect();
        assert_eq!(downs, [0, 100, 200], "one pass every 100 ms");

        engine.trigger(F13, false, start + Duration::from_millis(251));
        let rest = play(&mut engine, start, 251, 600);
        assert!(rest.is_empty(), "{rest:?}");
        assert_eq!(engine.next_due(), None);
    }

    #[test]
    fn repeat_with_no_waits_is_paced() {
        let mut engine = Engine::default();
        engine.set_plan(&plan(Some("repeat"), &["text:x"]));
        let start = Instant::now();
        engine.trigger(F13, true, start);
        let seen = play(&mut engine, start, 0, 1000);
        assert!((30..=42).contains(&seen.len()), "{} passes", seen.len());
    }

    #[test]
    fn letting_go_mid_pass_releases_what_the_macro_holds() {
        let mut engine = Engine::default();
        engine.set_plan(&plan(Some("repeat"), &["down:a", "wait:500", "up:a"]));
        let start = Instant::now();
        engine.trigger(F13, true, start);
        assert_eq!(
            outputs(&play(&mut engine, start, 0, 100)),
            [Output::KeyDown(A)]
        );
        engine.trigger(F13, false, start + Duration::from_millis(101));
        // It stops at the end of the wait it is in, and lets go of the key.
        let rest = play(&mut engine, start, 101, 700);
        assert_eq!(rest, [(512, Output::KeyUp(A))]);
    }

    #[test]
    fn toggle_starts_on_one_press_and_stops_on_the_next() {
        let mut engine = Engine::default();
        engine.set_plan(&plan(Some("toggle"), &["key:b", "wait:76"]));
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        engine.trigger(F13, true, at(0));
        engine.trigger(F13, false, at(20));
        let running = play(&mut engine, start, 0, 350);
        let presses = |seen: &[(u64, Output)]| {
            seen.iter()
                .filter(|(_, o)| *o == Output::KeyDown(B))
                .count()
        };
        assert_eq!(presses(&running), 4, "still going with the button up");

        engine.trigger(F13, true, at(351));
        engine.trigger(F13, false, at(370));
        let after = play(&mut engine, start, 351, 900);
        assert_eq!(presses(&after), 0);
        assert_eq!(engine.next_due(), None);
    }

    #[test]
    fn a_new_plan_stops_what_is_running_and_lets_go_of_its_keys() {
        let mut engine = Engine::default();
        engine.set_plan(&plan(
            None,
            &["down:ctrl", "click:left", "wait:1000", "up:ctrl"],
        ));
        let start = Instant::now();
        engine.trigger(F13, true, start);
        play(&mut engine, start, 0, 15);
        assert_eq!(
            engine.set_plan(&Plan::default()),
            [Output::MouseUp(1), Output::KeyUp(CTRL)]
        );
        assert!(engine.triggers().is_empty());
        assert!(play(&mut engine, start, 16, 2000).is_empty());
    }

    #[test]
    fn keys_that_are_not_triggers_are_ignored() {
        let mut engine = Engine::default();
        engine.set_plan(&plan(None, &["key:a"]));
        let start = Instant::now();
        engine.trigger(0x69, true, start);
        engine.trigger(A, true, start);
        assert!(play(&mut engine, start, 0, 50).is_empty());
    }

    #[test]
    fn buttons_get_trigger_keys_in_the_order_they_sit_on_the_mouse() {
        let specs = BTreeMap::from([
            ("g10".to_string(), spec(None, &["key:a"])),
            ("g2".to_string(), spec(Some("repeat"), &["key:b"])),
            ("left".to_string(), spec(None, &["key:a"])),
        ]);
        let plan = Plan::new(Some(&specs)).unwrap();
        let order: Vec<(Button, u8)> = plan.macros.iter().map(|(b, key, _)| (*b, *key)).collect();
        assert_eq!(
            order,
            [
                (Button::Left, 0x68),
                (Button::Grid(2), 0x69),
                (Button::Grid(10), 0x6A)
            ]
        );
        assert_eq!(plan.macros[1].2.mode, Mode::Repeat);
        assert!(Plan::new(None).unwrap().is_empty());
    }

    #[test]
    fn mistakes_are_explained() {
        let fails = |mode: Option<&str>, steps: &[&str], expected: &str| {
            let error = format!("{:#}", Macro::parse(&spec(mode, steps)).unwrap_err());
            assert!(error.contains(expected), "{error:?} lacks {expected:?}");
        };
        fails(None, &[], "at least one step");
        fails(Some("forever"), &["key:a"], "unknown macro mode");
        fails(None, &["key:a", "jump"], "step 2");
        fails(None, &["key:nope"], "not a key");
        fails(None, &["down:ctrl+a"], "not one key");
        fails(None, &["wait:soon"], "milliseconds");
        fails(None, &["wait:90000"], "at most 60 seconds");
        fails(None, &["click:thumb"], "not a mouse button");
        fails(None, &["text:"], "something to type");
        fails(None, &["key:f13"], "F13 to F24");
        fails(None, &["key:0x99"], "cannot press");
        fails(None, &["dance:now"], "not a kind of step");
        fails(None, &["key:a"; 201], "at most 200");

        let specs: BTreeMap<String, MacroSpec> = Button::ALL
            .iter()
            .take(13)
            .map(|button| (button.to_string(), spec(None, &["key:a"])))
            .collect();
        let error = Plan::new(Some(&specs)).unwrap_err().to_string();
        assert!(error.contains("at most 12"), "{error}");
    }

    #[test]
    fn text_keeps_its_spaces_and_colons() {
        let made = Macro::parse(&spec(None, &["text: a: b "])).unwrap();
        assert_eq!(made.ops[0], Op::Send(Output::Text(" a: b ".into())));
    }

    #[test]
    fn every_named_key_can_be_pressed() {
        for name in ggez_core::key_names() {
            let Ok(Action::Keys([code, ..])) = format!("key:{name}").parse::<Action>() else {
                panic!("{name} does not parse");
            };
            assert!(
                scan_code(code).is_some(),
                "{name} ({code:#04x}) has no scan code"
            );
        }
        // Spot checks against the PC keyboard's table.
        assert_eq!(scan_code(0x04), Some((0x1E, false)), "A");
        assert_eq!(scan_code(0x1D), Some((0x2C, false)), "Z");
        assert_eq!(scan_code(0x27), Some((0x0B, false)), "0");
        assert_eq!(scan_code(0x4C), Some((0x53, true)), "Delete");
        assert_eq!(scan_code(0xE6), Some((0x38, true)), "right Alt");
        assert_eq!(scan_code(0x68), Some((0x64, false)), "F13");
        assert_eq!(scan_code(0x00), None);
    }
}
