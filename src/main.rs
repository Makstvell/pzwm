//! pzwm — an infinite-canvas Wayland compositor.

mod action;
mod backend;
mod bar;
mod bindings;
mod canvas;
mod config;
mod cursor;
mod events;
mod find;
mod gamma;
mod grabs;
mod groups;
mod handlers;
mod input;
mod ipc;
mod jumps;
mod minimap;
mod navigation;
mod output_management;
mod profiles;
mod protocols;
mod render;
mod rules;
mod scratchpad;
mod screencopy;
mod shell;
mod state;
mod tablet;
mod toplevels;

use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use smithay::reexports::calloop::{
    EventLoop,
    timer::{TimeoutAction, Timer},
};
use tracing::{info, warn};

use crate::{config::Config, state::State};

const HELP: &str = "\
pzwm — infinite-canvas Wayland compositor

USAGE: pzwm [OPTIONS]
       pzwm reload          re-read the config in the running session
                            (it also reloads by itself when the file is saved)
       pzwm msg <request>   talk to a running pzwm, e.g.
                            pzwm msg workspace 2 | zoom-in | overview | windows | outputs
                            pzwm msg windows --json | outputs --json
                            pzwm msg subscribe   stream JSON events, one per line

OPTIONS:
  --nested          run in a window inside the current compositor
  --tty             run on the TTY (default when no compositor is running)
  --autostart       also run [general] autostart when nested (always on a TTY)
  --check-config [FILE]
                    validate ~/.config/pzwm/config.toml (or FILE) and exit
  --print-default-config
                    print the documented default config
  -V, --version     print version
  -h, --help        print this help

Logs go to stderr; set RUST_LOG=debug for more detail.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{HELP}");
        return;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("pzwm {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|a| a == "--print-default-config") {
        print!("{}", config::DEFAULT_CONFIG);
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--check-config") {
        let file = args.get(i + 1).filter(|a| !a.starts_with("--"));
        std::process::exit(check_config(file.map(PathBuf::from)));
    }
    // Installed as `umbriel` too, for DankMaterialShell (see bar.rs).
    let as_umbriel = std::env::args()
        .next()
        .is_some_and(|a| Path::new(&a).file_name().is_some_and(|n| n == "umbriel"));
    let request = match args.first().map(String::as_str) {
        Some("msg") if as_umbriel => match bar::translate_umbriel(&args[1..].join(" ")) {
            Some(request) => Some(request),
            None => std::process::exit(1),
        },
        Some("msg") => Some(args[1..].join(" ")),
        Some("reload") => Some("reload".to_string()),
        _ => None,
    };
    if request.as_deref().map(str::trim) == Some("subscribe") {
        if let Err(err) = ipc::subscribe() {
            eprintln!("pzwm: {err:#}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(request) = request {
        match ipc::send(&request) {
            Ok(reply) => {
                println!("{reply}");
                std::process::exit(if reply.starts_with("error") { 1 } else { 0 });
            }
            Err(err) => {
                eprintln!("pzwm: {err:#}");
                std::process::exit(1);
            }
        }
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,smithay=warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let nested = if args.iter().any(|a| a == "--nested") {
        true
    } else if args.iter().any(|a| a == "--tty") {
        false
    } else {
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some()
    };

    if let Err(err) = run(nested) {
        eprintln!("pzwm: {err:#}");
        std::process::exit(1);
    }
}

fn check_config(file: Option<PathBuf>) -> i32 {
    let path = file.unwrap_or_else(Config::path);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) => {
            println!(
                "{}: {err} (built-in defaults would be used)",
                path.display()
            );
            return 0;
        }
    };
    match Config::parse(&text) {
        Ok(cfg) => {
            let problems: Vec<String> = cfg
                .rules
                .iter()
                .flat_map(rules::WindowRule::problems)
                .collect();
            for problem in &problems {
                println!("{}: warning: {problem}", path.display());
            }
            println!(
                "{}: OK, {} key bindings, {} window rules",
                path.display(),
                cfg.keybinds.len(),
                cfg.rules.len()
            );
            0
        }
        Err(err) => {
            println!("{}: {err}", path.display());
            1
        }
    }
}

fn run(nested: bool) -> anyhow::Result<()> {
    let config = Config::load();
    let mut event_loop: EventLoop<'static, State> = EventLoop::try_new()?;
    let mut state = if nested {
        backend::winit::init(&mut event_loop, config)?
    } else {
        backend::udev::init(&mut event_loop, config)?
    };

    let socket = state.socket_name.to_string_lossy().into_owned();
    info!("listening on WAYLAND_DISPLAY={socket}");
    let ipc_path = ipc::init(&mut state)?;
    let bar_path = bar::init(&mut state)?;
    state.watch_config();
    // SAFETY: still single-threaded; nothing else reads the environment concurrently.
    unsafe {
        std::env::set_var("WAYLAND_DISPLAY", &socket);
        std::env::set_var("XDG_CURRENT_DESKTOP", "pzwm");
        std::env::set_var("XDG_SESSION_DESKTOP", "pzwm");
        std::env::set_var("XDG_SESSION_TYPE", "wayland");
        std::env::set_var("PZWM_SOCKET", &ipc_path);
        // DankMaterialShell's workspace widget, see bar.rs.
        std::env::set_var("UMBRIEL_SOCKET", &bar_path);
        std::env::set_var("XCURSOR_THEME", &state.config.general.cursor_theme);
        std::env::set_var("XCURSOR_SIZE", state.config.general.cursor_size.to_string());
        std::env::remove_var("DISPLAY");
    }

    if state.config.general.xwayland {
        start_xwayland_satellite(&mut state);
    }
    start_parallax_tick(&mut state);
    let real_session = !nested && state.config.general.systemd_session;
    if real_session {
        start_systemd_session(&state);
    }
    // Nested runs are for trying things out: don't start a second copy of
    // the session daemons (idle, clipboard, wallpaper) unless asked.
    if !nested || std::env::args().any(|a| a == "--autostart") {
        for cmd in state.config.general.autostart.clone() {
            state.spawn(&cmd);
        }
    }

    // SIGTERM/SIGINT end the session cleanly (children, systemd target).
    let signals = calloop::signals::Signals::new(&[
        calloop::signals::Signal::SIGTERM,
        calloop::signals::Signal::SIGINT,
    ])?;
    event_loop
        .handle()
        .insert_source(signals, |_, _, state| state.loop_signal.stop())
        .map_err(|e| anyhow::anyhow!("signal source: {e}"))?;

    event_loop.run(None, &mut state, |state| {
        state.popups.cleanup();
        state.bar_publish();
        state.events_publish();
        state.toplevels_refresh();
        state.output_management_refresh();
        if let Err(err) = state.display_handle.flush_clients() {
            warn!("flush_clients: {err}");
        }
    })?;

    info!("shutting down");
    if real_session {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", "pzwm-session.target"])
            .status();
    }
    for child in &mut state.children {
        let _ = child.kill();
    }
    let _ = std::fs::remove_file(&ipc_path);
    let _ = std::fs::remove_file(&bar_path);
    Ok(())
}

/// How often a drifting wallpaper is repainted. It moves a few pixels per
/// second, so this is smooth without waking up on every vblank.
const PARALLAX_TICK: Duration = Duration::from_millis(33);

/// Repaints the outputs while wallpaper parallax is on (read on every tick,
/// so toggling it in the config takes effect without a restart).
fn start_parallax_tick(state: &mut State) {
    let tick = state
        .loop_handle
        .insert_source(Timer::immediate(), |_, _, state| {
            if state.config.parallax.enabled {
                state.queue_redraw_all();
            }
            TimeoutAction::ToDuration(PARALLAX_TICK)
        });
    if let Err(err) = tick {
        warn!("parallax timer: {err}");
    }
}

/// X11 apps through xwayland-satellite, when it is installed.
fn start_xwayland_satellite(state: &mut State) {
    let Some(number) = (0..32).find(|n| !Path::new(&format!("/tmp/.X11-unix/X{n}")).exists())
    else {
        return;
    };
    let x_display = format!(":{number}");
    let spawned = Command::new("xwayland-satellite")
        .arg(&x_display)
        .env("WAYLAND_DISPLAY", &state.socket_name)
        .stdin(Stdio::null())
        .spawn();
    match spawned {
        Ok(child) => {
            info!("X11 apps: xwayland-satellite on DISPLAY={x_display}");
            state.children.push(child);
            // SAFETY: still single-threaded.
            unsafe { std::env::set_var("DISPLAY", &x_display) };
            state.xwayland_display = Some(x_display);
        }
        Err(_) => info!("xwayland-satellite not installed; X11 apps disabled"),
    }
}

/// Exports the session environment and starts `pzwm-session.target`, which
/// brings up `graphical-session.target` and everything bound to it (bars
/// like DankMaterialShell, portals, polkit agents).
fn start_systemd_session(state: &State) {
    let mut vars = vec![
        "WAYLAND_DISPLAY",
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "XDG_SESSION_TYPE",
        "PZWM_SOCKET",
        "UMBRIEL_SOCKET",
        "XCURSOR_THEME",
        "XCURSOR_SIZE",
    ];
    if state.xwayland_display.is_some() {
        vars.push("DISPLAY");
    }
    let ok = |cmd: &mut Command| {
        cmd.stdin(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    };
    if !ok(Command::new("systemctl")
        .args(["--user", "import-environment"])
        .args(&vars))
    {
        warn!("systemctl --user import-environment failed (no systemd user session?)");
        return;
    }
    let _ = ok(Command::new("dbus-update-activation-environment")
        .arg("--systemd")
        .args(&vars));
    // Start without waiting: units may take a while and must see our socket.
    let started = Command::new("systemctl")
        .args(["--user", "--no-block", "start", "pzwm-session.target"])
        .stdin(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if started {
        info!("started pzwm-session.target");
    } else {
        warn!("pzwm-session.target missing — run ./install.sh to install the session files");
    }
}
