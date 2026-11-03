#![windows_subsystem = "windows"]

mod app;
mod arrange;
mod arrange_overlay;
mod arrange_win;
mod audio;
mod behind;
mod calendar;
mod capture;
mod claude;
mod clean;
mod cli;
mod cli_folders;
mod cli_panel;
mod clipboard;
mod config;
mod data_dir;
mod devices;
mod devices_win;
mod dpapi;
mod eq_audio;
mod gestures;
mod google;
mod greeting;
mod halo;
mod hotkey;
mod i18n;
mod identify_overlay;
mod lyrics;
mod lyrics_card;
mod media_log;
mod model;
mod mole;
mod notes;
mod notch_monitor;
mod notif;
mod pages;
mod pointer_watch;
mod pomodoro;
mod reminders;
mod screenshot;
mod search;
mod search_details;
mod search_files;
mod shelf;
mod shelf_ole;
mod shortcuts;
mod shot_edit;
mod sources;
mod state;
mod system;
mod theme;
mod tiles;
mod ui;
mod update;
mod widgets;

use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

/// The release binary uses the GUI subsystem, so CLI modes must attach to the
/// launching console before anything they print becomes visible.
fn attach_parent_console() {
    // Fails harmlessly when there is no parent console (e.g. started by a hook runner with pipes).
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Demo modes (debug builds) read no config: `DI_LANG=en` picks English.
#[cfg(debug_assertions)]
fn demo_lang_from_env() {
    let code = std::env::var("DI_LANG").unwrap_or_default();
    i18n::set_lang(i18n::Lang::from_code(&code).unwrap_or_default());
}

/// Modes that create windows: they need the embedded fonts registered with
/// GDI before the first one opens.
fn opens_windows(arg: Option<&str>) -> bool {
    match arg {
        None => true,
        Some(media_log::FLAG) => true,
        // The updater starts the new exe with it: the island as usual.
        Some("--updated") => true,
        #[cfg(debug_assertions)]
        Some(
            "--demo"
            | "--demo-editor"
            | "--demo-cli"
            | "--demo-robot"
            | "--demo-devices"
            | "--demo-arrange"
            | "--demo-notice-close"
            | "--demo-widgets"
            | "--demo-snap"
            | "--demo-shot-editor"
            | "--demo-lyrics-card"
            | "--settings",
        ) => true,
        Some(_) => false,
    }
}

fn main() {
    let arg = std::env::args().nth(1);
    if opens_windows(arg.as_deref()) {
        ui::fonts::register_gdi();
    }
    match arg.as_deref() {
        None => app::run(false, None),
        // The island as usual, with the media diagnosis log on. No console:
        // it runs like a normal start, single instance included.
        Some(media_log::FLAG) => app::run(true, None),
        // Started by the updater (`update::launch_updated`) as `--updated
        // <old version>`: the island as usual, but it waits for the old
        // one's mutex instead of quitting, and says it was updated.
        Some("--updated") => {
            let old = std::env::args().nth(2).unwrap_or_default();
            app::run(false, Some(&old));
        }
        // No console attach: the hook prints nothing, and attaching can swap an
        // unset stdin for the console's, leaving the hook blocked on keyboard input.
        Some("--hook") => claude::hook::run_hook(),
        // The cleaner's administrator helper, started elevated by the
        // island itself (UAC): it removes the Windows Update leftovers and
        // exits with a status. No window, no console, no island.
        Some(clean::ADMIN_FLAG) => {
            let job = std::env::args().nth(2);
            std::process::exit(clean::run_admin(job.as_deref()));
        }
        Some("--probe-notifications") => {
            attach_parent_console();
            sources::notifications::probe();
        }
        // `--probe-open <app name> [AUMID|-] [site|-] [title]`: what opening
        // such a toast would do. Looks only; nothing is selected or started.
        Some("--probe-open") => {
            attach_parent_console();
            sources::notif_open::probe(std::env::args().skip(2).collect());
        }
        // `<hook payload> | vale --probe-claude-open`: the origin the hook
        // would capture here and what a click on its notice would do. Looks
        // only; nothing is focused, invoked, selected or opened.
        Some("--probe-claude-open") => {
            attach_parent_console();
            sources::claude_open::probe();
        }
        #[cfg(debug_assertions)]
        Some("--probe-media") => {
            attach_parent_console();
            sources::media::probe();
        }
        #[cfg(debug_assertions)]
        Some("--probe-system") => {
            attach_parent_console();
            sources::battery::probe();
        }
        #[cfg(debug_assertions)]
        Some("--demo") => {
            attach_parent_console();
            demo_lang_from_env();
            ui::demo::run();
        }
        #[cfg(debug_assertions)]
        Some("--demo-editor") => {
            attach_parent_console();
            demo_lang_from_env();
            ui::editor::run_demo(std::env::args().nth(2).as_deref());
        }
        #[cfg(debug_assertions)]
        Some("--demo-cli") => {
            attach_parent_console();
            demo_lang_from_env();
            ui::demo::run_cli();
        }
        #[cfg(debug_assertions)]
        Some("--demo-robot") => {
            attach_parent_console();
            demo_lang_from_env();
            ui::demo::run_robot();
        }
        // Only the Devices page's scenes: made-up drives, no scan, no eject.
        #[cfg(debug_assertions)]
        Some("--demo-devices") => {
            attach_parent_console();
            demo_lang_from_env();
            ui::demo::run_devices();
        }
        // Only the Susun page's scenes: made-up windows and monitors, click-through
        // overlays; it never calls the mover and never lists a real window.
        #[cfg(debug_assertions)]
        Some("--demo-arrange") => {
            attach_parent_console();
            demo_lang_from_env();
            ui::demo::run_arrange();
        }
        // The desktop widgets alone, on a made-up notes file in the temp
        // folder; `reset` seeds it again.
        #[cfg(debug_assertions)]
        Some("--demo-widgets") => {
            attach_parent_console();
            demo_lang_from_env();
            let reset = std::env::args().nth(2).as_deref() == Some("reset");
            widgets::demo::run(reset);
        }
        // The notice ✕ on a real `IslandState` fed made-up notices: effects are
        // printed, never run.
        #[cfg(debug_assertions)]
        Some("--demo-notice-close") => {
            attach_parent_console();
            demo_lang_from_env();
            ui::demo::run_notice_close();
        }
        // The capture overlay on a desktop drawn in code: no screen grab, no
        // window list, no UI Automation.
        #[cfg(debug_assertions)]
        Some("--demo-snap") => {
            attach_parent_console();
            demo_lang_from_env();
            // "inline": the picked region stays on screen to draw on.
            let inline = std::env::args().nth(2).as_deref() == Some("inline");
            capture::demo::run_snap(inline);
        }
        // The screenshot editor on a made-up picture ("empty": no annotations).
        #[cfg(debug_assertions)]
        Some("--demo-shot-editor") => {
            attach_parent_console();
            demo_lang_from_env();
            let empty = std::env::args().nth(2).as_deref() == Some("empty");
            shot_edit::window::run_demo(empty);
        }
        // The lyrics card on made-up data: `--demo-lyrics-card [scene]`.
        #[cfg(debug_assertions)]
        Some("--demo-lyrics-card") => {
            attach_parent_console();
            demo_lang_from_env();
            let scene = std::env::args().nth(2);
            lyrics_card::demo::run(scene.as_deref());
        }
        #[cfg(debug_assertions)]
        Some("--settings") => {
            attach_parent_console();
            demo_lang_from_env();
            let args: Vec<String> = std::env::args().skip(2).collect();
            ui::settings::run_demo(&args);
        }
        Some("--install-hooks") => {
            attach_parent_console();
            claude::install::install_hooks();
        }
        Some("--uninstall-hooks") => {
            attach_parent_console();
            claude::install::uninstall_hooks();
        }
        Some(other) => {
            attach_parent_console();
            eprintln!("vale: unknown argument `{other}`");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_admin_helper_opens_no_window() {
        assert!(!opens_windows(Some(clean::ADMIN_FLAG)));
        assert!(opens_windows(None));
    }
}
