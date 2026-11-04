Place external plugin manifest `.toml` files in this directory.

During development, Gpotlight loads manifests from this project-level directory.
RPM packages install this directory to `/usr/share/gpotlight/plugins`, which is
also searched at runtime. User-installed manifests can still be placed in
`$XDG_CONFIG_HOME/gpotlight/plugins`.

## Query resource limits

External queries run off the GTK main thread, with at most four commands running
at once. Each command has a three-second deadline and a 1 MiB stdout limit.
Commands exceeding either limit are stopped along with their process group, and
their results are discarded. Query commands receive closed stdin; return JSON
lines on stdout. Stderr is discarded.

Rapid input is debounced for 60 ms and pending queries are replaced by the newest
input. Built-in results appear immediately. Refresh requests for visible rows
from the same plugin are combined into one query using the shortest requested
interval (at least 100 ms). Hiding the launcher removes pending refresh timers.

## Browser History

The bundled `browser-history.toml` / `browser_history.py` plugin uses only the
Python standard library. Gpotlight defaults it to the `h` prefix with global
search disabled, including when an existing config has no entry for this new
plugin. `h` lists recent pages; `h keyword` searches page titles and URLs. Word
prefixes require a separating space (or an empty suffix), so `hello` is not a
history query. Symbol prefixes such as `/` still support `/keyword`.

Results include browser/profile information and visit time. Activating a result
opens its HTTP(S) URL in the system's default browser; its action button copies
the link. Chromium-family and Firefox-family timestamps are converted to the
same time scale before results from different profiles are merged.

Settings include `history_days` (30), `max_results` (20), and
`max_history_entries` (5,000 recent records per profile), plus individual switches
for Chrome, Chromium, Edge, Brave, Vivaldi, Opera, Firefox, and Zen. The candidate
limit bounds recent visits for Chromium and recent page rows for Firefox/Zen;
matches beyond that limit are not searched. Increase it to cover a larger
history at the cost of more query work.

SQLite connections use `mode=ro` and include committed records in live WAL files.
Read-only mode still respects the exclusive locks used by running browsers. If
direct access is blocked, the plugin copies the database and WAL into a private
temporary directory and queries that copy read-only. SQLite rebuilds shared
memory there; the browser's `-shm` is never copied. The copy is removed after the
query, and source files are never opened for writing by the fallback.

Copying checks source identity, size and nanosecond modification/change times
before and after, retrying detected changes at most once. This is a best-effort
quiet-interval copy, not an atomic backup. Active rollback journals are skipped
to avoid exposing uncommitted pages. Copies are limited to 256 MiB for database
plus WAL and share the 150 ms per-profile budget (1.5 seconds for the whole
search). Oversized, continuously changing, or otherwise unavailable profiles
can be absent from results. Normal read-only queries do not copy the database.
Discovery stays within known browser profile roots and paths listed in profile
metadata. No history is uploaded or persistently cached by Gpotlight.

Fixture tests create temporary databases; they do not need a browser or the
user's real history:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -v
```

Known locations include each Chromium browser's `$XDG_CONFIG_HOME` directory,
its `Default`/`Profile *` directories and `Local State` profile list; Firefox's
`~/.mozilla/firefox` and `profiles.ini`; and Zen's `~/.zen` or
`$XDG_CONFIG_HOME/zen`. Firefox/Zen `profiles.ini` may point at custom absolute
profile directories. Corresponding Flatpak roots under `~/.var/app` are checked,
including both Zen `.zen`/`zen` layouts and the legacy Flatpak ID. Standard
Firefox and Chromium Snap directories are supported as well. Arbitrary
Chromium `--user-data-dir` locations outside these roots are not auto-discovered.

`h` scopes the search to matching prefix plugins, so unrelated global plugins
are not started for a history query. History uses the manifest's
`preserve_order = true` option to retain visit-time order even when general
usage-based ranking is enabled. Other plugins can opt into this ordering policy.
