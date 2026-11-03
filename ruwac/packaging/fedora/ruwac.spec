# Fedora/COPR package built from source. Crates are vendored (Source1); the
# upstream GitHub testing RPMs (packaging/ruwac.spec) are a separate channel.
# Updates come only from DNF: this package ships no self-updater or timer.
%{!?ruwac_version:%global ruwac_version 0.4.53}
%bcond check 1
# Desktop unit tests build an RDP test server whose TLS stack compiles
# aws-lc from source; keep them opt-in for distribution builds.
%bcond desktop_check 0

Name:           ruwac
Version:        %{ruwac_version}
Release:        1%{?dist}
Summary:        Persistent terminal sessions with a native remote-desktop workspace

# ruwac: Apache-2.0. Statically linked crates (see LICENSE.dependencies):
SourceLicense:  Apache-2.0
License:        Apache-2.0 AND MIT AND Unicode-3.0 AND (Apache-2.0 OR MIT) AND (Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT) AND (MIT OR Zlib OR Apache-2.0) AND (0BSD OR MIT OR Apache-2.0) AND (BSD-2-Clause OR Apache-2.0) AND (Unlicense OR MIT)
URL:            https://github.com/w3fans/RUWAC
Source0:        %{url}/archive/v%{version}/RUWAC-%{version}.tar.gz
# Created by packaging/fedora/make-sources.sh with `cargo vendor --locked`.
Source1:        ruwac-%{version}-vendor.tar.xz

ExclusiveArch:  %{rust_arches}

BuildRequires:  cargo-rpm-macros >= 26
BuildRequires:  gcc
BuildRequires:  pkgconfig(gtk4)
BuildRequires:  pkgconfig(libadwaita-1)
BuildRequires:  pkgconfig(webkitgtk-6.0)
BuildRequires:  pkgconfig(openssl)
BuildRequires:  pkgconfig(libssh2)
BuildRequires:  pkgconfig(sqlite3)
BuildRequires:  pkgconfig(zlib)
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib
%if %{with desktop_check}
BuildRequires:  cmake
BuildRequires:  clang
%endif

Requires:       openssh-clients
Requires:       python3

# Patched upstream crate carried in the source tree (vendor/alacritty_terminal).
Provides:       bundled(crate(alacritty_terminal)) = 0.26.0

%description
RUWAC keeps terminal sessions running in a per-user daemon so they survive
disconnects, and attaches to them from a command-line client that works like
GNU Screen or tmux, locally and over SSH.

%package desktop
Summary:        Native GTK workspace for RUWAC terminals, RDP and SSH machines
License:        Apache-2.0 AND MIT AND Unicode-3.0 AND BSD-3-Clause AND ISC AND (Apache-2.0 OR MIT) AND (Apache-2.0 WITH LLVM-exception) AND (Apache-2.0 OR BSL-1.0) AND (BSD-3-Clause OR Apache-2.0) AND (BSD-2-Clause OR Apache-2.0 OR MIT) AND (MIT OR Zlib OR Apache-2.0) AND (0BSD OR MIT OR Apache-2.0) AND (Unlicense OR MIT)
Requires:       %{name}%{?_isa} = %{version}-%{release}
Requires:       hicolor-icon-theme
Requires:       python3-pillow
Requires:       tesseract
Requires:       tesseract-langpack-eng
Recommends:     jetbrains-mono-fonts
# Patched IronRDP crates carried in the source tree (desktop/vendor).
Provides:       bundled(crate(ironrdp-connector)) = 0.10.0
Provides:       bundled(crate(ironrdp-egfx)) = 0.3.0
Provides:       bundled(crate(ironrdp-graphics)) = 0.9.0
Provides:       bundled(crate(ironrdp-pdu)) = 0.9.0
Provides:       bundled(crate(ironrdp-session)) = 0.11.0

%description desktop
GTK 4 and libadwaita workspace for RUWAC: terminal tabs and splits attached to
the RUWAC daemon, an embedded RDP client (including GNOME Remote Login), saved
SSH and RDP machines, clipboard and file transfer.

%prep
%autosetup -n RUWAC-%{version} -p1
# The tarball unpacks to cargo-vendor/; ruwac/vendor holds the patched crate.
tar -xJf %{SOURCE1} -C ruwac
cd ruwac
%cargo_prep -v cargo-vendor

%build
cd ruwac
# Link the system libssh2 rather than its bundled copy.
export LIBSSH2_SYS_USE_PKG_CONFIG=1
%cargo_build
%{cargo_license_summary}
%{cargo_license} > LICENSE.dependencies
%cargo_vendor_manifest
cd desktop
%cargo_build
%{cargo_license_summary}
%{cargo_license} > ../LICENSE.desktop-dependencies

%install
cd ruwac
install -Dpm 0755 target/rpm/ruwac %{buildroot}%{_bindir}/ruwac
install -Dpm 0755 desktop/target/rpm/ruwac-desktop %{buildroot}%{_bindir}/ruwac-desktop
install -Dpm 0644 packaging/ruwac.1 %{buildroot}%{_mandir}/man1/ruwac.1
install -Dpm 0644 packaging/si.z4.ruwac.svg \
    %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/si.z4.ruwac.svg
# The distribution owns updates: drop the upstream "Install verified update" action.
sed -e 's/^Actions=NewWindow;Update;$/Actions=NewWindow;/' \
    -e '/^\[Desktop Action Update\]$/,$d' packaging/ruwac.desktop > si.z4.ruwac.desktop
desktop-file-install --dir=%{buildroot}%{_datadir}/applications si.z4.ruwac.desktop
install -Dpm 0644 packaging/fedora/si.z4.ruwac.metainfo.xml \
    %{buildroot}%{_metainfodir}/si.z4.ruwac.metainfo.xml
# Tells ruwac and ruwac-desktop that DNF manages updates (no GitHub updater).
install -d %{buildroot}%{_datadir}/ruwac
printf '{"kind":"distribution","manager":"dnf","version":"%s"}\n' '%{version}' \
    > %{buildroot}%{_datadir}/ruwac/rpm-installation.json

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/si.z4.ruwac.desktop
appstream-util validate-relax --nonet %{buildroot}%{_metainfodir}/si.z4.ruwac.metainfo.xml
%if %{with check}
cd ruwac
export LIBSSH2_SYS_USE_PKG_CONFIG=1
%cargo_test
%if %{with desktop_check}
cd desktop
%cargo_test
%endif
%endif

%posttrans
# Replacing an upstream GitHub testing RPM removes its ruwac-rpm-update timer
# but not an enable link pointing at it; drop only that dangling link.
link=%{_sysconfdir}/systemd/system/timers.target.wants/ruwac-rpm-update.timer
if [ -L "$link" ] && [ ! -e "$link" ]; then
    rm -f "$link"
    systemctl daemon-reload >/dev/null 2>&1 || :
fi

%files
%license LICENSE ruwac/LICENSE.dependencies ruwac/cargo-vendor.txt
%doc README.md CHANGELOG.md SECURITY.md
%{_bindir}/ruwac
%{_mandir}/man1/ruwac.1*
%dir %{_datadir}/ruwac
%{_datadir}/ruwac/rpm-installation.json

%files desktop
%license LICENSE ruwac/LICENSE.desktop-dependencies
%{_bindir}/ruwac-desktop
%{_datadir}/applications/si.z4.ruwac.desktop
%{_datadir}/icons/hicolor/scalable/apps/si.z4.ruwac.svg
%{_metainfodir}/si.z4.ruwac.metainfo.xml

%changelog
* Tue Nov 03 2026 RUWAC maintainers - 0.4.53-1
- Initial from-source Fedora/COPR package
