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

---

## Good To Know On Shared PCs

- **No Admin Rights**: The app does not write to the Windows Registry, install services, or leave temp files behind.
- **SmartScreen Warning**: If Windows flags the file after downloading it in a browser, right-click `PreventShutdownApp.exe` → **Properties** → check **Unblock** → **Apply**. Copying the file to a FAT32 or exFAT flash drive will clear this flag automatically. This file is **not signed**, so it may trigger "Unknown Publisher" notices.
- **Reminders vs Hard Locks**: Users can always click "Shut down anyway", and forced shutdowns (such as holding down the hardware power button) will bypass Windows shutdown screens. Treat this tool strictly as a helpful reminder.
