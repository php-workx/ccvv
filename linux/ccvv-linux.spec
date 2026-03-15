Name: ccvv-linux
Version: @@VERSION@@
Release: 1%{?dist}
Summary: Clipboard sanitizer daemon for Linux desktops
License: MIT
URL: https://github.com/runger/ccvv
Source0: ccvv-linux-%{version}-x86_64-unknown-linux-gnu.tar.gz
BuildArch: x86_64

%description
ccvv-linux runs the ccvv clipboard sanitizer as a user-session daemon on Linux.

%prep
%autosetup -n ccvv-linux-%{version}-x86_64-unknown-linux-gnu

%install
install -Dpm0755 ccvv-linux %{buildroot}%{_bindir}/ccvv-linux
install -Dpm0755 ccvv-tray-sni %{buildroot}%{_bindir}/ccvv-tray-sni
install -Dpm0644 ccvv-linux.desktop %{buildroot}%{_sysconfdir}/xdg/autostart/ccvv.desktop
install -Dpm0644 ccvv-linux.service %{buildroot}%{_userunitdir}/ccvv.service

%files
%{_bindir}/ccvv-linux
%{_bindir}/ccvv-tray-sni
%config(noreplace) %{_sysconfdir}/xdg/autostart/ccvv.desktop
%{_userunitdir}/ccvv.service
