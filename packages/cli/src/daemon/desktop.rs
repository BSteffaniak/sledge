//! Explicit desktop selection; never infer compositor or input method from hints.
use crate::cli::{LinuxDesktop, LinuxInputMethod, RunArgs};
use anyhow::{Context, Result};
use sledge_linux::desktop::worker::{DesktopWorker, InputMethodProvider, WithInputMethod};
use sledge_linux::desktop::{DesktopProvider, companion, hyprland, sway, x11};

pub(super) fn start(args: &RunArgs) -> Result<Option<DesktopWorker>> {
    let Some(selected) = args.linux_desktop else {
        return Ok(None);
    };
    let socket = || {
        args.linux_desktop_socket
            .as_deref()
            .context("selected desktop requires --linux-desktop-socket")
    };
    let mut provider: Box<dyn DesktopProvider> = match selected {
        LinuxDesktop::Gnome => Box::new(companion::Gnome::connect()?),
        LinuxDesktop::Kwin => Box::new(companion::KwinBridge::connect()?),
        LinuxDesktop::Sway => Box::new(sway::Sway::new(socket()?)),
        LinuxDesktop::Hyprland => Box::new(hyprland::Hyprland::new(socket()?)),
        LinuxDesktop::X11 => {
            let cookie_path = args
                .linux_x11_cookie
                .as_ref()
                .context("X11 requires --linux-x11-cookie with raw MIT-MAGIC-COOKIE-1 bytes")?;
            let cookie =
                std::fs::read(cookie_path).context("reading explicitly selected X11 cookie")?;
            anyhow::ensure!(
                cookie.len() == 16,
                "MIT-MAGIC-COOKIE-1 must contain exactly 16 raw bytes"
            );
            let stream = std::os::unix::net::UnixStream::connect(socket()?)
                .context("connecting explicit local X11 socket")?;
            Box::new(x11::X11::connect(
                stream,
                args.linux_x11_screen,
                b"MIT-MAGIC-COOKIE-1".to_vec(),
                cookie,
            )?)
        }
    };
    if let Some(method) = args.linux_input_method {
        let method = match method {
            LinuxInputMethod::Ibus => {
                InputMethodProvider::IBus(sledge_linux::desktop::ibus::IBus::connect()?)
            }
            LinuxInputMethod::Fcitx5 => {
                InputMethodProvider::Fcitx5(sledge_linux::desktop::fcitx5::Fcitx5::connect()?)
            }
        };
        provider = Box::new(WithInputMethod::new(provider, method));
    }
    Ok(Some(DesktopWorker::start(provider)))
}
