# ferrisfetch

A fastfetch clone written in Rust.

```
             .',;::::;,'.                OS     : Fedora Linux 44 (KDE Plasma Desktop Edition)
         .';:cccccccccccc:;,.            Host   : babich@fedora
      .;cccccccccccccccccccccc;.         Kernel : Linux 7.2.5-200.fc44.x86_64
    .:cccccccccccccccccccccccccc:.       Uptime : 5h 23m
  .;ccccccccccccc;.:dddl:.;ccccccc;.     CPU    : Intel(R) Xeon(R) CPU E5-2680 v3 (12) @ 2.97 GHz : 1%
 .:ccccccccccccc;OWMKOOXMWd;ccccccc:.    GPU    : NVIDIA Corporation TU116
.:ccccccccccccc;KMMc;cc;xMMc;ccccccc:.   Monitor  : DP-1: 1920x1080 (24.0") @ 60.00 Hz
,cccccccccccccc;MMM.;cc;;WW:;cccccccc,   Monitor 2: HDMI-A-1: 1024x768 (14.9") @ 75.03 Hz
:cccccccccccccc;MMM.;cccccccccccccccc:   RAM      : 5.8 GiB / 15.4 GiB (37%)
:ccccccc;oxOOOo;MMM000k.;cccccccccccc:   Disk     : 289.2 GiB / 155.3 GiB (65%) - btrfs
cccccc;MMKxdd:;MMMkddc.;cccccccccccc;
ccccc;XMO';cccc;MMM.;cccccccccccccccc'
ccccc;MMo;ccccc;MMW.;ccccccccccccccc;
ccccc;MNc.ccc.xMMd;ccccccccccccccc;
cccccc;dNMWXXXWM0:;cccccccccccccc:,
```

## Install

From [crates.io](https://crates.io/crates/ferrisfetch):

```sh
cargo install ferrisfetch --locked
```

Or build from a local checkout:

```sh
cargo install --path . --locked
```

## Usage

```
ferrisfetch [OPTIONS]
```

| Option | Description |
|---|---|
| `-m, --modules <MODULES>` | Show only these modules, in the given order |
| `-u, --hide-modules <MODULES>` | Hide these from the default set |
| `--logo-off` | Drop the logo, print a single column |
| `--logo <LOGO>` | Force a logo by name or alias, or `off` |
| `--logo-list` | List all 531 available logos |
| `--no-color` | Disable ANSI colour |
| `--nerd-separator` | Use the Nerd Font separator instead of `:` |
| `--pipe` | Plain `Key: value` output for scripts |
| `--json` | JSON object output |

```sh
ferrisfetch --modules os,ram,cpu  # custom selection and order
ferrisfetch --hide-modules gpu    # everything except the GPU
ferrisfetch --pipe | grep '^RAM:' # scriptable
ferrisfetch --json | jq .kernel   # structured output
ferrisfetch --logo-list | grep nix # find a logo by name
```

## Modules

`os`, `host`, `kernel`, `uptime`, `cpu`, `gpu`, `ram`, `disk`, `monitor`

A module whose data is unavailable is silently omitted rather than printed as
`unknown`, so the output stays clean on bare servers and inside containers.

GPU detection tries `nvidia-smi` first, then `lspci`, then `/sys/class/drm`.
Disk shows used and available GiB, percentage used and filesystem type for the
root volume (or the largest available disk). Each active monitor appears on its
own line between GPU and RAM with its current resolution, diagonal in inches
(when known) and refresh rate;
it uses Hyprland or XRandR and is omitted on headless systems.

## Logos

**531 logos, vendored verbatim from fastfetch** (`src/logo/ascii`, MIT — see
`LICENSE-fastfetch`). The art is stored unmodified under `logos/`, laid out in
the same `a/`, `b/`, `c/` … structure fastfetch uses.

Fastfetch's per-logo metadata lives in C headers, not in the `.txt` files, so
`tools/generate_logos.py` reads those headers and emits `src/logo/generated.rs`,
pairing each logo with its display names and colour palette:

```sh
python3 tools/generate_logos.py          # regenerate the table
python3 tools/generate_logos.py --check  # fail if the table is stale
```

Art uses `$1`, `$2`, … as palette indices. The parsing follows fastfetch's
`ffLogoPrintChars` exactly, because the details matter:

* `$` takes **one** following character, not a number — `$2888` is palette entry
  2 followed by the literal text `888`;
* `$$` is a literal `$`;
* an out-of-range index leaves the `$` as text;
* the active colour carries over to the next line.

The key column takes its colour from the logo's first palette entry, matching
fastfetch's default of `.colorKeys = colors[0]`.

A logo is chosen from the `ID` field of `/etc/os-release` and dropped
automatically when the terminal is too narrow to fit both columns.

## Notes

Colour is disabled automatically when stdout is not a TTY, when `NO_COLOR` is
set, or with `--no-color`, so redirected output never contains escape sequences.

The `cpu` module needs two samples ~200 ms apart to compute load, which accounts
for essentially all of the runtime. Runs that do not request it finish instantly.

## Development

```sh
cargo test                              # unit + integration tests
cargo clippy --all-targets
cargo fmt
```

One of the integration tests renders all 531 logos and checks each one resolves.

### Releases

Develop on `dev`, bump the version in `Cargo.toml` and `Cargo.lock`, then merge
into `main`. The release workflow validates the code, builds RPM/DEB packages,
publishes to crates.io, and creates the matching GitHub release. Reruns skip
crate versions already published to crates.io.

Publishing uses crates.io Trusted Publishing, not a stored API token. In the
crate's settings on crates.io, add a GitHub trusted publisher with:

- Owner: `babichgh`
- Repository: `ferrisfetch`
- Workflow filename: `release.yml`
- Environment: leave empty (the workflow does not use a GitHub environment).

Configure this before pushing a new release to `main`.

## License

MIT. Logo artwork is © fastfetch contributors, also MIT, bundled under
`LICENSE-fastfetch`.
