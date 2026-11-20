//! Real binary + PTY: graphics gating, path resolution, and pixel transport.
#![cfg(target_os = "linux")]

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use base64::{Engine, engine::general_purpose::STANDARD};
use rustix::pty::{OpenptFlags, ioctl_tiocgptpeer, openpt, unlockpt};
use rustix::termios::{Winsize, tcsetwinsize};

struct Sandbox(PathBuf);

impl Sandbox {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrisfetch-images-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("settings with spaces")).unwrap();
        fs::create_dir_all(path.join("xdg/ferrisfetch")).unwrap();
        Self(path)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrisfetch"));
        command
            .current_dir(&self.0)
            .env("XDG_CONFIG_HOME", self.0.join("xdg"))
            .env("HOME", &self.0)
            .env("TERM", "xterm-kitty")
            .env("NO_COLOR", "1")
            .env_remove("TERM_PROGRAM")
            .env_remove("TMUX")
            .env_remove("STY");
        command
    }

    fn fixture(&self, path: &str) {
        let path = self.0.join(path);
        if path.extension().unwrap() == "png" {
            image::RgbaImage::from_pixel(40, 20, image::Rgba([250, 60, 10, 128]))
                .save(path)
                .unwrap();
        } else {
            image::RgbImage::from_pixel(40, 20, image::Rgb([250, 60, 10]))
                .save(path)
                .unwrap();
        }
    }

    fn config(&self, path: &str, logo: serde_json::Value) {
        fs::write(
            self.0.join(path),
            serde_json::to_vec(&serde_json::json!({"modules":["os"], "logo":logo})).unwrap(),
        )
        .unwrap();
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tty(mut command: Command, columns: u16, pixels: bool) -> String {
    tty_probe(&mut command, columns, pixels, Some("OK")).0
}

fn tty_probe(
    command: &mut Command,
    columns: u16,
    pixels: bool,
    response: Option<&str>,
) -> (String, usize) {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC).unwrap();
    unlockpt(&master).unwrap();
    let slave = ioctl_tiocgptpeer(
        &master,
        OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC,
    )
    .unwrap();
    tcsetwinsize(
        &slave,
        Winsize {
            ws_col: columns,
            ws_row: 40,
            ws_xpixel: if pixels { columns * 8 } else { 0 },
            ws_ypixel: if pixels { 640 } else { 0 },
        },
    )
    .unwrap();
    let check_slave = slave.try_clone().unwrap();
    let before = rustix::termios::tcgetattr(&check_slave).unwrap();
    command.stdout(Stdio::from(slave)).stderr(Stdio::piped());
    let child = command.spawn().unwrap();
    // Command retains its configured stdout fd; drop it so the master sees EOF.
    command.stdout(Stdio::null());
    let response = response.map(str::to_owned);
    let reader = std::thread::spawn(move || {
        let mut file = File::from(master);
        let mut bytes = Vec::new();
        let mut queries = 0;
        let mut buffer = [0; 8192];
        loop {
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    bytes.extend_from_slice(&buffer[..n]);
                    // Act as a terminal, not just a passive capture. Handle
                    // requests split across reads, remove probes from artwork.
                    if let Some(start) = bytes.windows(6).position(|w| w == b"\x1b_Ga=q")
                        && let Some(end) = bytes[start..].windows(2).position(|w| w == b"\x1b\\")
                    {
                        let end = start + end + 2;
                        let request = std::str::from_utf8(&bytes[start..end]).unwrap();
                        let id = request
                            .split(",i=")
                            .nth(1)
                            .unwrap()
                            .split(',')
                            .next()
                            .unwrap();
                        queries += 1;
                        if let Some(response) = &response {
                            let reply = if response == "unrelated" {
                                "\x1b_Gi=0;OK\x1b\\".to_owned()
                            } else if response == "malformed" {
                                "\x1b_Ginvalid\x1b\\".to_owned()
                            } else {
                                format!("\x1b_Gi={id};{response}\x1b\\")
                            };
                            // Each byte is independently writable/readable, so
                            // the client cannot rely on an atomic response.
                            for byte in reply.as_bytes() {
                                file.write_all(&[*byte]).unwrap();
                            }
                        }
                        bytes.drain(start..end);
                    }
                }
                Err(error) if error.raw_os_error() == Some(5) => break, // PTY EOF on Linux
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("PTY read: {error}"),
            }
        }
        (String::from_utf8(bytes).unwrap(), queries)
    });
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let after = rustix::termios::tcgetattr(&check_slave).unwrap();
    assert_eq!(
        before.local_modes, after.local_modes,
        "probe must restore TTY settings"
    );
    assert_eq!(before.input_modes, after.input_modes);
    assert_eq!(before.output_modes, after.output_modes);
    assert_eq!(before.control_modes, after.control_modes);
    drop(check_slave);
    reader.join().unwrap()
}

fn payload(output: &str) -> (String, Vec<u8>) {
    let mut encoded = String::new();
    let mut first = String::new();
    for (index, chunk) in output.split("\x1b_G").skip(1).enumerate() {
        let (command, _) = chunk.split_once("\x1b\\").unwrap();
        let (header, data) = command.split_once(';').unwrap();
        assert!(data.len() <= 4096);
        if index == 0 {
            first = header.to_owned();
        } else {
            assert!(matches!(header, "m=1" | "m=0"));
        }
        encoded.push_str(data);
    }
    (first, STANDARD.decode(encoded).unwrap())
}

#[test]
fn png_and_jpeg_resolve_from_loaded_config_with_spaces_and_absolute_paths() {
    let sandbox = Sandbox::new();
    for extension in ["png", "jpg", "jpeg"] {
        let relative = format!("settings with spaces/ferris photo.{extension}");
        sandbox.fixture(&relative);
        for source in [
            format!("./ferris photo.{extension}"),
            sandbox.0.join(&relative).to_str().unwrap().to_owned(),
        ] {
            for logo in [
                serde_json::json!(source),
                serde_json::json!({"type":"kitty", "source":source, "width":28}),
            ] {
                sandbox.config("settings with spaces/custom.json", logo);
                let mut command = sandbox.command();
                command.args(["--config", "settings with spaces/custom.json"]);
                let output = tty(command, 120, true);
                let (header, pixels) = payload(&output);
                assert!(
                    header.starts_with("a=T,f=32,t=d,s=224,v=112,C=1,q=2,"),
                    "{header}"
                );
                assert_eq!(pixels.len(), 224 * 112 * 4);
                assert!(output.contains("                               OS"));
                if extension == "png" {
                    assert!(pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 128));
                } else {
                    assert!(pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
                }
            }
        }
    }
}

#[test]
fn default_config_and_cli_precedence_use_their_own_path_bases() {
    let sandbox = Sandbox::new();
    sandbox.fixture("xdg/ferrisfetch/config image.png");
    sandbox.fixture("cli image.png");
    sandbox.config(
        "xdg/ferrisfetch/config.json",
        serde_json::json!({"type":"kitty", "source":"./config image.png", "width":4}),
    );
    let (header, _) = payload(&tty(sandbox.command(), 120, true));
    assert!(header.contains("s=32,v=16,"));
    let mut command = sandbox.command();
    command.args(["--logo", "./cli image.png"]);
    let (header, _) = payload(&tty(command, 120, true));
    assert!(header.contains("s=224,v=112,"));
    for args in [
        vec!["--logo", "arch"],
        vec!["--logo", "off"],
        vec!["--logo-off"],
        vec!["--logo", "./cli image.png", "--logo-off"],
    ] {
        let mut command = sandbox.command();
        command.args(&args);
        let output = tty(command, 120, true);
        assert!(!output.contains("\x1b_G"));
        if !args.contains(&"arch") {
            assert!(output.starts_with("OS"));
        }
    }
}

#[test]
fn bad_files_and_unsupported_terminals_fall_back_to_automatic_builtin() {
    let sandbox = Sandbox::new();
    sandbox.fixture("good.png");
    fs::write(sandbox.0.join("broken.jpg"), b"not an image").unwrap();
    File::create(sandbox.0.join("large.png"))
        .unwrap()
        .set_len(16 * 1024 * 1024 + 1)
        .unwrap();
    image::RgbaImage::new(8193, 1)
        .save(sandbox.0.join("wide.png"))
        .unwrap();
    let mut baseline = sandbox.command();
    baseline.args(["--no-config", "--modules", "os"]);
    let baseline = tty(baseline, 120, true);
    for path in ["missing.png", "broken.jpg", "large.png", "wide.png"] {
        let mut command = sandbox.command();
        command.args(["--no-config", "--modules", "os", "--logo", path]);
        assert_eq!(tty(command, 120, true), baseline);
    }
    for (response, pixels) in [
        (None, true),
        (Some("ENOTSUP:unsupported"), true),
        (Some("unrelated"), true),
        (Some("malformed"), true),
        (Some("OK"), false),
    ] {
        let mut command = sandbox.command();
        command.args(["--no-config", "--modules", "os", "--logo", "good.png"]);
        let (output, queries) = tty_probe(&mut command, 120, pixels, response);
        assert_eq!(output, baseline);
        assert_eq!(queries, usize::from(pixels));
    }
}

#[test]
fn scripted_or_redirected_output_never_contains_graphics_even_with_kitty_env() {
    let sandbox = Sandbox::new();
    sandbox.fixture("xdg/ferrisfetch/ferris.png");
    for logo in [
        serde_json::json!("./ferris.png"),
        serde_json::json!({"type":"kitty", "source":"./ferris.png"}),
    ] {
        sandbox.config("xdg/ferrisfetch/config.json", logo);
        for flag in [None, Some("--pipe"), Some("--json")] {
            let mut command = sandbox.command();
            if let Some(flag) = flag {
                command.arg(flag);
            }
            let output = command.output().unwrap();
            assert!(output.status.success(), "{output:?}");
            assert!(!output.stdout.contains(&0x1b));
            if flag == Some("--json") {
                serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
            }
            if let Some(flag) = flag {
                let mut command = sandbox.command();
                command.arg(flag);
                let (output, queries) = tty_probe(&mut command, 120, true, None);
                assert!(!output.contains('\x1b'));
                assert_eq!(queries, 0);
            }
        }
    }
}

#[test]
fn protocol_support_is_independent_of_terminal_brand_and_environment_hints() {
    let sandbox = Sandbox::new();
    sandbox.fixture("good.png");
    for (term, program) in [
        ("xterm-256color", "UnknownFutureTerminal"),
        ("xterm-kitty", "kitty"),
        ("xterm-ghostty", "ghostty"),
        ("screen-256color", "anything"),
    ] {
        let mut command = sandbox.command();
        command
            .args(["--no-config", "--modules", "os", "--logo", "good.png"])
            .env("TERM", term)
            .env("TERM_PROGRAM", program);
        // A multiplexer that forwards the protocol is allowed too; environment
        // variables alone neither enable nor disable graphics.
        command.env("TMUX", "inherited-hint");
        let (output, queries) = tty_probe(&mut command, 120, true, Some("OK"));
        assert_eq!(queries, 1);
        assert!(output.contains("\x1b_Ga=T,"));
    }
}

#[test]
fn tall_images_reserve_rows_and_narrow_windows_do_not_emit_graphics() {
    let sandbox = Sandbox::new();
    image::RgbaImage::from_pixel(10, 100, image::Rgba([255, 0, 0, 255]))
        .save(sandbox.0.join("tall.png"))
        .unwrap();
    let mut command = sandbox.command();
    command.args(["--no-config", "--modules", "os", "--logo", "./tall.png"]);
    let output = tty(command, 120, true);
    let (header, _) = payload(&output);
    assert!(header.contains("s=62,v=624,"));
    assert!(output.starts_with(&format!("\r{}\x1b[39A\r", "\r\n".repeat(39))));
    assert_eq!(output.split("\x1b\\").last().unwrap().lines().count(), 39);
    assert!(output.ends_with("\r\n"));
    let mut command = sandbox.command();
    command.args(["--no-config", "--modules", "os", "--logo", "./tall.png"]);
    assert!(!tty(command, 8, true).contains("\x1b_G"));
}
