# AGENTS.md

Guidance for coding agents working on this repository.

## What this is

`ferrisfetch` is a fastfetch clone in Rust. It prints system information in a column
next to an ASCII logo for the running distribution. It is a binary crate with no
library target, so everything lives in `src/`.

## Commands

```sh
cargo test                              # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt
cargo build --release

python3 tools/generate_logos.py --check # is the generated logo table current?
```

The release profile has `panic = "abort"`, so `cargo test` uses the `test`
profile (inherited from dev) and is unaffected.

## Branch and release model

- All development and fixes go to `dev` until a release is ready. Do not push
  unreleased work directly to `main`.
- `main` is the release branch: every merge or push to it must create a **new**
  release. Before merging, bump `version` in `Cargo.toml` on `dev` to a version
  that has not been released yet, and update `Cargo.lock` accordingly.
- Pushing to `main` runs CI and the release workflow, which publishes a GitHub
  release tagged `v<version>` from `Cargo.toml`. Without a version bump the
  workflow overwrites the existing release assets but does not move its Git tag;
  never merge to `main` with an already-released version.

To cut a release: finish and test changes on `dev`, bump the version there,
merge `dev` into `main`, then push `main` to publish the new release.

## Architecture

```
src/
├── main.rs      orchestration; --pipe and --json bypass the layout
├── cli.rs       clap args
├── fetch/       data collection, one module per fact
├── logo/        531 vendored artworks + marker parsing
└── render/      two-column layout, colours, ANSI-aware width
```

`fetch::Context::collect(&modules)` refreshes only what was requested. Reading
`gpu` shells out to `nvidia-smi`; `monitor` queries Hyprland or XRandR only
when selected; reading `cpu-usage` sleeps 200 ms by default waiting for a load
delta. `cpu` only reads model/core/frequency information and does not wait. Do not
make collection unconditional.

Modules return `Option`. Data that is unavailable makes the line disappear
rather than print `unknown`.

## Adding or changing modules

- Add the detector under `src/fetch/`, register its `Module` variant and label,
  and connect it to selective collection and output. Decide explicitly whether
  it belongs in `DEFAULT_ORDER`; expensive or privacy-sensitive modules should
  be opt-in. `cpu-usage`, unlike `cpu`, is not in the default set.
- Refresh only the resources the selected modules need. Keep sampling waits
  exclusive to modules that require a delta; respect sysinfo's
  `MINIMUM_CPU_UPDATE_INTERVAL`.
- Keep parsing/formatting separate from filesystem reads and process execution.
  Test parsers with fixtures for missing, malformed, and multiple-device data;
  do not rely only on the developer's hardware or session.
- Add CLI tests for selection, ordering, and relevant `--pipe`/`--json` output,
  and update the module table and options in `README.md`.
- Treat CLI names, JSON keys/types, and default output as user-facing contracts.
  Document intentional changes; do not silently replace the JSON schema.
- Measure performance using release builds before and after changes to
  collection. Avoid tight wall-clock assertions in tests: CI load varies.

## External commands and privacy

- New command-based detectors must use bounded execution (timeout and output
  size), check exit status, and control locale when parsing localized text.
  Use the shared `fetch::command::output` helper rather than repeating
  process-management logic.
- Pass arguments directly; do not use `sh -c` for ordinary detectors or execute
  commands obtained from configuration implicitly. Do not require root.
- No network requests by default. Serial numbers, machine UUIDs, MAC addresses,
  and similar identifiers must not be added to normal output implicitly.
- Linux is the primary target. Missing tools, unsupported sessions, and denied
  access should omit unavailable data rather than panic or invent values.

## Things that will bite you

**sysinfo 0.39 API.** `System::uptime()`, `System::host_name()`,
`System::kernel_version()`, `System::name()` and `System::physical_core_count()`
are *associated functions*, not methods. `Disks` is a separate type from
`System`; use `Disks::new_with_refreshed_list()`, which loads both the list and
the counters. Check the vendored source rather than guessing:
`~/.cargo/registry/src/*/sysinfo-0.39.6/`.

**`System::name()` is not the kernel name on Linux.** sysinfo resolves it from
`os-release`, so it returns the distribution name ("Fedora Linux"). `fetch/kernel.rs`
uses a `cfg(target_os)` literal instead.

**ANSI escapes break width maths.** `render::width::visible_len` strips CSI and
OSC sequences and measures terminal columns with `unicode-width`. Use it for
any padding or column alignment; never `.len()` or `.chars().count()`. Measure
whole visible strings, not individual characters: combining marks and emoji
sequences can span code points and ANSI changes. Ambiguous-width characters
use non-CJK widths, including single-column Nerd Font icons.

**Logo colour markers are not numbers.** In the vendored art, `$` takes exactly
one following character, per fastfetch's `ffLogoPrintChars`. `$2888` is palette
entry 2 followed by the literal text `888`; `$$` is a literal dollar; the active
colour carries over to the next line. The obvious "read all the digits"
implementation breaks 37 logos. Do not "simplify" `logo::render_art`.

**Do not hand-draw logos.** They were tried and read as random glyphs. All 531
come from fastfetch under `logos/`, copied verbatim.

## Regenerating the logo table

`logos/*.txt` is art; `logos/meta.json` is names and palettes extracted from
fastfetch's C headers. Both feed `src/logo/generated.rs`, which is committed.

```sh
python3 tools/generate_logos.py               # regenerate generated.rs
python3 tools/generate_logos.py --check       # fail if stale
python3 tools/generate_logos.py --sync PATH   # refresh meta.json from a
                                               # fastfetch checkout
```

`--sync` is the only mode that needs a fastfetch clone; plain runs and `--check`
work from `meta.json`, which is why CI can verify the table.

The generator runs its output through `rustfmt`. Without that it fights
`cargo fmt` and `--check` reports the file as stale immediately.

Art is marked `-text -diff` in `.gitattributes`. Do not let an editor or EOL
conversion rewrite it.

## Packaging

`packaging/ferrisfetch.spec` builds the rpm; `packaging/build-deb.sh` builds the deb
from `packaging/debian/control.in`.

- `%cargo_build` is deliberately not used: it shells out to `fg` and fails in a
  non-interactive build shell.
- `--locked`, never `--offline`. A clean buildroot has no registry cache, so
  `--offline` cannot work.
- The deb is compiled inside `rust:1-bookworm`, not on the Ubuntu runner, so the
  binary does not need a newer glibc than the machines it targets.
- Only the binary rpm is published; Fedora auto-generates debuginfo and
  debugsource packages that roughly double the download.

## Testing

`tests/cli.rs` runs the compiled binary. One test renders **all 531 logos**, so
a change that breaks any single artwork is caught. That test is also the slow
one; the rpm spec's `%check` runs `--bins` only to skip it.

All external detector commands go through `fetch::command::output`: two-second
timeout, 1 MiB limit per output stream, `C` locale, null stdin, and successful
exit status required. On Unix, commands run in a private process group which is
killed during cleanup; the direct child is always reaped. Do not bypass this
helper with direct `Command::output` calls. `tests/commands.rs` uses fake tools
to verify fallback behaviour without depending on hardware or the session.

## Known issues

- `main` currently has no branch protection; `dev` merges are not enforced.
