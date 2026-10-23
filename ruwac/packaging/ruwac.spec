# Upstream testing RPMs use the version already assigned by the release controller.
%global debug_package %{nil}
%global ruwac_unitdir %{_prefix}/lib/systemd/system
%bcond_without desktop
Name: ruwac
Version: %{ruwac_version}
Release: 1%{?dist}
Summary: Persistent terminal daemon and command-line workspace client
License: %{ruwac_core_license}
URL: https://github.com/w3fans/RUWAC
Source0: ruwac-testing-payload.tar.gz
ExclusiveArch: x86_64
Requires: python3
Requires: openssh-clients
Requires: gh >= 2.97.0
Requires: dnf5
Requires: systemd

%description
Persistent daemon-owned terminals and authenticated command-line control.
This upstream Fedora testing package preserves existing sessions and configuration.
It is separate from the optional GTK desktop and does not autostart services.

%if %{with desktop}
%package desktop
Summary: Native GTK remote workspace and RDP client
License: %{ruwac_desktop_license}
Requires: ruwac = %{version}-%{release}
Requires: gtk4
Requires: webkitgtk6.0
Requires: libadwaita
Requires: openssl-libs
Requires: libssh2
Requires: sqlite-libs
Requires: python3-pillow
Requires: tesseract
Requires: tesseract-langpack-eng
Requires: polkit

Recommends: jetbrains-mono-fonts
Suggests: ibm-plex-mono-fonts fira-code-fonts adobe-source-code-pro-fonts redhat-mono-fonts

%description desktop
Native GTK workspace, authenticated RDP, desktop clipboard and file transfers.
This is an upstream testing candidate, not a Fedora repository qualification.
The known desktop RSA advisory remains unresolved; see the installed testing notes.
%endif

%prep
%setup -q -n ruwac-rpm-payload

%build
# Payloads are compiled and tested from the controller's source by build.sh.
# This packaging adapter neither recompiles nor assigns a product version.

%install
install -D -m 0755 target/release/ruwac %{buildroot}%{_bindir}/ruwac
install -D -m 0644 source.json %{buildroot}%{_datadir}/ruwac/rpm-installation.json
install -D -m 0755 rpm-update.py %{buildroot}%{_datadir}/ruwac/rpm-update.py
install -D -m 0644 update-user.py %{buildroot}%{_datadir}/ruwac/update-user.py
ln -s ../share/ruwac/rpm-update.py %{buildroot}%{_bindir}/ruwac-rpm-update
install -D -m 0644 ruwac-rpm-update.service %{buildroot}%{ruwac_unitdir}/ruwac-rpm-update.service
install -D -m 0644 ruwac-rpm-update.timer %{buildroot}%{ruwac_unitdir}/ruwac-rpm-update.timer
install -D -m 0644 ruwac.1 %{buildroot}%{_mandir}/man1/ruwac.1
%if %{with desktop}
install -D -m 0755 target/release/ruwac-desktop %{buildroot}%{_bindir}/ruwac-desktop
install -D -m 0644 si.z4.ruwac.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/si.z4.ruwac.svg
install -D -m 0644 ruwac.desktop %{buildroot}%{_datadir}/applications/si.z4.ruwac.desktop
%endif

%files
%defattr(-,root,root,0755)
%license LICENSE notices/core
%doc TESTING.txt
%{_bindir}/ruwac
%{_mandir}/man1/ruwac.1*
%dir %{_datadir}/ruwac
%{_datadir}/ruwac/rpm-installation.json
%{_datadir}/ruwac/rpm-update.py
%{_datadir}/ruwac/update-user.py
%{_bindir}/ruwac-rpm-update
%{ruwac_unitdir}/ruwac-rpm-update.service
%{ruwac_unitdir}/ruwac-rpm-update.timer

%if %{with desktop}
%files desktop
%defattr(-,root,root,0755)
%license LICENSE notices/desktop
%doc TESTING.txt
%{_bindir}/ruwac-desktop
%{_datadir}/applications/si.z4.ruwac.desktop
%{_datadir}/icons/hicolor/scalable/apps/si.z4.ruwac.svg
%endif
