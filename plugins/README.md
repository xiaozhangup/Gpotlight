Place external plugin manifest `.toml` files in this directory.

During development, Gpotlight loads manifests from this project-level directory.
RPM packages install this directory to `/usr/share/gpotlight/plugins`, which is
also searched at runtime. User-installed manifests can still be placed in
`$XDG_CONFIG_HOME/gpotlight/plugins`.
