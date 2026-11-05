use clap::Parser;

use crate::machine::{laptop_check, Identity, MachineInfo};

pub const MAX_REASON_UTF16: usize = 255;

pub const DEFAULT_TITLE: &str = "PreventShutdownApp";

#[derive(Parser, Debug, Default)]
#[command(
    name = "PreventShutdownApp",
    version,
    about = "Blocks Windows shutdown, restart and sign-out with your own message.",
    after_help = "Templates (use --list-templates for details):\n  \
                  unplug_usb_warning [DRIVE | any]\n  \
                  take_charger\n  \
                  while_running NAME.exe\n\n\
                  Examples:\n  \
                  PreventShutdownApp --title \"Backup running\" --body \"Wait for the backup to finish.\"\n  \
                  PreventShutdownApp --template unplug_usb_warning E:\n  \
                  PreventShutdownApp --template while_running VirtualBoxVM.exe\n  \
                  PreventShutdownApp -- robocopy C:\\Data E:\\Backup /MIR   (block until it finishes)\n  \
                  E:\\PreventShutdownApp.exe   (no arguments: same as --template unplug_usb_warning)\n  \
                  PreventShutdownApp --stop"
)]
pub struct Cli {
    /// Window title - shown as the app's name
    #[arg(long)]
    pub title: Option<String>,

    /// Reason shown - Backslashes (\\) and Newline (\n) supported, max 255 characters
    #[arg(long)]
    pub body: Option<String>,

    /// Use a built-in preset: --title and --body still override its text
    #[arg(long, value_name = "NAME")]
    pub template: Option<String>,

    /// Beep twice when you lock the PC while shutdown is being blocked (can also be switched on and off from the tray icon)
    #[arg(long)]
    pub lock_reminder: bool,

    /// List the built-in templates and exit
    #[arg(long, conflicts_with_all = ["title", "body", "template", "stop"])]
    pub list_templates: bool,

    /// Close every running PreventShutdownApp instance and exit
    #[arg(long, conflicts_with_all = ["title", "body", "template"])]
    pub stop: bool,

    /// Write a diagnostic log of every shutdown request to PreventShutdownApp.log next to the exe
    #[arg(long)]
    pub log: bool,

    /// DIAGNOSTIC: watch and log shutdowns without blocking them. Allows every shutdown, so it protects nothing
    #[arg(long)]
    pub observe_only: bool,

    /// Play an alert sound when a shutdown is attempted, for PCs that ignore the block. Only fires if the sound could actually be heard
    #[arg(long)]
    pub alert: bool,

    /// Report whether an alert sound could be heard on this PC, then exit
    #[arg(long, conflicts_with_all = ["title", "body", "template", "stop"])]
    pub audio_check: bool,

    /// Show what this drive remembers about the PCs it has been used on, then exit
    #[arg(long, conflicts_with_all = ["title", "body", "template", "stop"])]
    pub memory: bool,

    /// Don't read or write the record of which PCs ignore the block
    #[arg(long)]
    pub no_memory: bool,

    /// Keep that record in this file instead of next to the exe
    #[arg(long, value_name = "PATH", conflicts_with = "no_memory")]
    pub memory_file: Option<String>,

    /// DIAGNOSTIC: pretend the computer is called this, to check which remembered
    /// machines and name prefixes it would match without renaming anything
    #[arg(long, value_name = "NAME")]
    pub as_host: Option<String>,

    /// DIAGNOSTIC: wait this many milliseconds before answering Windows' shutdown request (max 60000), to test the unresponsive-app path instead of the veto path
    #[arg(long, value_name = "MS", default_value_t = 0)]
    pub stall: u32,

    /// Write the diagnostic log to this file instead (implies --log)
    #[arg(long, value_name = "PATH")]
    pub log_file: Option<String>,

    /// Extra arguments for the template such as a drive letter
    #[arg(value_name = "ARGS")]
    pub args: Vec<String>,

    /// After "--": a command to run. Shutdown is blocked until it exits, then PreventShutdownApp exits with the command's exit code
    #[arg(last = true, value_name = "COMMAND")]
    pub command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// Block whenever the app is running
    Always,
    /// Block only while a volume with this drive letter is present (and has media)
    DrivePresent(char),
    AnyRemovableDrive,
    OnAcPower,
    ProcessRunning(String),
    Command(Vec<String>),
}

impl Condition {
    pub fn is_job(&self) -> bool {
        matches!(self, Condition::ProcessRunning(_) | Condition::Command(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub title: String,
    pub reason: String,
    pub condition: Condition,
    pub lock_reminder: bool,
    pub reminder: ReminderText,
    pub show_window_title: bool,
    /// Log shutdown requests but never veto them. Diagnostic only.
    pub observe_only: bool,
    /// Play an alert when a shutdown is attempted, if it could be heard.
    pub alert: bool,
    /// Milliseconds to wait before answering a shutdown request.
    pub stall_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderText {
    pub menu_label: String,
    pub explanation: String,
}

impl Default for ReminderText {
    fn default() -> Self {
        Self {
            menu_label: "Remind me when computer locks".into(),
            explanation: "Whenever you lock the computer while this app is running, two short beeps will play as a reminder.".into(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Run(Config),
    Stop,
    Info(String),
}

#[derive(Debug, Default, Clone)]
pub struct LaunchInfo {
    pub removable_exe_drive: Option<char>,
    pub machine: MachineInfo,
    /// Which PC this is. None only off Windows, and in tests.
    pub identity: Option<Identity>,
}

struct Template {
    name: &'static str,
    usage: &'static str,
    description: &'static str,
    build: fn(&[String], &LaunchInfo) -> Result<Config, String>,
}

const TEMPLATES: &[Template] = &[
    Template {
        name: "unplug_usb_warning",
        usage: "--template unplug_usb_warning [DRIVE | any]",
        description: "Blocks shutdown and sign-out while DRIVE (for example E:) is plugged in.\n\
                      With \"any\", blocks while any removable drive (USB stick, SD card) is in.\n\
                      Mice, keyboards, phones and most external hard drives don't count.\n\
                      If left out: the removable drive this EXE was started from, else \"any\".\n\
                      In --body, {drive} is replaced with the drive letter(s).",
        build: unplug_usb_warning,
    },
    Template {
        name: "take_charger",
        usage: "--template take_charger",
        description:
            "Laptops only. Blocks shutdown and sign-out while the charger is plugged in.\n\
                      Refuses to start on desktops and all-in-ones, using the chassis type\n\
                      from the firmware rather than trusting the battery reading.",
        build: take_charger,
    },
    Template {
        name: "while_running",
        usage: "--template while_running NAME.exe",
        description: "Blocks shutdown and sign-out while a program with that name is running\n\
                      (only your own copies, not other users' on a shared PC). If it isn't\n\
                      running yet, waits for it to start. Quits by itself once it has run and\n\
                      every copy has exited. The shutdown reason includes the program's window\n\
                      title, which some apps use to show progress.\n\
                      To run a command and wait for it instead, use: PreventShutdownApp -- COMMAND",
        build: while_running,
    },
];

fn unplug_usb_warning(args: &[String], launch: &LaunchInfo) -> Result<Config, String> {
    let any = match args {
        [] => launch.removable_exe_drive.is_none(),
        [one] => one.eq_ignore_ascii_case("any"),
        _ => {
            return Err(
                "unplug_usb_warning takes at most one argument (a drive letter or \"any\")".into(),
            )
        }
    };
    let (condition, which) = if any {
        (Condition::AnyRemovableDrive, "a USB drive is".to_string())
    } else {
        let drive = match args {
            [one] => parse_drive(one)?,
            _ => launch.removable_exe_drive.expect("checked above"),
        };
        (Condition::DrivePresent(drive), format!("{drive}: is"))
    };
    Ok(Config {
        title: "USB drive still plugged in".into(),
        reason: "Unplug your USB drive ({drive}:) and take it with you.".into(),
        condition,
        lock_reminder: false,
        show_window_title: false,
        observe_only: false,
        alert: false,
        stall_ms: 0,
        reminder: ReminderText {
            menu_label: "Remind me to unplug when computer locks".into(),
            explanation: format!("Whenever you lock the computer while {which} still plugged in, two short beeps will play to remind you to unplug it."),
        },
    })
}

pub fn exe_name(name: &str) -> String {
    let base = name.rsplit(['\\', '/']).next().unwrap_or(name).trim();
    if base.to_ascii_lowercase().ends_with(".exe") {
        base.to_string()
    } else {
        format!("{base}.exe")
    }
}

fn job_config(display: &str, condition: Condition) -> Config {
    Config {
        title: format!("Waiting for {display}"),
        reason: format!("Wait for {display} to finish before shutting down."),
        condition,
        lock_reminder: false,
        show_window_title: true,
        observe_only: false,
        alert: false,
        stall_ms: 0,
        reminder: ReminderText {
            menu_label: "Remind me when computer locks".into(),
            explanation: format!("Whenever you lock the computer while {display} is running, two short beeps will play as a reminder."),
        },
    }
}

fn while_running(args: &[String], _launch: &LaunchInfo) -> Result<Config, String> {
    let [name] = args else {
        return Err(
            "while_running takes one argument: a program name, e.g. VirtualBoxVM.exe".into(),
        );
    };
    let exe = exe_name(name);
    if exe == ".exe" {
        return Err("while_running needs a program name, e.g. VirtualBoxVM.exe".into());
    }
    Ok(job_config(&exe, Condition::ProcessRunning(exe.clone())))
}

pub fn command_display_name(command: &[String]) -> String {
    let first = command.first().map(String::as_str).unwrap_or("command");
    let base = first.rsplit(['\\', '/']).next().unwrap_or(first);
    base.strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base)
        .to_string()
}

pub fn compose_reason(base: &str, window_title: Option<&str>) -> String {
    let Some(title) = window_title.map(str::trim).filter(|t| !t.is_empty()) else {
        return base.to_string();
    };
    let prefix = format!("{base} Now: ");
    let room = MAX_REASON_UTF16.saturating_sub(prefix.encode_utf16().count());
    if room < 4 {
        return base.to_string();
    }
    let mut out = prefix;
    if title.encode_utf16().count() <= room {
        out.push_str(title);
    } else {
        let mut used = 0;
        for c in title.chars() {
            if used + c.len_utf16() > room - 1 {
                break;
            }
            used += c.len_utf16();
            out.push(c);
        }
        out.push('…');
    }
    out
}

fn take_charger(args: &[String], launch: &LaunchInfo) -> Result<Config, String> {
    if !args.is_empty() {
        return Err("take_charger takes no arguments".into());
    }
    laptop_check(&launch.machine)
        .map_err(|why| format!("take_charger only works on laptops.\n\n{why}"))?;
    Ok(Config {
        title: "Charger still plugged in".into(),
        reason: "Unplug your charger and take it with you.".into(),
        condition: Condition::OnAcPower,
        lock_reminder: false,
        show_window_title: false,
        observe_only: false,
        alert: false,
        stall_ms: 0,
        reminder: ReminderText {
            menu_label: "Remind me to take the charger when computer locks".into(),
            explanation: "Whenever you lock the computer while the charger is plugged in, two short beeps will play to remind you to take it.".into(),
        },
    })
}

/// Accepts `E`, `e:`, `E:\` or `E:/` and returns the upper-case letter.
pub fn parse_drive(s: &str) -> Result<char, String> {
    let mut chars = s.trim().chars();
    let letter = chars.next().filter(char::is_ascii_alphabetic);
    let rest: String = chars.collect();
    match (letter, rest.as_str()) {
        (Some(c), "" | ":" | ":\\" | ":/") => Ok(c.to_ascii_uppercase()),
        _ => Err(format!(
            "'{s}' is not a drive letter (expected something like E:)"
        )),
    }
}

pub fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('n') => {
                    chars.next();
                    out.push('\n');
                    continue;
                }
                Some('\\') => {
                    chars.next();
                    out.push('\\');
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

pub fn list_templates() -> String {
    let mut s = String::from("Built-in templates:\n");
    for t in TEMPLATES {
        s.push_str(&format!("\n{}\n  {}\n", t.name, t.usage));
        for line in t.description.lines() {
            s.push_str(&format!("  {line}\n"));
        }
    }
    s
}

pub fn resolve(cli: Cli, launch: &LaunchInfo) -> Result<Action, String> {
    if cli.list_templates {
        return Ok(Action::Info(list_templates()));
    }
    if cli.stop {
        if !cli.args.is_empty() {
            return Err("--stop takes no extra arguments".into());
        }
        return Ok(Action::Stop);
    }

    if !cli.command.is_empty() {
        let while_running = cli
            .template
            .as_deref()
            .is_some_and(|t| t.eq_ignore_ascii_case("while_running"));
        if (cli.template.is_some() && !while_running) || !cli.args.is_empty() {
            return Err(
                "A command after \"--\" can't be combined with that template or with extra \
                 arguments.\n\n\
                 To run a command and wait for it:\n  \
                 PreventShutdownApp -- yt-dlp URL\n\n\
                 To wait for a program that's already running:\n  \
                 PreventShutdownApp --template while_running yt-dlp.exe"
                    .into(),
            );
        }
        let display = command_display_name(&cli.command);
        let mut config = job_config(&display, Condition::Command(cli.command.clone()));
        apply_overrides(&mut config, &cli)?;
        return Ok(Action::Run(config));
    }

    let template = cli.template.clone().or_else(|| {
        let no_input = cli.title.is_none() && cli.body.is_none() && cli.args.is_empty();
        (no_input && launch.removable_exe_drive.is_some()).then(|| "unplug_usb_warning".to_string())
    });

    let mut config = match &template {
        Some(name) => {
            let t = TEMPLATES
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| format!("Unknown template '{name}'.\n\n{}", list_templates()))?;
            (t.build)(&cli.args, launch)?
        }
        None => {
            if !cli.args.is_empty() {
                return Err(format!(
                    "Unexpected argument(s) {:?}: extra arguments only apply with --template",
                    cli.args
                ));
            }
            let body = cli
                .body
                .clone()
                .ok_or("Give --body (and optionally --title), or use --template")?;
            Config {
                title: DEFAULT_TITLE.into(),
                reason: body,
                condition: Condition::Always,
                lock_reminder: false,
                show_window_title: false,
                observe_only: false,
                alert: false,
                stall_ms: 0,
                reminder: ReminderText::default(),
            }
        }
    };

    apply_overrides(&mut config, &cli)?;
    Ok(Action::Run(config))
}

fn apply_overrides(config: &mut Config, cli: &Cli) -> Result<(), String> {
    config.lock_reminder = cli.lock_reminder;
    config.observe_only = cli.observe_only;
    config.alert = cli.alert;
    config.stall_ms = cli.stall;
    if cli.stall > 60_000 {
        return Err("--stall must be 60000 ms or less".into());
    }
    if let Some(title) = &cli.title {
        config.title = title.clone();
    }
    if let Some(body) = &cli.body {
        config.reason = body.clone();
    }

    config.title = unescape(&config.title).replace('\n', " ");
    config.reason = unescape(&config.reason);
    if let Condition::DrivePresent(d) = config.condition {
        config.reason = config.reason.replace("{drive}", &d.to_string());
    }

    if config.reason.trim().is_empty() {
        return Err("The body must not be empty".into());
    }
    let len = config.reason.encode_utf16().count();
    if len > MAX_REASON_UTF16 {
        return Err(format!(
            "The body is {len} characters long; Windows allows at most {MAX_REASON_UTF16}."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str], launch: &LaunchInfo) -> Result<Action, String> {
        let cli = Cli::try_parse_from(std::iter::once("app").chain(args.iter().copied()))
            .map_err(|e| e.to_string())?;
        resolve(cli, launch)
    }

    fn config(args: &[&str], launch: &LaunchInfo) -> Config {
        match run(args, launch).expect("should parse") {
            Action::Run(c) => c,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn plain_body_blocks_always() {
        let c = config(
            &["--title", "Hi", "--body", "line1\\nline2"],
            &LaunchInfo::default(),
        );
        assert_eq!(c.title, "Hi");
        assert_eq!(c.reason, "line1\nline2");
        assert_eq!(c.condition, Condition::Always);
    }

    #[test]
    fn body_is_required_without_template() {
        assert!(run(&["--title", "Hi"], &LaunchInfo::default()).is_err());
    }

    #[test]
    fn stray_args_without_template_are_rejected() {
        assert!(run(&["--body", "x", "E:"], &LaunchInfo::default()).is_err());
    }

    #[test]
    fn usb_template_with_explicit_drive() {
        let c = config(
            &["--template", "unplug_usb_warning", "e:"],
            &LaunchInfo::default(),
        );
        assert_eq!(c.condition, Condition::DrivePresent('E'));
        assert!(c.reason.contains("(E:)"));
    }

    #[test]
    fn usb_template_defaults_to_exe_drive() {
        let launch = LaunchInfo {
            removable_exe_drive: Some('F'),
            ..Default::default()
        };
        let c = config(&["--template", "unplug_usb_warning"], &launch);
        assert_eq!(c.condition, Condition::DrivePresent('F'));
    }

    #[test]
    fn usb_template_from_fixed_drive_falls_back_to_any() {
        let c = config(
            &["--template", "unplug_usb_warning"],
            &LaunchInfo::default(),
        );
        assert_eq!(c.condition, Condition::AnyRemovableDrive);
        assert!(c.reason.contains("{drive}"));
        assert!(c.reminder.explanation.contains("a USB drive is"));
    }

    #[test]
    fn usb_template_any_argument() {
        let launch = LaunchInfo {
            removable_exe_drive: Some('E'),
            ..Default::default()
        };
        for arg in ["any", "ANY"] {
            let c = config(&["--template", "unplug_usb_warning", arg], &launch);
            assert_eq!(c.condition, Condition::AnyRemovableDrive);
        }
    }

    fn laptop() -> LaunchInfo {
        LaunchInfo {
            machine: MachineInfo {
                chassis: Some(10),
                mobile_platform_role: Some(true),
                has_battery: true,
            },
            ..Default::default()
        }
    }

    #[test]
    fn take_charger_on_laptop() {
        let c = config(&["--template", "take_charger"], &laptop());
        assert_eq!(c.condition, Condition::OnAcPower);
        assert!(c.reminder.menu_label.contains("charger"));
        assert!(run(&["--template", "take_charger", "x"], &laptop()).is_err());
    }

    #[test]
    fn take_charger_refuses_all_in_one() {
        let aio = LaunchInfo {
            machine: MachineInfo {
                chassis: Some(13),
                mobile_platform_role: Some(true),
                has_battery: true,
            },
            ..Default::default()
        };
        let err = run(&["--template", "take_charger"], &aio).unwrap_err();
        assert!(
            err.contains("only works on laptops") && err.contains("All in One"),
            "{err}"
        );
    }

    #[test]
    fn overrides_keep_drive_placeholder() {
        let c = config(
            &[
                "--template",
                "unplug_usb_warning",
                "G",
                "--body",
                "Take {drive}: home",
            ],
            &LaunchInfo::default(),
        );
        assert_eq!(c.reason, "Take G: home");
        assert_eq!(c.title, "USB drive still plugged in");
    }

    #[test]
    fn no_args_from_usb_uses_usb_template() {
        let launch = LaunchInfo {
            removable_exe_drive: Some('E'),
            ..Default::default()
        };
        let c = config(&[], &launch);
        assert_eq!(c.condition, Condition::DrivePresent('E'));
    }

    #[test]
    fn no_args_from_fixed_drive_fails() {
        assert!(run(&[], &LaunchInfo::default()).is_err());
    }

    #[test]
    fn lock_reminder_flag() {
        let launch = LaunchInfo {
            removable_exe_drive: Some('E'),
            ..Default::default()
        };
        assert!(!config(&[], &launch).lock_reminder);
        assert!(config(&["--lock-reminder"], &launch).lock_reminder);
    }

    #[test]
    fn reminder_text_follows_template() {
        let usb = config(
            &["--template", "unplug_usb_warning", "E:"],
            &LaunchInfo::default(),
        );
        assert!(usb.reminder.menu_label.contains("unplug"));
        assert!(usb.reminder.explanation.contains("E:"));

        let plain = config(&["--body", "Backup running"], &LaunchInfo::default());
        assert_eq!(plain.reminder, ReminderText::default());
        assert!(!plain.reminder.menu_label.contains("unplug"));
    }

    #[test]
    fn while_running_template() {
        for arg in [
            "VirtualBoxVM",
            "VirtualBoxVM.exe",
            "C:\\Program Files\\VB\\VirtualBoxVM.exe",
        ] {
            let c = config(
                &["--template", "while_running", arg],
                &LaunchInfo::default(),
            );
            assert_eq!(
                c.condition,
                Condition::ProcessRunning("VirtualBoxVM.exe".into())
            );
            assert!(c.condition.is_job());
            assert!(c.show_window_title);
            assert!(c.reason.contains("VirtualBoxVM.exe"));
        }
        assert!(run(&["--template", "while_running"], &LaunchInfo::default()).is_err());
        assert!(run(
            &["--template", "while_running", "a", "b"],
            &LaunchInfo::default()
        )
        .is_err());
    }

    #[test]
    fn command_wrapper() {
        let c = config(
            &[
                "--body",
                "Backing up",
                "--",
                "robocopy.exe",
                "C:\\Data",
                "E:\\Backup",
                "/MIR",
            ],
            &LaunchInfo::default(),
        );
        assert_eq!(
            c.condition,
            Condition::Command(vec![
                "robocopy.exe".into(),
                "C:\\Data".into(),
                "E:\\Backup".into(),
                "/MIR".into()
            ])
        );
        assert_eq!(c.reason, "Backing up");
        assert_eq!(c.title, "Waiting for robocopy");
        assert!(c.condition.is_job());

        let plain = config(
            &["--", "yt-dlp", "--console-title", "URL"],
            &LaunchInfo::default(),
        );
        assert!(plain.reason.contains("yt-dlp"));

        assert!(run(
            &["--template", "take_charger", "--", "x"],
            &LaunchInfo::default()
        )
        .is_err());

        let combined = config(
            &[
                "--template",
                "while_running",
                "--",
                "yt-dlp",
                "--console-title",
                "URL",
            ],
            &LaunchInfo::default(),
        );
        assert_eq!(
            combined.condition,
            Condition::Command(vec![
                "yt-dlp".into(),
                "--console-title".into(),
                "URL".into()
            ])
        );
        assert!(run(
            &["--template", "while_running", "x.exe", "--", "yt-dlp"],
            &LaunchInfo::default()
        )
        .is_err());
    }

    #[test]
    fn reminder_modes_are_not_jobs() {
        assert!(!config(&["--body", "x"], &LaunchInfo::default())
            .condition
            .is_job());
    }

    #[test]
    fn reason_with_window_title() {
        assert_eq!(compose_reason("Wait.", None), "Wait.");
        assert_eq!(compose_reason("Wait.", Some("  ")), "Wait.");
        assert_eq!(
            compose_reason("Wait.", Some("HandBrake (45.21%)")),
            "Wait. Now: HandBrake (45.21%)"
        );
        let long = "x".repeat(400);
        let r = compose_reason("Wait.", Some(&long));
        assert_eq!(r.encode_utf16().count(), MAX_REASON_UTF16);
        assert!(r.ends_with('…'));
        let emoji = "🎬".repeat(200);
        assert!(compose_reason("Wait.", Some(&emoji)).encode_utf16().count() <= MAX_REASON_UTF16);
    }

    #[test]
    fn unknown_template_fails() {
        assert!(run(&["--template", "nope"], &LaunchInfo::default()).is_err());
    }

    #[test]
    fn reason_length_limit() {
        let ok = "a".repeat(MAX_REASON_UTF16);
        let too_long = "a".repeat(MAX_REASON_UTF16 + 1);
        assert!(run(&["--body", &ok], &LaunchInfo::default()).is_ok());
        assert!(run(&["--body", &too_long], &LaunchInfo::default()).is_err());
    }

    #[test]
    fn stop_and_list() {
        assert_eq!(run(&["--stop"], &LaunchInfo::default()), Ok(Action::Stop));
        assert!(matches!(
            run(&["--list-templates"], &LaunchInfo::default()),
            Ok(Action::Info(_))
        ));
    }

    #[test]
    fn drive_parsing() {
        for ok in ["E", "e:", "E:\\", "e:/", " E: "] {
            assert_eq!(parse_drive(ok), Ok('E'), "{ok}");
        }
        for bad in ["", "EE", "1:", "E:\\foo"] {
            assert!(parse_drive(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn unescape_rules() {
        assert_eq!(unescape("a\\nb"), "a\nb");
        assert_eq!(unescape("a\\\\nb"), "a\\nb");
        assert_eq!(unescape("C:\\Temp"), "C:\\Temp");
        assert_eq!(unescape("end\\"), "end\\");
    }
}
