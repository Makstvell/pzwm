//! `pzwm msg …`: a line-based unix socket for scripts and bars.
//!
//! Requests are one line: any bind action (`workspace 3`, `zoom-in`,
//! `spawn foot`), `windows` / `outputs` for state as text, and the
//! `pointer …` family that drives the pointer for tests.

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    time::Duration,
};

use smithay::{
    backend::input::ButtonState,
    reexports::calloop::{Interest, Mode, PostAction, generic::Generic},
    utils::Point,
};
use tracing::{info, warn};

use crate::{action::Action, input, state::State};

pub fn socket_path(wayland_display: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    dir.join(format!("pzwm-{wayland_display}.sock"))
}

pub fn init(state: &mut State) -> anyhow::Result<PathBuf> {
    let path = socket_path(&state.socket_name.to_string_lossy());
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    state
        .loop_handle
        .insert_source(
            Generic::new(listener, Interest::READ, Mode::Level),
            |_, listener, state| {
                while let Ok((stream, _)) = listener.accept() {
                    if let Err(err) = serve(state, stream) {
                        warn!("ipc: {err}");
                    }
                }
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| anyhow::anyhow!("ipc source: {e}"))?;
    info!("ipc socket {}", path.display());
    Ok(path)
}

fn serve(state: &mut State, stream: UnixStream) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line)?;
    let reply = handle(state, line.trim());
    (&stream).write_all(reply.as_bytes())?;
    (&stream).write_all(b"\n")
}

fn handle(state: &mut State, request: &str) -> String {
    let words: Vec<&str> = request.split_whitespace().collect();
    match words.as_slice() {
        ["windows"] => windows(state),
        ["outputs"] => outputs(state),
        ["pointer", rest @ ..] => pointer(state, rest),
        ["profile", rest @ ..] => match state.profile_command(rest) {
            Ok(msg) => msg,
            Err(err) => format!("error: {err}"),
        },
        ["reload"] => match state.reload_config() {
            Ok(()) => "ok: config reloaded".into(),
            Err(err) => format!("error: {err}"),
        },
        _ => match Action::parse(request) {
            Some(action) => {
                state.do_action(action);
                "ok".into()
            }
            None => format!("error: unknown request `{request}`"),
        },
    }
}

fn windows(state: &State) -> String {
    let mut out = String::new();
    for m in &state.windows {
        let title = state.app_id(m).unwrap_or_default();
        let size = m.size();
        let focused = state.focused.as_ref() == Some(&m.window);
        out.push_str(&format!(
            "{} app={} ws={} placement={:?} pos={:.0},{:.0} size={}x{} focused={}\n",
            m.id,
            title,
            m.ws + 1,
            m.placement,
            m.pos.x,
            m.pos.y,
            size.w,
            size.h,
            focused
        ));
    }
    out.trim_end().to_string()
}

fn outputs(state: &State) -> String {
    state
        .outputs
        .iter()
        .map(|o| {
            let g = state.output_geometry(o);
            let cam = state.camera(o);
            format!(
                "{} {}x{}+{}+{} scale={} ws={} camera={:.1},{:.1} zoom={:.3}",
                o.name(),
                g.size.w,
                g.size.h,
                g.loc.x,
                g.loc.y,
                o.current_scale().fractional_scale(),
                state.output_ws(o) + 1,
                cam.pos.x,
                cam.pos.y,
                cam.zoom
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `pointer move X Y`, `pointer button left|middle|right press|release`,
/// `pointer scroll DY`, `pointer key-super down|up` — synthetic input.
fn pointer(state: &mut State, args: &[&str]) -> String {
    let time = state.now_ms();
    match args {
        ["move", x, y] => match (x.parse::<f64>(), y.parse::<f64>()) {
            (Ok(x), Ok(y)) => {
                state.cursor = Point::from((x, y));
                state.clamp_cursor();
                state.synthetic_motion(time);
                "ok".into()
            }
            _ => "error: bad coordinates".into(),
        },
        ["button", button, action] => {
            let code = match *button {
                "left" => input::BTN_LEFT,
                "right" => input::BTN_RIGHT,
                "middle" => input::BTN_MIDDLE,
                _ => return "error: bad button".into(),
            };
            let pressed = match *action {
                "press" => ButtonState::Pressed,
                "release" => ButtonState::Released,
                _ => return "error: press or release".into(),
            };
            state.synthetic_button(code, pressed, time);
            "ok".into()
        }
        ["scroll", dy] => match dy.parse::<f64>() {
            Ok(dy) => {
                state.synthetic_wheel(dy);
                "ok".into()
            }
            Err(_) => "error: bad amount".into(),
        },
        _ => "error: pointer move X Y | button B press|release | scroll DY".into(),
    }
}

/// Client side of `pzwm msg`.
pub fn send(request: &str) -> anyhow::Result<String> {
    let path = match std::env::var("PZWM_SOCKET") {
        Ok(p) => PathBuf::from(p),
        Err(_) => {
            let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-1".into());
            socket_path(&display)
        }
    };
    let mut stream = UnixStream::connect(&path).map_err(|e| {
        anyhow::anyhow!(
            "cannot reach pzwm at {} ({e}); is it running?",
            path.display()
        )
    })?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(b"\n")?;
    // The server writes the whole reply and closes the connection.
    let mut reply = String::new();
    std::io::Read::read_to_string(&mut stream, &mut reply)?;
    Ok(reply.trim_end().to_string())
}
