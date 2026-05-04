%global debug_package %{nil}

Name:           gpotlight
Version:        0.1.1
Release:        1%{?dist}
Summary:        Spotlight-style launcher for GNOME

License:        MIT
URL:            https://github.com/gpotlight/gpotlight

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gtk4-devel
BuildRequires:  libadwaita-devel
BuildRequires:  glib2-devel
BuildRequires:  pkgconf-pkg-config
BuildRequires:  dbus-devel
BuildRequires:  openssl-devel

Requires:       gtk4
Requires:       libadwaita
Requires:       glib2

%description
Gpotlight is a Spotlight-style launcher for GNOME Wayland, built with Rust and GTK4.

%prep

%build
cd %{gpotlight_project_dir}
cargo build --release --locked

%install
cd %{gpotlight_project_dir}
install -Dm0755 target/release/gpotlight %{buildroot}%{_bindir}/gpotlight
install -Dm0644 data/io.github.gpotlight.Gpotlight.desktop %{buildroot}%{_datadir}/applications/io.github.gpotlight.Gpotlight.desktop
install -Dm0644 data/icons/hicolor/scalable/apps/io.github.gpotlight.Gpotlight.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.gpotlight.Gpotlight.svg
install -dm0755 %{buildroot}%{_datadir}/gpotlight/plugins
cp -a plugins/. %{buildroot}%{_datadir}/gpotlight/plugins/

%files
%{_bindir}/gpotlight
%{_datadir}/applications/io.github.gpotlight.Gpotlight.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.gpotlight.Gpotlight.svg
%{_datadir}/gpotlight/plugins

%changelog
* Mon May 04 2026 Gpotlight Developers <noreply@example.com> - 0.1.1-1
- Add result buttons and bundled media/project plugins.

* Sun May 03 2026 Gpotlight Developers <noreply@example.com> - 0.1.0-1
- Initial RPM package.
