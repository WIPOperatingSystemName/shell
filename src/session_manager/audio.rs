//! Audio servers launched here belong to the authenticated logind scope.
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};
use telorgon::session::{RestartPolicy, SessionHandle, Stream};

pub(super) fn start(session: &SessionHandle, runtime: &Path) -> Result<(), String> {
    // An existing user audio stack may belong to another login. Reuse it rather
    // than taking its lifetime over; only our own children are session-managed.
    let socket = runtime.join("pipewire-0");
    if UnixStream::connect(&socket).is_ok() { return Ok(()); }
    session.command("/usr/bin/pipewire").stdin(Stream::Null)
        .restart(RestartPolicy::OnFailure).spawn().map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(3);
    while UnixStream::connect(&socket).is_err() {
        if Instant::now() >= deadline { return Err("The user audio server did not become ready".into()); }
        std::thread::sleep(Duration::from_millis(25));
    }
    for program in ["/usr/bin/wireplumber", "/usr/bin/pipewire-pulse"] {
        // WirePlumber exits successfully when PipeWire disconnects. It still
        // needs to return when our supervised audio server reconnects.
        session.command(program).stdin(Stream::Null).restart(RestartPolicy::OnExit)
            .spawn().map_err(|error| error.to_string())?;
    }
    Ok(())
}
