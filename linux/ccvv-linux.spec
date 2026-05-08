Name: ccvv-linux
Version: @@VERSION@@
Release: 1%{?dist}
Summary: Clipboard sanitizer daemon for Linux desktops
License: MIT
URL: https://github.com/php-workx/ccvv
Source0: ccvv-linux-%{version}-x86_64-unknown-linux-gnu.tar.gz
BuildArch: x86_64
Requires: glibc
Requires: libgcc
Requires: libX11
Requires: libXfixes
Requires: wayland
Requires: dbus

%description
ccvv-linux runs the ccvv clipboard sanitizer as a user-session daemon on Linux.

%prep
%autosetup -n ccvv-linux-%{version}-x86_64-unknown-linux-gnu

%install
install -Dpm0755 ccvv-linux %{buildroot}%{_bindir}/ccvv-linux
install -Dpm0755 ccvv-tray-sni %{buildroot}%{_bindir}/ccvv-tray-sni
install -Dpm0755 ccvv %{buildroot}%{_bindir}/ccvv
install -Dpm0644 ccvv-linux.desktop %{buildroot}%{_sysconfdir}/xdg/autostart/ccvv.desktop
install -Dpm0644 ccvv-linux.service %{buildroot}%{_userunitdir}/ccvv.service
install -d %{buildroot}%{_datadir}/icons/hicolor/scalable/apps
install -pm0644 icons/*.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/

%files
%{_bindir}/ccvv-linux
%{_bindir}/ccvv-tray-sni
%{_bindir}/ccvv
%config(noreplace) %{_sysconfdir}/xdg/autostart/ccvv.desktop
%{_userunitdir}/ccvv.service
%{_datadir}/icons/hicolor/scalable/apps/ccvv-*.svg
