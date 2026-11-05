#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
mod audio;
mod config;
#[cfg(windows)]
#[macro_use]
mod log;
mod machine;
mod memory;
#[cfg_attr(not(windows), allow(dead_code))]
mod sound;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    use clap::Parser;
    use config::{Action, Cli};

    std::panic::set_hook(Box::new(|info| {
        win::message_box(
            "PreventShutdownApp crashed",
            &format!(
                "PreventShutdownApp hit an internal error and has to close.\n\n{info}\n\n\
                 Shutdown is no longer being blocked."
            ),
            true,
        );
    }));

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            let is_error = e.use_stderr();
            win::message_box("PreventShutdownApp", &e.to_string(), is_error);
            std::process::exit(if is_error { 2 } else { 0 });
        }
    };

    if cli.log || cli.log_file.is_some() {
        let target = log::init(cli.log_file.as_deref());
        match &target {
            Some(path) => {
                log_line!(
                    "================ PreventShutdownApp {} starting ================",
                    env!("CARGO_PKG_VERSION")
                );
                log_line!(
                    "exe: {}",
                    std::env::current_exe()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|_| "<unknown>".into())
                );
                log_line!("command line: {:?}", std::env::args().collect::<Vec<_>>());
                log_line!("log file: {}", path.display());
            }
            None => win::message_box(
                "PreventShutdownApp",
                "Couldn't open a log file next to the exe or in the temp folder.\n\n\
                 Try --log-file with a path you can write to.",
                true,
            ),
        }
    }

    if cli.audio_check {
        let state = audio::probe();
        log_line!("--audio-check: {} ({:?})", state.describe(), state);
        win::message_box(
            "PreventShutdownApp - alert sound check",
            &format!(
                "Alert sound on this PC: {}\n\n{}",
                state.describe(),
                if state.can_be_heard() {
                    "An alert would be played if a shutdown is attempted with --alert."
                } else {
                    "--alert would stay silent on this PC, so it is not used. \
                     Fix the volume or the audio service first, then check again."
                }
            ),
            !state.can_be_heard(),
        );
        std::process::exit(if state.can_be_heard() { 0 } else { 1 });
    }

    let launch = win::prepare_process(cli.as_host.as_deref());

    let memory_path = if cli.no_memory {
        None
    } else {
        cli.memory_file
            .as_deref()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(|dir| dir.join(memory::FILE_NAME)))
            })
    };
    if let Some(id) = launch.identity.clone() {
        win::init_memory(id, memory_path);
    }

    if cli.memory {
        let report = win::memory_report();
        log_line!("--memory:\n{report}");
        win::message_box("PreventShutdownApp - remembered machines", &report, false);
        std::process::exit(0);
    }

    match config::resolve(cli, &launch) {
        Ok(Action::Run(cfg)) => {
            if !cfg.condition.is_job() {
                win::stop_all();
            }
            if cfg.observe_only {
                std::thread::spawn(|| {
                    win::message_box(
                    "PreventShutdownApp - diagnostic mode",
                    "--observe-only is on.

                     Shutdown will NOT be blocked. Every shutdown is allowed and only logged, so this protects nothing. Use it to find out how this PC treats a shutdown, then run again without the flag.",
                    false,
                )
                });
            }
            match win::run(cfg) {
                Ok(code) => std::process::exit(code),
                Err(e) => {
                    win::message_box("PreventShutdownApp", &e, true);
                    std::process::exit(1);
                }
            }
        }
        Ok(Action::Stop) => {
            win::stop_all();
        }
        Ok(Action::Info(text)) => win::message_box("PreventShutdownApp", &text, false),
        Err(e) => {
            win::message_box("PreventShutdownApp", &e, true);
            std::process::exit(2);
        }
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("PreventShutdownApp only runs on Windows.");
    std::process::exit(1);
}
