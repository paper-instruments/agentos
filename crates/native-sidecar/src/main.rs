#[cfg(unix)]
use std::os::fd::{FromRawFd, OwnedFd};

#[cfg(unix)]
use nix::fcntl::{fcntl, FcntlArg};

#[cfg(unix)]
const CONTROL_FD: i32 = 3;

fn main() {
    // Default to WARN so near-limit / backpressure warnings actually surface
    // (they were swallowed at ERROR-only); operators can tune via AGENTOS_LOG
    // (e.g. `error` to quiet, `debug` for queue snapshots). Logs MUST go to stderr:
    // stdout is the framed wire-protocol channel, so logging there would corrupt it.
    let level = std::env::var("AGENTOS_LOG")
        .ok()
        .and_then(|value| value.parse::<tracing::Level>().ok())
        .unwrap_or(tracing::Level::WARN);
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(level)
        .init();
    #[cfg(unix)]
    let control_endpoint = {
        if let Err(error) = fcntl(CONTROL_FD, FcntlArg::F_GETFD) {
            tracing::error!(
                ?error,
                fd = CONTROL_FD,
                "missing inherited sidecar response/control descriptor"
            );
            std::process::exit(1);
        }
        // SAFETY: the process launch contract reserves fd 3 for the inherited
        // response/control socket and transfers its sole ownership to the sidecar.
        // The fcntl probe above establishes that the descriptor is open before it
        // is adopted.
        unsafe { OwnedFd::from_raw_fd(CONTROL_FD) }
    };
    #[cfg(windows)]
    let control_endpoint = match std::env::var("AGENTOS_CONTROL_PIPE") {
        Ok(pipe_name) if !pipe_name.is_empty() => pipe_name,
        Ok(_) | Err(_) => {
            tracing::error!("missing AGENTOS_CONTROL_PIPE for sidecar control channel");
            std::process::exit(1);
        }
    };
    if let Err(error) = agentos_native_sidecar::stdio::run(control_endpoint) {
        tracing::error!(?error, "agentos-native-sidecar startup failed");
        std::process::exit(1);
    }
}
