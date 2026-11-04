%global debug_package %{nil}

Name:           gpotlight
Version:        0.1.8
Release:        1%{?dist}
Summary:        Spotlight-style launcher for GNOME

License:        MIT
URL:            https://github.com/xiaozhangup/Gpotlight
VCS:            {{{ git_repo_vcs }}}
Source0:        {{{ git_repo_pack }}}

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  gtk4-devel >= 4.22
BuildRequires:  libadwaita-devel >= 1.9
BuildRequires:  glib2-devel
BuildRequires:  pkgconf-pkg-config

Requires:       gtk4 >= 4.22
Requires:       libadwaita >= 1.9
Requires:       glib2
Requires:       python3
Requires:       python3-gobject
Requires:       /usr/bin/setsid

%description
Gpotlight is a Spotlight-style launcher for GNOME Wayland, built with Rust and GTK4.

%prep
{{{ git_repo_setup_macro }}}

%build
cargo build --release --locked

%install
install -Dm0755 target/release/gpotlight %{buildroot}%{_bindir}/gpotlight
install -Dm0644 data/io.github.gpotlight.Gpotlight.desktop %{buildroot}%{_datadir}/applications/io.github.gpotlight.Gpotlight.desktop
install -Dm0644 data/icons/hicolor/scalable/apps/io.github.gpotlight.Gpotlight.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/io.github.gpotlight.Gpotlight.svg
install -dm0755 %{buildroot}%{_datadir}/gpotlight/plugins
cp -a plugins/. %{buildroot}%{_datadir}/gpotlight/plugins/

%files
%license LICENSE
%doc README.md PERFORMANCE.md
%{_bindir}/gpotlight
%{_datadir}/applications/io.github.gpotlight.Gpotlight.desktop
%{_datadir}/icons/hicolor/scalable/apps/io.github.gpotlight.Gpotlight.svg
%{_datadir}/gpotlight/plugins

%changelog
* Wed Nov 04 2026 Gpotlight Developers <noreply@example.com> - 0.1.8-1
- Cancel obsolete plugin queries and reap their processes when input changes or the launcher hides.
- Display each plugin's results as it finishes while preserving ranking and selection.

* Wed Nov 04 2026 Gpotlight Developers <noreply@example.com> - 0.1.7-1
- Refresh launcher styling and request Wayland background blur on supported desktops.

* Wed Nov 04 2026 Gpotlight Developers <noreply@example.com> - 0.1.6-1
- Ship responsive background search and reduced idle resource use.
- Include browser history search with read-only access to locked browser databases.

* Mon Oct 05 2026 Gpotlight Developers <noreply@example.com> - 0.1.5-1
- Read history from running browsers with exclusive database locks.
- Add bounded temporary database/WAL copies while keeping source history read-only.

* Mon Oct 05 2026 Gpotlight Developers <noreply@example.com> - 0.1.4-1
- Add recent browser history search for Chromium-family browsers, Firefox and Zen.
- Use the h prefix by default and support read-only multi-profile history queries.

* Mon Oct 05 2026 Gpotlight Developers <noreply@example.com> - 0.1.3-1
- Keep search responsive with bounded background plugin queries and reusable rows.
- Remove idle IPC/tray polling and reduce application and bundled plugin search work.
- Document build requirements and performance validation.

* Tue May 19 2026 Gpotlight Developers <noreply@example.com> - 0.1.2-1
- Add color preview plugin and require GTK 4.22.

* Mon May 04 2026 Gpotlight Developers <noreply@example.com> - 0.1.1-1
- Add result buttons and bundled media/project plugins.

* Sun May 03 2026 Gpotlight Developers <noreply@example.com> - 0.1.0-1
- Initial RPM package.
