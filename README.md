# PreventShutdownApp

> [!NOTE]
> This is just a little side project that I decided to make public. If my use case actually helped you wherever you may be, I'd be happy to hear it.
> Like all my other public projects, if you have an idea for this application and would like to make it yourself, do feel free to fork or clone the repository and run it yourself. If you have a suggestion for me instead, do put it in the Issues tab or email me about it at [hello@wilsoon.dev](mailto:hello@wilsoon.dev). I read all my emails :)

A small standalone Windows app that **blocks shutdown, restart, and sign-out** and puts a custom reminder on the Windows "This app is preventing shutdown" screen.

The main use case for me: not walking away from a shared or school PC and accidentally leaving my thumbdrive behind.

---

## Quick Start (USB Thumbdrives)

1. Download `PreventShutdownApp.exe` from the [Releases](../../releases) tab.
2. Drop `PreventShutdownApp.exe` directly onto your USB thumbdrive (for example, `E:\PreventShutdownApp.exe`).
3. Double-click it when you plug your drive in.

The app sits quietly in your system tray, automatically tracks the drive letter it was launched from, and prevents shutdown until you unplug the drive or safely eject it. No installation or administrator privileges required.

---

## Templates & Built-in Use Cases

You can run `PreventShutdownApp.exe` from the command line or a desktop shortcut using built-in presets:

### 1. Removable Drives

```bat
:: Watch a specific drive letter
PreventShutdownApp.exe --template unplug_usb_warning E:

:: Watch ANY removable drive (great for shared PCs where drive letters change)
PreventShutdownApp.exe --template unplug_usb_warning any
```

- **What counts:** USB flash drives, external memory cards, and SD cards in built-in readers.
- **What doesn't count:** Keyboards, mice, webcams, phones (MTP), or any external hard drives/SSDs.

### 2. Laptop Charger Reminder

```bat
PreventShutdownApp.exe --template take_charger
```

Blocks shutdown while your laptop is plugged into wall power so you don't leave your power brick behind. (Laptops and convertibles only. Shows an error on desktop PCs).

### 3. Don't Shut Down While A Task Runs

```bat
:: Block shutdown while another app is running (e.g. a virtual machine)
PreventShutdownApp.exe --template while_running VirtualBoxVM.exe

:: Run a backup script and block shutdown until it finishes
PreventShutdownApp.exe --body "Backing up files to E:" -- robocopy C:\Data E:\Backup /MIR
```

---

## System Tray & Play Sound On Lock

While running, an icon sits in your taskbar notification area:

- **Warning sign:** Currently blocking shutdown (e.g. drive is still plugged in).
- **Info sign:** Idle (shutdown will proceed normally).
- **Right-click / click:** Shows a menu where you can click **Quit** or test reminders.

### Locking

Windows doesn't allow third-party apps to block the computer from locking or show popups on the Windows lock screen. Instead, if you lock your PC while a reminder is active, `PreventShutdownApp` plays a quick two-tone beep as you walk away.

- Enable it from the tray menu under **Remind me when computer locks** (or launch with `--lock-reminder`).
- Use **Test lock reminder** in the tray menu to test the sound without locking.
- If the sound is too loud or soft, adjust your PC's master volume or Volume Mixer.

---

## Command Reference

```
PreventShutdownApp [--title TEXT] [--body TEXT]
PreventShutdownApp --template NAME [ARGS...] [--title TEXT] [--body TEXT]
PreventShutdownApp [--title TEXT] [--body TEXT] -- COMMAND [COMMAND ARGS...]
PreventShutdownApp --list-templates
PreventShutdownApp --stop
PreventShutdownApp --audio-check
```

| Option            | What it does                                                             |
| :---------------- | :----------------------------------------------------------------------- |
| `--template`      | Built-in preset (`unplug_usb_warning`, `take_charger`, `while_running`). |
| `ARGS`            | Extra argument for the template (e.g. drive letter `E:` or `any`).       |
| `--title`         | Custom title shown as the app name on the Windows shutdown screen.       |
| `--body`          | Custom message shown under the title (**Max 255 characters**).           |
| `--lock-reminder` | Launches with the lock-screen beep enabled.                              |
| `--stop`          | Closes any running reminder instance cleanly.                            |
| `-- COMMAND`      | Runs a command and blocks shutdown until that command exits.             |

### Diagnostics

Only needed if the app doesn't seem to be blocking anything. See [When shutdown isn't blocked at all](#when-shutdown-isnt-blocked-at-all).

| Option            | What it does                                                             |
| :---------------- | :----------------------------------------------------------------------- |
| `--log`           | Writes `PreventShutdownApp.log` next to the exe, recording every shutdown request and what the app did about it. |
| `--log-file PATH` | Writes that log somewhere else instead (implies `--log`).                |
| `--alert`         | Plays the two-tone beep when a shutdown is attempted. Stays silent unless the sound could actually be heard. |
| `--audio-check`   | Reports whether an alert could be heard on this PC, then exits.          |
| `--observe-only`  | **Protects nothing.** Allows every shutdown and only records what Windows did. For diagnosis only. |
| `--memory`        | Shows which PCs this drive remembers, and what this one matches, then exits. |
| `--no-memory`     | Doesn't read or write that record at all.                                |
| `--memory-file PATH` | Keeps the record somewhere other than next to the exe.               |
| `--as-host NAME`  | Pretends the PC has this name, to see which remembered machines and prefixes it would match. Changes nothing on the PC. |
| `--show-window`   | Shows the blocker window instead of keeping it hidden.                   |
| `--stall MS`      | Waits this long before answering Windows, to test the unresponsive-app path rather than the veto path. Max 60000. |

---

## When Shutdown Isn't Blocked At All

On some managed PCs (school, work, library), pressing **Shut down** turns the machine off with no "Closing apps" screen and no pause at all. If that happens, the app is almost certainly working correctly and being overruled.

Windows lets an application decline a shutdown, but **a forced shutdown ignores that answer entirely**. When Windows ends the session it sets a flag called `ENDSESSION_CRITICAL`, which Microsoft documents as *"The application is forced to shut down."* No application can block that, by design, and no setting in this app can change it.

### Finding out which is happening

Run with logging and then shut down normally:

```bat
PreventShutdownApp.exe --log --template unplug_usb_warning any
```

Boot back up, plug the drive in, and open `PreventShutdownApp.log` next to the exe. The useful lines look like this:

```
WM_QUERYENDSESSION received: lparam=0x0 [none (plain shutdown/restart)], currently blocking=true
  -> returning 0 (VETO: we told Windows not to shut down...)
WM_ENDSESSION: session IS ending (wparam=1), lparam=0x40000000 [ENDSESSION_CRITICAL]
```

- **No `WM_QUERYENDSESSION` line at all:** the app wasn't blocking. Check the tray icon — if the condition isn't met (drive not detected as removable, for instance), there is nothing to block.
- **A veto, then `ENDSESSION_CRITICAL`:** your veto was overruled. This PC forces shutdowns, and nothing the app does can stop it.
- **A veto and no `WM_ENDSESSION`:** it worked. The shutdown was cancelled.

To check whether it's only the Start menu doing this, try a shutdown that is explicitly *not* forced:

```bat
shutdown /s /t 0
```

> [!NOTE]
> The `/t 0` is not optional. `shutdown.exe` implies `/f` (force) whenever the timeout is greater than zero, and the timeout defaults to 30 seconds — so a bare `shutdown /s` forces, and tells you nothing.

If that is overruled too, the whole machine forces every shutdown regardless of where it came from.

### If it can't be blocked

Blocking is out of reach on such a PC, but a warning isn't:

```bat
PreventShutdownApp.exe --alert --template unplug_usb_warning any
```

`--alert` beeps the moment a shutdown is attempted, while you are still standing at the machine. It is deliberately conservative: before playing, it checks the default playback device, and if there is no device, or it's muted, or the volume is under 10%, it stays silent and records why in the log, rather than leaving you trusting a warning that was never heard. Check in advance with `--audio-check`.

---

## Remembering Which PCs Ignore The Block

A warning during a shutdown is already too late: the screen is covered by Windows' own shutdown splash, and on a PC with no audio there is no way to reach you at all. The app therefore writes down the PCs that overruled it, in `PreventShutdownApp.machines.txt` next to the exe — so on a thumbdrive the record travels with you between machines.

The first time a PC overrules the block, that costs you one shutdown. From then on, launching the app there shows a window saying so for as long as you are working, instead of a tray icon quietly promising protection it cannot deliver.

Run `--memory` to see what the drive remembers:

```bat
PreventShutdownApp.exe --memory
```

```
This PC
  name:   SIT-L424-PC777
  key:    622786f84571c439 (from the firmware UUID)
  groups: SIT-*, SIT-L424-*

  SIT-*                  block ignored  2026-09-01
  SIT-L424-*             block ignored  2026-10-02 to 2026-10-20, 4 times
  SIT-L501-*             block worked   2026-10-11

Match
  SIT-L424-PC505 (matched SIT-L424-*), 4 times, 2026-10-02 to 2026-10-20
  state: overridden
```

### One shutdown teaches it about the whole room

Lab PCs are deployed from one image under one group policy, so a PC that overrules the block almost certainly has neighbours that do too. Alongside each machine, the app records the **narrowest prefix of the computer name** — `SIT-L424-PC505` is recorded as `SIT-L424-*` — and a PC it has never seen before is warned about if its name matches a recorded prefix.

The file is plain text and meant to be edited. Prefixes can be widened by hand, so a single line covers a whole site:

```
group SIT- state=overridden first=2026-09-01 last=2026-09-01 count=1
```

The most specific match always wins: the machine itself, then the longest matching prefix. So a room that *does* honour the block (`SIT-L501-*` above) is not tarred by a site-wide line, and a PC that starts behaving is recorded as `respected` and stops being warned about. Delete a line to forget it, or the file to forget everything.

Use `--as-host NAME` to check what a given computer name would match without renaming anything.

### What it identifies a PC by

Keyed, in order of preference, by:

1. The **SMBIOS system UUID** from the firmware. It survives both reimaging and renaming, which is exactly what a managed estate does to its machines.
2. The **baseboard serial**, if the firmware left the UUID blank or filled it with `FF`s, as some OEMs do.
3. The **computer name**, if neither is usable.

Deliberately *not* the MAC address: Windows randomises Wi-Fi MACs per network, and a dock or USB-Ethernet adapter carries its own, so the identity would follow the adapter between PCs rather than staying with the PC. Also not `MachineGuid` or the machine SID, which can be identical across every PC deployed from one image.

The key is stored as a hash, not as the serial itself, so a drive that goes missing in a lab PC isn't carrying a readable list of hardware IDs. Computer names are kept in plain text, since they are what make the file worth reading.

> [!NOTE]
> Nothing here needs admin rights, and nothing is written to the PC — the firmware table and the Windows build number are read-only lookups, and the only file written is the one next to the exe. Use `--no-memory` to switch it off entirely.

---

## Good To Know On Shared PCs

- **No Admin Rights**: The app does not write to the Windows Registry, install services, or leave temp files behind.
- **SmartScreen Warning**: If Windows flags the file after downloading it in a browser, right-click `PreventShutdownApp.exe` → **Properties** → check **Unblock** → **Apply**. Copying the file to a FAT32 or exFAT flash drive will clear this flag automatically. This file is **not signed**, so it may trigger "Unknown Publisher" notices.
- **Reminders vs Hard Locks**: Users can always click "Shut down anyway", and forced shutdowns (holding the hardware power button, or a managed PC that forces every shutdown) bypass Windows shutdown screens entirely. Treat this tool strictly as a helpful reminder. If it never seems to block anything, see [When shutdown isn't blocked at all](#when-shutdown-isnt-blocked-at-all), and [Remembering which PCs ignore the block](#remembering-which-pcs-ignore-the-block) for how the app warns you on those PCs in future.
