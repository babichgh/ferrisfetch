//! Bounded Kitty capability negotiation on stdout's own TTY, never stdin.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::termios::{LocalModes, OptionalActions, Termios, tcgetattr, tcsetattr};

// Restore settings on every exit path; never flush queued user input.
struct Restore<'a>(&'a std::fs::File, Termios);

impl Drop for Restore<'_> {
    fn drop(&mut self) {
        let _ = tcsetattr(self.0, OptionalActions::Now, &self.1);
    }
}

pub(super) fn supported() -> Option<bool> {
    let stdout = std::io::stdout();
    if !std::io::IsTerminal::is_terminal(&stdout) {
        return None;
    }
    // stdin may be a pipe or another terminal. /dev/tty can also differ from
    // stdout, so reopen the output terminal itself to read its response.
    let name = rustix::termios::ttyname(&stdout, Vec::new()).ok()?;
    let mut tty: std::fs::File = rustix::fs::open(
        &name,
        rustix::fs::OFlags::RDWR
            | rustix::fs::OFlags::NOCTTY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )
    .ok()?
    .into();
    if rustix::termios::tcgetpgrp(&tty).is_ok_and(|group| group != rustix::process::getpgrp()) {
        return None; // Never read a background job's controlling terminal.
    }
    let original = tcgetattr(&tty).ok()?;
    let restore_file = tty.try_clone().ok()?;
    let mut mode = original.clone();
    mode.local_modes
        .remove(LocalModes::ICANON | LocalModes::ECHO);
    tcsetattr(&tty, OptionalActions::Now, &mode).ok()?;
    let _restore = Restore(&restore_file, original);
    if rustix::io::ioctl_fionread(&tty).ok()? != 0 {
        return None; // Leave pre-existing input untouched, including partial lines.
    }

    let id = std::process::id().max(1);
    let request = format!("\x1b_Ga=q,i={id},t=d,f=32,s=1,v=1;AAAAAA==\x1b\\");
    let mut output = stdout.lock();
    output.write_all(request.as_bytes()).ok()?;
    output.flush().ok()?;
    drop(output);

    let deadline = Instant::now() + Duration::from_millis(200);
    let mut reply = Vec::new();
    for _ in 0..1024 {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        let timeout = Timespec::try_from(remaining).ok()?;
        let mut descriptors = [PollFd::new(&tty, PollFlags::IN)];
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(0) => return None,
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => return None,
        }
        // Read through the response terminator only, not any following keys.
        let mut byte = [0];
        match tty.read(&mut byte) {
            Ok(1) => reply.push(byte[0]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            _ => return None,
        }
        if reply.ends_with(b"\x1b\\") {
            if let Some(supported) = parse_reply(&reply, id) {
                return Some(supported);
            }
            reply.clear(); // Ignore unrelated/stale protocol responses.
        }
    }
    None
}

fn parse_reply(bytes: &[u8], id: u32) -> Option<bool> {
    let reply = std::str::from_utf8(bytes)
        .ok()?
        .strip_prefix("\x1b_G")?
        .strip_suffix("\x1b\\")?;
    let (header, status) = reply.split_once(';')?;
    let expected = format!("i={id}");
    header
        .split(',')
        .any(|field| field == expected)
        .then_some(status == "OK")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_must_confirm_our_query_not_a_terminal_name() {
        assert_eq!(parse_reply(b"\x1b_Gi=123;OK\x1b\\", 123), Some(true));
        assert_eq!(parse_reply(b"\x1b_Gp=1,i=123;OK\x1b\\", 123), Some(true));
        assert_eq!(
            parse_reply(b"\x1b_Gi=123;EINVAL:unsupported\x1b\\", 123),
            Some(false)
        );
        for bytes in [
            b"\x1b_Gi=12;OK\x1b\\".as_slice(),
            b"\x1b_Gi=123;OK",
            b"junk",
            b"\x1b[?1;2c",
        ] {
            assert_eq!(parse_reply(bytes, 123), None);
        }
    }
}
