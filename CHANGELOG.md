# Changelog

What changed in each version of ggez, newest first. Downloads are on the
[Releases page](https://github.com/Thrasherop/ggez/releases).

ggez is developed against one Aerox 9 Wireless (firmware 1.2.4) on
Windows 11. Each version says what in it had not been tried on a real
mouse when it was released; the table in the
[README](README.md#how-it-works-and-how-sure-we-are) has the current state.

## Not yet released

### Fixed

- After an update on Windows, the installer that was downloaded is deleted
  once it has finished. It used to stay until ggez was next opened,
  because ggez opens again while the installer is still running and its
  file cannot be deleted until then.

## 0.5.0

### New

- **One-click updates.** When a newer version is out, a line at the top
  of the window says so. **Update now** downloads it, checks it against
  the release's published checksum, and restarts ggez with it. On Windows
  the icon by the clock announces it too, and `ggez update` does it from
  a terminal. ggez looks once a day by asking github.com for one small
  file and sends nothing about you; **Settings → Look for a new version
  once a day** turns that off. Nothing is ever installed without a click,
  and an update waits for changes you have not applied yet.
- Before an update replaces the programs, the helper is asked to finish
  what it is doing and stop, wherever it is running, so that no key a
  macro holds is left down.

### Not yet tried on hardware

- Updating on a Mac and on Linux. Replacing the app from a real disk
  image, and the programs from a real archive, is tested on those systems
  in CI; the download and restart around it have only been run on
  Windows.
- Everything listed under 0.4.0.

## 0.4.0

### New

- **Macros on macOS.** A button with a macro now works on a Mac as it
  does on Windows. macOS has to be told to allow it: switch ggez on under
  System Settings → Privacy & Security → Accessibility. The macro editor
  says when that is needed and opens the list for you, and macros start
  by themselves once it is done. A Mac knows eight of the twelve keys the
  mouse uses to say a macro button is held, so the first eight macro
  buttons work there.
- The README has a section on what to do when something is wrong, and a
  table of what has been tried on which system.

### Changed

- The helper watches the keyboard only while the profile has macros.
  Before, on Windows, it did so whenever it was running.

### Not yet tried on hardware

- Macros on a Mac. The key table and the events sent are tested on a Mac
  in CI; catching the mouse's keys and the permission step have only been
  compiled.
- Everything listed under 0.3.0.

## 0.3.0

### New

- **Ctrl/Cmd.** Copy is Ctrl+C on Windows and Linux and Cmd+C on a Mac,
  and the mouse cannot tell which it is plugged into. A binding can use a
  modifier that is Ctrl on a PC and Cmd on a Mac: the **Ctrl/Cmd** button
  under "Held with", or `key:primary+c` in a profile. Each time the mouse
  arrives at a computer, ggez there sends it that computer's version.
  This needs ggez running on every computer the mouse is used with, and
  the same profile on each.
- On macOS and Linux, closing the window while "Start the helper when I
  log in" is on leaves the helper running, as it would be after logging
  in. Before, it stopped until the next login.

### Fixed

- The Win modifier is called Cmd on a Mac and Super on Linux.
- A binding too long for its key in the side-button grid ends in an
  ellipsis instead of being cut off at both edges.

### Not yet tried on hardware

- Ctrl/Cmd with a mouse moved between a PC and a Mac.
- Macros from a real button press.
- Anything on Linux.

## 0.2.0

### New

- **A new window.** The mouse is drawn to the Aerox 9's measured shape
  and lit in your colors. Buttons are chosen on the drawing; lighting
  zones are set beside the zones they light.
- **Macros (Windows).** A button can run a list of steps: shortcuts one
  after another, typed text, clicks and pauses. Once per press, repeating
  while held, or from one press until the next. Up to twelve buttons.

### Fixed

- **macOS: a plugged-in mouse shown as "Not connected".** ggez chose the
  mouse's settings interface by a number macOS does not always report. It
  is now also recognised by its usage page, and opened shared, so the
  window and the helper cannot shut each other out. Since confirmed on a
  MacBook.
- A mouse that is found but that the system will not open says so, with
  the system's own reason and what to try.
- `ggez list` says whether the settings interface opens, and why not.

## 0.1.0

The first release: buttons, sensitivity, polling rate, lighting, power
settings, profiles, the Windows notification-area icon and the `ggez`
command line, over the USB cable and the 2.4 GHz dongle.
