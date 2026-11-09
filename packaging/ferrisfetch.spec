Name:           ferrisfetch
Version:        %{ferrisfetch_version}
Release:        1%{?dist}
Summary:        A fastfetch clone written in Rust

License:        MIT
URL:            https://github.com/babichgh/ferrisfetch
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo >= 1.85
BuildRequires:  rust >= 1.85

# The binary is self-contained and links only libc, so nothing has to be
# resolved per-distribution. Keep the artifact set predictable.
ExclusiveArch:  x86_64

%description
ferrisfetch prints system information next to a logo for the running
distribution. Logo artwork is vendored from fastfetch (MIT).

%prep
%autosetup -p1 -n %{name}-%{version}

%build
# Invoked directly rather than through %cargo_build: that macro shells out to
# `fg` to re-apply RUSTFLAGS, which fails in a non-interactive build shell.
#
# --locked pins every dependency to Cargo.lock; that, not --offline, is what
# makes the build reproducible. --offline is not usable from a clean buildroot
# because the registry cache does not exist yet.
cargo fetch --locked
cargo build --release --locked

%install
install -Dm 0755 target/release/ferrisfetch %{buildroot}%{_bindir}/ferrisfetch

# %license and %doc in %files install these from the build tree; manually
# installing them here creates duplicate, potentially unpackaged files.

%check
# Unit tests only. The integration suite renders all 531 logos, which is worth
# doing in CI but adds minutes to every package build for no extra signal that
# the binary is sound.
cargo test --release --locked --bins

%files
%license LICENSE
%license LICENSE-fastfetch
%doc README.md
%{_bindir}/ferrisfetch

%changelog
* Tue Oct 13 2026 babichgh <john@babich.pw> - %{ferrisfetch_version}-1
- Initial packaged release
