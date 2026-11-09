# ferrisfetch

[![CI](https://img.shields.io/github/actions/workflow/status/babichgh/ferrisfetch/ci.yml?branch=dev&label=CI)](https://github.com/babichgh/ferrisfetch/actions/workflows/ci.yml)
[![GitHub release](https://img.shields.io/github/v/release/babichgh/ferrisfetch?logo=github)](https://github.com/babichgh/ferrisfetch/releases/latest)
[![crates.io](https://img.shields.io/crates/v/ferrisfetch?logo=rust)](https://crates.io/crates/ferrisfetch)
[![Downloads](https://img.shields.io/crates/d/ferrisfetch)](https://crates.io/crates/ferrisfetch)
[![License](https://img.shields.io/github/license/babichgh/ferrisfetch)](LICENSE)

**ferrisfetch** is a [Fastfetch](https://github.com/fastfetch-cli/fastfetch)-inspired
system information tool written in Rust. It displays your system's details next
to an ASCII logo, with selectable modules, automatic colours, and plain-text or
JSON output for scripts

- **531 built-in logos**, vendored from Fastfetch
- **14 system information modules**, shown in the order you choose
- **Terminal-aware layout** that hides the logo when space is limited
- **Script-friendly output** with `--pipe`, `--json`, and automatic colour detection

> Linux is the primary target. Prebuilt RPM and DEB packages target x86-64;
> module availability depends on your hardware and desktop session

## Preview

![ferrisfetch displaying system information alongside the Omarchy logo](screenshots/preview.png)

## Installation

### Cargo

Install the latest published version from [crates.io](https://crates.io/crates/ferrisfetch)
using a recent stable Rust toolchain:

```sh
cargo install ferrisfetch
```

### Linux packages

Download the appropriate package from the
[latest GitHub release](https://github.com/babichgh/ferrisfetch/releases/latest),
then run the corresponding command in your download directory:

| Distribution | Package | Install |
|---|---|---|
| Fedora | `.x86_64.rpm` | `sudo dnf install ./ferrisfetch-*.x86_64.rpm` |
| Debian / Ubuntu | `_amd64.deb` | `sudo apt install ./ferrisfetch_*_amd64.deb` |

Release assets include `checksums.txt` for SHA-256 verification. These are local
package installs; ferrisfetch is not currently advertised as a package in your
distribution's official repositories

## Build from source

With Git and a recent stable Rust toolchain installed:

```sh
git clone https://github.com/babichgh/ferrisfetch.git
cd ferrisfetch
cargo install --path .
```

To build without installing, run `cargo build --release --locked`. The binary
will be at `target/release/ferrisfetch`

## Usage

```sh
# Display system information with your distribution's logo
ferrisfetch

# Choose modules and their order
ferrisfetch --modules os,host,kernel,cpu,ram

# Hide modules from the default output
ferrisfetch --hide-modules gpu,monitor

# Explore the available options
ferrisfetch --help
```

### Available modules

| Module | Information |
|---|---|
| `os` | Operating system / distribution |
| `host` | User and hostname |
| `kernel` | Kernel name and version |
| `uptime` | Time since boot |
| `cpu` | Processor, core count, and frequency (no sampling delay) |
| `cpu-usage` | Sampled processor usage (opt-in; waits 200 ms by default) |
| `gpu` | Detected graphics hardware |
| `monitor` | Active displays, resolution, refresh rate, and size when available |
| `ram` | Used and total memory |
| `disk` | Used and available space, usage percentage, and filesystem type |
| `shell` | Current shell name from parent processes; login shell fallback (opt-in) |
| `terminal` | Terminal emulator or tmux/screen endpoint name (opt-in) |
| `de` | Desktop environment name from XDG session variables (opt-in) |
| `wm` | Hyprland, Sway, or KWin window manager name (opt-in) |

Unavailable information is omitted rather than displayed as `unknown`

The default output excludes `cpu-usage`, `shell`, `terminal`, `de`, and `wm`.
To include sampled usage, select it explicitly:

```sh
ferrisfetch --modules os,host,kernel,uptime,cpu,cpu-usage,gpu,monitor,ram,disk
ferrisfetch --pipe --modules cpu-usage --cpu-usage-interval 500
```

The sampling interval is in milliseconds, with a minimum of 200. It has no effect
when `cpu-usage` is not selected.

**Unreleased output change (after 0.3.1):** `cpu` no longer includes the usage percentage.
Use `cpu-usage` for that value; in JSON it is a separate `cpu_usage` key.

### Optional session modules

```sh
ferrisfetch --modules shell,terminal,de,wm
ferrisfetch --json --modules shell,terminal,de,wm
```

These modules report names only, without version queries. JSON uses the same flat
string values as existing modules, with keys `shell`, `terminal`, `de`, and `wm`;
unavailable values are omitted.

- `shell` walks at most 64 processes in the Linux parent chain, skipping wrappers
  and unreadable executable names when the parent PID is still available. The
  nearest recognized shell wins. If ancestry is inaccessible or no known shell
  is found, `$SHELL` supplies a recognized **login shell**, which may differ from
  the current shell. An inaccessible parent PID ends the walk; no full process
  scan is performed.
- `terminal` uses recognized parent executable names, `TERM_PROGRAM`, and
  terminal-specific environment hints. `$TERM` is never an emulator name, and
  generic VTE information alone is insufficient. tmux/screen are reported as
  the terminal endpoint, not an unverifiable outer emulator. SSH suppresses
  emulator hints; a remote multiplexer can still be reported. Unknown endpoints
  are omitted. Inherited environment hints can be stale.
- `de` reads `XDG_CURRENT_DESKTOP`, then `XDG_SESSION_DESKTOP`, normalizes known
  aliases, and deduplicates colon-separated names (for example `ubuntu:GNOME`
  becomes `GNOME`, `Budgie:GNOME` becomes `Budgie / GNOME`). Unknown names and
  standalone WM names are not presented as desktop environments.
- `wm` uses Hyprland/Sway session variables, without scanning processes.
  KWin can also be confirmed using its D-Bus support information, only in a
  graphical KDE session: one `qdbus6` query (`qdbus` for KDE 5), through the bounded
  command helper. Missing tools, D-Bus access, or an unrecognized response omit
  the value; KDE alone does not imply KWin. SSH suppresses WM detection.

Only requested session modules are collected. The parent chain is shared when
needed by `shell`/`terminal`; `de` and `wm` do not read processes. Detection does
not scan all processes or run a sequence of compositor utilities. On platforms without Linux `/proc`,
shell/terminal detection relies on the environment fallbacks.

### Output for scripts

```sh
ferrisfetch --pipe
ferrisfetch --pipe --modules ram | grep '^RAM:'
ferrisfetch --json
ferrisfetch --json --modules kernel | jq .kernel
```

`--pipe` produces plain `Key: value` lines. `--json` produces a JSON object.
Neither output mode includes the logo or ANSI colour codes

## Customization

ferrisfetch is configured through command-line options; it does not currently
read Fastfetch's JSONC configuration files

```sh
# Find a built-in logo by name
ferrisfetch --logo-list | grep nix

# Override the automatically selected logo
ferrisfetch --logo arch

# Hide the logo or disable colours
ferrisfetch --logo-off
ferrisfetch --no-color

# Use the alternative separator (requires a Nerd Font)
ferrisfetch --nerd-separator
```

The default logo is selected from your distribution's `/etc/os-release` ID.
Key colours follow the logo palette. The logo is automatically hidden if the
terminal is too narrow to fit both columns

Alignment uses Unicode terminal column widths, including wide CJK characters,
combining marks, and emoji sequences. Ambiguous-width characters and Nerd Font
icons are treated as one column; actual rendering can vary by terminal and font.

<details>
<summary>Command-line options</summary>

| Option | Description |
|---|---|
| `-m, --modules <MODULES>` | Show only these modules, in the given order |
| `-u, --hide-modules <MODULES>` | Hide these from the default set |
| `--cpu-usage-interval <MS>` | Sampling interval for `cpu-usage` (minimum/default: 200 ms) |
| `--logo-off` | Drop the logo, print a single column |
| `--logo <LOGO>` | Force a logo by name or alias, or `off` |
| `--logo-list` | List all 531 available logos |
| `--no-color` | Disable ANSI colour |
| `--nerd-separator` | Use the Nerd Font separator instead of `:` |
| `--pipe` | Plain `Key: value` output for scripts |
| `--json` | JSON object output |
| `-h, --help` | Show help |
| `-V, --version` | Show the installed version |

</details>

## FAQ

### Is this Fastfetch rewritten in Rust?

No. ferrisfetch is an independent, smaller implementation inspired by Fastfetch,
not a feature-complete port or a drop-in replacement. The ASCII logos are
vendored from Fastfetch and retain their original MIT attribution

### Why is a module missing from the output?

Modules are only shown when their data is available. GPU detection enumerates
Linux PCI display controllers in `/sys/bus/pci/devices`, adds names from
`lspci -D`, and enriches matching devices with names and total memory from
`nvidia-smi`. Devices are matched by normalized PCI address and sorted by
domain, bus, device, and function; identical models at different addresses are
kept, including Intel/AMD graphics alongside NVIDIA. All adapters remain in one
`GPU` entry (for example, `2x Intel GPU + NVIDIA GPU`), and JSON's `gpu` value
remains a string.

Either command can supply devices when sysfs is unavailable, and sysfs provides
vendor-level names if the tools fail or are missing. Unavailable memory is
omitted. Detection runs only when `gpu` is selected. Non-PCI GPUs are not
enumerated; command rows without a valid PCI address cannot be matched and are
ignored. A restricted container or unavailable driver/tool may therefore expose
only partial hardware information. No GPU temperature or utilization is queried.

Monitor detection queries Hyprland or XRandR, so it may be unavailable in other
sessions or on a headless machine

### Does a normal run wait for a CPU usage sample?

No. The `cpu` module reads processor information without waiting. Only the
optional `cpu-usage` module samples usage twice, 200 ms apart by default:

```sh
ferrisfetch --modules cpu,cpu-usage
```

### Why are colours missing when I redirect output?

Colour is disabled when stdout is not a terminal, when `NO_COLOR` is set, or
when `--no-color` is passed. This keeps redirected output free of escape codes

### Can I use a custom image or an external logo file?

Currently, `--logo` selects a built-in ASCII logo by name or alias. External
logo files and image protocols are not supported

### What happens if an external tool hangs?

External detection commands have a two-second timeout and a 1 MiB limit on each
of stdout and stderr. Missing, failing, or over-limit commands are skipped in
favour of the next backend. Commands run with the `C` locale and stdin disconnected
from the terminal. On Unix, cleanup also terminates their process group.

### Where can I report a bug or suggest a feature?

Open an [issue](https://github.com/babichgh/ferrisfetch/issues), including your
ferrisfetch version, distribution, command, and expected versus actual output

## Development

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
python3 tools/generate_logos.py --check
```

One integration test renders all 531 built-in logos. Artwork lives in [`logos/`](logos),
with metadata in `logos/meta.json`; regenerate the committed Rust table with
`python3 tools/generate_logos.py` after changing those inputs

Contributions are welcome. Target `dev` for development and fixes; `main` is
reserved for releases

Local session-module release benchmark against base `d5c2ec2`: 10 warmups,
300 subprocess runs per case in shuffled order, `--pipe`, fixed Hyprland/kitty
environment hints, and stdout discarded. Median wall times (including startup):

| Selection | Median | Added versus `os` in the same build |
|---|---|---|
| `os` (before / after) | 1.536 / 1.530 ms | — |
| `os,shell` | 1.778 ms | 0.249 ms |
| `os,terminal` | 1.787 ms | 0.258 ms |
| `os,de` | 1.539 ms | 0.010 ms |
| `os,wm` | 1.540 ms | 0.011 ms |
| `os,shell,terminal,de,wm` | 1.804 ms | 0.274 ms |

Default `--pipe` output with inherited environment (100 runs) measured
32.790 ms before / 32.719 ms after. These are local observations, not timing
guarantees; environment-only differences are near measurement noise. The KWin
D-Bus backend was not benchmarked here and adds external-command latency (bounded
by the shared two-second timeout).

<details>
<summary>Release process and Trusted Publishing setup</summary>


Develop on `dev`, bump the version in `Cargo.toml` and `Cargo.lock`, then merge
into `main`. The release workflow validates the code, builds RPM/DEB packages,
publishes to crates.io, and creates the matching GitHub release. Reruns skip
crate versions already published to crates.io

Publishing uses crates.io Trusted Publishing, not a stored API token. In the
crate's settings on crates.io, add a GitHub trusted publisher with:

- Owner: `babichgh`
- Repository: `ferrisfetch`
- Workflow filename: `release.yml`
- Environment: leave empty (the workflow does not use a GitHub environment)

Configure this before pushing a new release to `main`

</details>

## Acknowledgements

- [Fastfetch](https://github.com/fastfetch-cli/fastfetch) for the inspiration and
  the vendored ASCII logo collection
- [sysinfo](https://github.com/GuillaumeGomez/sysinfo) for system information
  collection

## License

MIT. Logo artwork is © fastfetch contributors, also MIT, bundled under
`LICENSE-fastfetch`
