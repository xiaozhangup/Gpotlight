# Performance and validation

The launcher now displays in-memory search results immediately and runs external
commands in the background. External searches have a 60 ms debounce, one active
batch, and at most four concurrent commands. New input replaces pending work;
results from an older query or a hidden window are discarded. A command is
limited to three seconds and 1 MiB of stdout.

IPC and tray callbacks wake the GTK loop on demand. Visible result rows are
reused, unchanged icons are retained, and a plugin refreshes all its visible rows
with one query. Application names and descriptions are normalized once when the
index is loaded. Usage-ranking keys are calculated once per result.

Bundled plugins also avoid repeated work: MPRIS reads two property dictionaries
per player instead of five individual properties, and project plugins cache
repeated desktop-file/icon lookups within each invocation.

## Measurements

Measured on Fedora 44, Rust 1.99, GTK 4.22.5 and libadwaita 1.9.4, comparing the
original checkout with the optimized release binary. Both processes used an
isolated session bus and config directory, Xvfb/X11 with the cairo renderer,
and the repository's bundled plugins.

| Measurement | Original | Optimized |
| --- | ---: | ---: |
| Idle main-thread voluntary context switches/s | 29.899 | 0.000 |
| Idle CPU, one-core percentage | 0.3–0.4% | 0.0–0.1% |
| Resident memory across both runs | 97.9–119.9 MiB | 98.0–98.7 MiB |
| IPC write to window visible, median of 12 toggles | 27.28–27.86 ms | 4.01–4.62 ms |
| Application matching: 600 queries × 1,000 synthetic apps | 54.15 ms | 8.05 ms |

Two comparisons were run in opposite order, with a 10-second idle sample and
12 toggles per binary in each comparison. Latency and idle wakeup improvements
were reproduced; a resident-memory reduction or faster process startup was not
reproduced. Zero measured CPU means no CPU
ticks were recorded in that sample, not that the entire application never uses
CPU. Memory and startup measurements vary with allocator/cache state. The
matching benchmark measures the matching loop, not total search latency. The
virtual display has no window manager or compositor, so these toggle timings do
not measure actual GNOME/Wayland frame presentation.

## Reproduce

```sh
cargo build --release --locked
cargo test --release --locked
cargo test --release --locked benchmark_precomputed_application_search -- --ignored --nocapture
cargo clippy --release --locked --all-targets -- -D warnings
```

The GUI regression covers continued GTK event processing during a slow query,
latest-query handling, row reuse, grouped refreshes and hide-time cleanup:

```sh
sudo dnf install xorg-x11-server-Xvfb dbus-daemon xdotool
dbus-run-session -- xvfb-run -a env GDK_BACKEND=x11 GSK_RENDERER=cairo GTK_A11Y=none \
  cargo test --release --locked async_search_keeps_ui_responsive_and_discards_stale_results \
  -- --ignored --test-threads=1
```

To compare two release binaries, keep the original binary before rebuilding and
run:

```sh
python3 scripts/benchmark-runtime.py /path/to/original/gpotlight target/release/gpotlight
```

The benchmark isolates app config/runtime directories and the session bus, and
writes logs under `target/`. Use the same repository/plugin files for both runs.
Add `--trace` (requires `strace`) to record poll syscalls. Repeat the comparison
in your real desktop session for representative user-visible performance.

## Browser history (0.1.4–0.1.5)

The history plugin is prefix-only by default, so normal typing does not open any
history database. A matching prefix scopes the search to that plugin. Chromium
queries use the visit-time index and join the bounded candidate set to URLs;
Firefox/Zen use the last-visit index. Locks wait at most 20 ms, a database query
has a 150 ms budget, and discovery plus querying has a 1.5 s budget, backed by the
host's three-second process limit. Slow or unavailable profiles may therefore
be absent from a result set. No persistent history cache is created. Version
0.1.5 adds a fallback for exclusive locks held by running browsers: a private
temporary database + WAL copy, capped at 256 MiB and sharing the same 150 ms
budget. It uses 1 MiB copy chunks, checks for source changes, and retries a changed
copy at most once. Normal readable profiles retain the path without copying.
Cold storage, large databases or continuous writes can exhaust the budget and
cause that profile to be skipped. Temporary files are removed after querying.

A 0.1.4 synthetic fixture with 100,000 records in each of Chrome, Firefox and Zen
(300,000 total), using the default 5,000-record candidate cap, measured median
51.45 ms for recent pages and 73.51 ms for a keyword query over seven fresh
Python processes. This includes interpreter startup and JSON output, with warm
filesystem caches; it excludes the GUI and its 60 ms external-query debounce.
Opening a URI now waits for the browser launcher off the GTK thread.
