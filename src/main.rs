#![cfg_attr(windows, windows_subsystem = "windows")]

mod config;
mod machine;
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

    let launch = win::prepare_process();

    match config::resolve(cli, &launch) {
        Ok(Action::Run(cfg)) => {
            if !cfg.condition.is_job() {
                win::stop_all();
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
