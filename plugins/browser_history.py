#!/usr/bin/env python3
"""Search recent local history, with a bounded snapshot fallback for busy browsers."""

import configparser
import json
import os
import sqlite3
import stat
import sys
import tempfile
import time
from dataclasses import dataclass
from datetime import datetime
from itertools import islice
from pathlib import Path
from urllib.parse import unquote, urlsplit


CHROMIUM_EPOCH_US = 11_644_473_600_000_000
SEARCH_TIMEOUT = 1.5
DATABASE_TIMEOUT = 0.15
MAX_DATABASES = 128
MAX_DIRECTORY_ENTRIES = 256
MAX_METADATA_BYTES = 1024 * 1024
SNAPSHOT_MAX_BYTES = 256 * 1024 * 1024
SNAPSHOT_COPY_CHUNK = 1024 * 1024


@dataclass(frozen=True)
class Browser:
    key: str
    name: str
    family: str
    icon: str
    config_dirs: tuple[str, ...]
    flatpak_ids: tuple[str, ...]


BROWSERS = (
    Browser("chrome", "Chrome", "chromium", "google-chrome",
            ("google-chrome", "google-chrome-beta", "google-chrome-unstable"),
            ("com.google.Chrome",)),
    Browser("chromium", "Chromium", "chromium", "chromium",
            ("chromium",), ("org.chromium.Chromium",)),
    Browser("edge", "Microsoft Edge", "chromium", "microsoft-edge",
            ("microsoft-edge", "microsoft-edge-beta", "microsoft-edge-dev"),
            ("com.microsoft.Edge",)),
    Browser("brave", "Brave", "chromium", "brave-browser",
            ("BraveSoftware/Brave-Browser", "BraveSoftware/Brave-Browser-Beta",
             "BraveSoftware/Brave-Browser-Nightly"), ("com.brave.Browser",)),
    Browser("vivaldi", "Vivaldi", "chromium", "vivaldi",
            ("vivaldi", "vivaldi-snapshot"), ("com.vivaldi.Vivaldi",)),
    Browser("opera", "Opera", "chromium", "opera",
            ("opera", "opera-beta", "opera-developer"), ("com.opera.Opera",)),
    Browser("firefox", "Firefox", "firefox", "firefox",
            ("mozilla/firefox", "firefox"), ("org.mozilla.firefox",)),
    Browser("zen", "Zen", "firefox", "zen-browser",
            ("zen", "zen-twilight"),
            ("app.zen_browser.zen", "io.github.zen_browser.zen")),
)


@dataclass(frozen=True)
class Profile:
    browser: Browser
    name: str
    database: Path


@dataclass(frozen=True)
class HistoryEntry:
    title: str
    url: str
    visited_us: int
    profile: Profile


def load_config() -> dict:
    try:
        value = json.loads(os.environ.get("GPOTLIGHT_PLUGIN_CONFIG", "{}"))
        return value if isinstance(value, dict) else {}
    except (ValueError, TypeError):
        return {}


def integer_setting(config: dict, key: str, default: int, minimum: int, maximum: int) -> int:
    value = config.get(key, default)
    if isinstance(value, bool) or not isinstance(value, int):
        return default
    return max(minimum, min(maximum, value))


def read_metadata(path: Path) -> str:
    with path.open(encoding="utf-8") as source:
        text = source.read(MAX_METADATA_BYTES + 1)
    if len(text) > MAX_METADATA_BYTES:
        raise ValueError("browser profile metadata is too large")
    return text


def child_directories(root: Path, deadline: float):
    try:
        with os.scandir(root) as children:
            for child in islice(children, MAX_DIRECTORY_ENTRIES):
                if time.monotonic() >= deadline:
                    break
                if child.is_dir():
                    yield Path(child.path)
    except OSError:
        return


def browser_roots(browser: Browser, home: Path, config_home: Path):
    yield from (config_home / name for name in browser.config_dirs)
    if browser.key == "firefox":
        yield home / ".mozilla/firefox"
        yield home / "snap/firefox/common/.mozilla/firefox"
    elif browser.key == "zen":
        yield home / ".zen"
        yield home / ".zen-twilight"
    elif browser.key == "chromium":
        yield home / "snap/chromium/common/chromium"
    for app_id in browser.flatpak_ids:
        app_root = home / ".var/app" / app_id
        yield from (app_root / "config" / name for name in browser.config_dirs)
        if browser.key == "firefox":
            yield app_root / ".mozilla/firefox"
        elif browser.key == "zen":
            # Both spellings occur in official Zen documentation/packages.
            yield app_root / ".zen"
            yield app_root / "zen"


def chromium_profiles(root: Path, deadline: float):
    yield root, root.name  # Opera and explicitly supplied user-data roots.
    try:
        data = json.loads(read_metadata(root / "Local State"))
        info = data.get("profile", {}).get("info_cache", {})
        if isinstance(info, dict):
            for directory, metadata in islice(info.items(), MAX_DIRECTORY_ENTRIES):
                # Chromium profile directories are immediate children of the root.
                if not isinstance(directory, str) or Path(directory).name != directory:
                    continue
                if directory in (".", "..", ""):
                    continue
                name = metadata.get("name", directory) if isinstance(metadata, dict) else directory
                yield root / directory, name if isinstance(name, str) else directory
    except (OSError, ValueError, AttributeError, UnicodeError):
        pass
    yield root / "Default", "Default"
    for directory in child_directories(root, deadline):
        if directory.name.startswith("Profile "):
            yield directory, directory.name


def firefox_profiles(root: Path, deadline: float):
    parser = configparser.ConfigParser(interpolation=None, strict=False)
    try:
        parser.read_string(read_metadata(root / "profiles.ini"))
        for section in islice(parser.sections(), MAX_DIRECTORY_ENTRIES):
            if not section.startswith("Profile"):
                continue
            location = parser.get(section, "Path", fallback="")
            if not location:
                continue
            path = Path(location)
            if parser.get(section, "IsRelative", fallback="1") == "1":
                path = root / path
            elif not path.is_absolute():
                continue
            yield path, parser.get(section, "Name", fallback=path.name)
    except (OSError, ValueError, configparser.Error, UnicodeError):
        pass
    yield root, root.name
    # One level only: no traversal of unrelated home directories.
    for directory in child_directories(root, deadline):
        yield directory, directory.name
    for directory in child_directories(root / "Profiles", deadline):
        yield directory, directory.name


def discover_profiles(config: dict, home: Path, config_home: Path, deadline: float):
    if time.monotonic() >= deadline:
        return
    seen_roots = set()
    seen_databases = set()
    for browser in BROWSERS:
        if config.get(browser.key + "_enabled", True) is False:
            continue
        for root in browser_roots(browser, home, config_home):
            if time.monotonic() >= deadline or len(seen_databases) >= MAX_DATABASES:
                return
            if root in seen_roots or not root.is_dir():
                continue
            seen_roots.add(root)
            profiles = (chromium_profiles(root, deadline) if browser.family == "chromium"
                        else firefox_profiles(root, deadline))
            filename = "History" if browser.family == "chromium" else "places.sqlite"
            for directory, name in profiles:
                if time.monotonic() >= deadline or len(seen_databases) >= MAX_DATABASES:
                    return
                try:
                    database = (directory / filename).resolve()
                    if database in seen_databases or not database.is_file():
                        continue
                except (OSError, RuntimeError):
                    continue
                seen_databases.add(database)
                yield Profile(browser, name, database)


# Chromium indexes visits.visit_time, but does not index urls.last_visit_time.
# LIMIT inside this subquery ensures filtering and joining cannot scan the full DB.
CHROMIUM_SQL = """
    SELECT u.url, u.title, recent.visit_time
    FROM (
        SELECT url, visit_time FROM visits
        WHERE visit_time >= ? AND visit_time <= ?
        ORDER BY visit_time DESC LIMIT ?
    ) AS recent JOIN urls AS u ON u.id = recent.url
    WHERE u.hidden = 0
    ORDER BY recent.visit_time DESC
"""
FIREFOX_SQL = """
    SELECT url, title, last_visit_date FROM (
        SELECT url, title, last_visit_date, hidden FROM moz_places
        WHERE last_visit_date >= ? AND last_visit_date <= ?
        ORDER BY last_visit_date DESC LIMIT ?
    ) WHERE hidden = 0
    ORDER BY last_visit_date DESC
"""


def safe_url(url) -> bool:
    if not isinstance(url, str) or len(url) > 8192:
        return False
    if any(ord(char) <= 32 or ord(char) == 127 for char in url):
        return False
    try:
        parsed = urlsplit(url)
        return parsed.scheme in ("https", "http") and bool(parsed.hostname)
    except ValueError:
        return False


class SnapshotChanged(OSError):
    """The browser changed its database while we were copying it."""


def file_stamp(path: Path):
    try:
        value = path.stat()
    except FileNotFoundError:
        return None
    if not stat.S_ISREG(value.st_mode):
        raise OSError("history file is not a regular file")
    return (value.st_dev, value.st_ino, value.st_size, value.st_mtime_ns, value.st_ctime_ns)


def copy_snapshot(database: Path, directory: Path, deadline: float) -> Path:
    """Copy only during an observed quiet interval; never open the source for writing.

    This is a best-effort snapshot, not an SQLite backup: exclusive browser locks
    prevent the backup API from acquiring its source read lock too. Source file
    identity, size and nanosecond timestamps must stay unchanged across the copy.
    SQLite then recovers committed WAL frames in the private copy, rebuilding its
    own shm. Never use immutable=1 on a live DB or omit its WAL.
    """
    paths = [Path(str(database) + suffix) for suffix in ("", "-wal", "-journal")]
    before = [file_stamp(path) for path in paths]
    if before[0] is None:
        raise OSError("history database disappeared")
    if sum(stamp[2] for stamp in before[:2] if stamp) > SNAPSHOT_MAX_BYTES:
        raise OSError("history snapshot exceeds size limit")
    if time.monotonic() >= deadline:
        raise TimeoutError("history snapshot deadline exceeded")
    if before[2] and before[2][2]:
        with paths[2].open("rb") as journal:
            # PERSIST journals can remain after commit with their magic zeroed.
            # A live rollback journal may accompany uncommitted DB pages. Skip
            # it: recovery could also reference a super-journal outside our copy.
            if journal.read(8) != b"\0" * 8:
                raise OSError("history has an active rollback journal")
    snapshot = directory / "history.sqlite"
    for source, stamp, suffix in zip(paths[:2], before[:2], ("", "-wal")):
        if stamp is None:
            continue
        destination = Path(str(snapshot) + suffix)
        with source.open("rb") as reader, destination.open("xb") as writer:
            os.fchmod(writer.fileno(), 0o600)
            remaining = stamp[2]
            while remaining:
                if time.monotonic() >= deadline:
                    raise TimeoutError("history snapshot deadline exceeded")
                chunk = reader.read(min(remaining, SNAPSHOT_COPY_CHUNK))
                if not chunk:
                    raise SnapshotChanged("history file shrank during copy")
                writer.write(chunk)
                remaining -= len(chunk)
    if before != [file_stamp(path) for path in paths]:
        raise SnapshotChanged("history changed during copy")
    if time.monotonic() >= deadline:
        raise TimeoutError("history snapshot deadline exceeded")
    return snapshot


def query_history(database: Path, profile: Profile, tokens: list[str], cutoff_us: int,
                  now_us: int, candidate_limit: int, result_limit: int, deadline: float):
    offset = CHROMIUM_EPOCH_US if profile.browser.family == "chromium" else 0
    query = CHROMIUM_SQL if offset else FIREFOX_SQL
    connection = None
    matched_urls = set()
    entries = []
    try:
        connection = sqlite3.connect(
            database.as_uri() + "?mode=ro", uri=True,
            timeout=max(0, min(0.02, deadline - time.monotonic())),
        )
        connection.set_progress_handler(lambda: time.monotonic() >= deadline, 1000)
        connection.execute("PRAGMA query_only = ON")
        connection.execute("PRAGMA cache_size = -512")
        cursor = connection.execute(
            query, (cutoff_us + offset, now_us + offset, candidate_limit)
        )
        for url, title, visited in cursor:
            if time.monotonic() >= deadline:
                break
            if not isinstance(url, str) or len(url) > 8192 or url in matched_urls:
                continue
            title = title if isinstance(title, str) else ""
            if tokens:
                haystack = (title + "\n" + url + "\n" + unquote(url)).casefold()
                if not all(token in haystack for token in tokens):
                    continue
            if not safe_url(url):
                continue
            if not isinstance(visited, int):
                continue
            matched_urls.add(url)
            entries.append(HistoryEntry(title, url, visited - offset, profile))
            # Each profile only needs to contribute at most the global top N.
            if len(matched_urls) >= result_limit:
                break
        return entries
    finally:
        if connection is not None:
            connection.close()


def read_history(profile: Profile, tokens: list[str], cutoff_us: int, now_us: int,
                 candidate_limit: int, result_limit: int, deadline: float):
    db_deadline = min(deadline, time.monotonic() + DATABASE_TIMEOUT)
    if time.monotonic() >= db_deadline:
        return
    args = (profile, tokens, cutoff_us, now_us, candidate_limit, result_limit, db_deadline)
    try:
        yield from query_history(profile.database, *args)
        return
    except sqlite3.Error as error:
        # Read-only connections still honor exclusive browser locks. Missing or
        # unwritable WAL shared memory may also need a private, writable directory.
        code = getattr(error, "sqlite_errorcode", 0) & 0xff
        if code not in (sqlite3.SQLITE_BUSY, sqlite3.SQLITE_LOCKED,
                        sqlite3.SQLITE_CANTOPEN, sqlite3.SQLITE_READONLY):
            return
    except (OSError, ValueError):
        return

    for _ in range(2):
        if time.monotonic() >= db_deadline:
            return
        try:
            # Only locked profiles pay for a copy. This directory is private
            # (0700) and removed, including SQLite's sidecars, after each attempt.
            with tempfile.TemporaryDirectory(prefix="gpotlight-history-") as directory:
                snapshot = copy_snapshot(profile.database, Path(directory), db_deadline)
                entries = query_history(snapshot, *args)
            yield from entries
            return
        except SnapshotChanged:
            continue
        except (OSError, sqlite3.Error, ValueError):
            return


def search_history(query: str, config: dict, *, home: Path | None = None,
                   config_home: Path | None = None, now: float | None = None,
                   deadline: float | None = None) -> list[HistoryEntry]:
    deadline = deadline if deadline is not None else time.monotonic() + SEARCH_TIMEOUT
    home = home if home is not None else Path.home()
    if config_home is None:
        configured = Path(os.environ.get("XDG_CONFIG_HOME", str(home / ".config")))
        config_home = configured if configured.is_absolute() else home / ".config"
    now_us = int((time.time() if now is None else now) * 1_000_000)
    days = integer_setting(config, "history_days", 30, 1, 3650)
    limit = integer_setting(config, "max_results", 20, 1, 50)
    candidates = integer_setting(config, "max_history_entries", 5000, 100, 50000)
    cutoff_us = now_us - days * 86_400_000_000
    tokens = query.casefold().split()
    entries = {}
    for profile in discover_profiles(config, home, config_home, deadline):
        for entry in read_history(profile, tokens, cutoff_us, now_us, candidates, limit, deadline):
            current = entries.get(entry.url)
            if current is None or entry.visited_us > current.visited_us:
                entries[entry.url] = entry
    return sorted(entries.values(), key=lambda item: (-item.visited_us, item.url))[:limit]


def display_text(text: str, limit: int) -> str:
    return " ".join(text.split())[:limit]


def result_for_entry(entry: HistoryEntry) -> dict:
    visited = datetime.fromtimestamp(entry.visited_us / 1_000_000).strftime("%m-%d %H:%M")
    profile = entry.profile
    source = f"{profile.browser.name} · {display_text(profile.name, 80)} · {visited}"
    return {
        "title": display_text(entry.title, 512) or entry.url[:512],
        "subtitle": f"{source} — {entry.url[:512]}",
        "icon": profile.browser.icon,
        "action": {"type": "open-uri", "uri": entry.url},
        "buttons": [{
            "title": "复制链接",
            "icon": "edit-copy-symbolic",
            "action": {"type": "copy-text", "text": entry.url},
        }],
    }


def main() -> int:
    query = sys.argv[1] if len(sys.argv) > 1 else ""
    for entry in search_history(query, load_config()):
        print(json.dumps(result_for_entry(entry), ensure_ascii=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
