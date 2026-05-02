%global debug_package %{nil}

Name:           gpotlight
Version:        0.1.0
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

%files
%{_bindir}/gpotlight
%{_datadir}/applications/io.github.gpotlight.Gpotlight.desktop

%changelog
* Sun May 03 2026 Gpotlight Developers <noreply@example.com> - 0.1.0-1
- Initial RPM package.
