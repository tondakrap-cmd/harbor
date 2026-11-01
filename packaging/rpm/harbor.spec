Name: harbor
Version: %{harbor_version}
Release: %{harbor_release}%{?dist}
Summary: Desktop MariaDB manager
License: MIT AND OFL-1.1
BuildArch: x86_64
Requires: libX11, libXcursor, libXrandr, libXi, libxkbcommon, libxkbcommon-x11, libwayland-client, libglvnd-glx, libglvnd-egl

%description
Harbor desktop MariaDB manager, four skins and a shared Rust core.

%package -n harbor-tui
Summary: Standalone terminal MariaDB manager
License: MIT

%description -n harbor-tui
Harbor terminal client for local terminals and SSH, without GUI dependencies.

%prep
%build

%install
mkdir -p %{buildroot}/usr/bin
install -m 755 %{harbor_repo}/target/release/harbor %{buildroot}/usr/bin/harbor
install -m 755 %{harbor_repo}/target/release/harbor-tui %{buildroot}/usr/bin/harbor-tui
mkdir -p %{buildroot}/usr/share/applications %{buildroot}/usr/share/icons/hicolor/scalable/apps
install -m 644 %{harbor_repo}/packaging/harbor.desktop %{buildroot}/usr/share/applications/harbor.desktop
install -m 644 %{harbor_repo}/packaging/harbor.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/harbor.svg
mkdir -p %{buildroot}/usr/share/licenses/harbor %{buildroot}/usr/share/licenses/harbor-tui
install -m 644 %{harbor_repo}/LICENSE %{buildroot}/usr/share/licenses/harbor/LICENSE
install -m 644 %{harbor_repo}/LICENSE %{buildroot}/usr/share/licenses/harbor-tui/LICENSE
install -m 644 %{harbor_repo}/crates/harbor-desktop/assets/FONT-LICENSE.txt %{buildroot}/usr/share/licenses/harbor/FONT-LICENSE.txt
cp -r %{harbor_repo}/target/licenses/harbor %{buildroot}/usr/share/licenses/harbor/third-party
cp -r %{harbor_repo}/target/licenses/harbor-tui %{buildroot}/usr/share/licenses/harbor-tui/third-party
mkdir -p %{buildroot}/usr/share/doc/harbor %{buildroot}/usr/share/doc/harbor-tui
install -m 644 %{harbor_repo}/README.md %{buildroot}/usr/share/doc/harbor/README.md
install -m 644 %{harbor_repo}/README.md %{buildroot}/usr/share/doc/harbor-tui/README.md

%files
/usr/bin/harbor
/usr/share/applications/harbor.desktop
/usr/share/icons/hicolor/scalable/apps/harbor.svg
%license /usr/share/licenses/harbor
%doc /usr/share/doc/harbor

%files -n harbor-tui
/usr/bin/harbor-tui
%license /usr/share/licenses/harbor-tui
%doc /usr/share/doc/harbor-tui

