"""Synthetic browser profiles only: never access the developer's history."""

import json
import os
from contextlib import closing, contextmanager
from pathlib import Path
import sqlite3
import select
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "plugins"))
import browser_history as history


NOW = 1_800_000_000
DAY = 86400


# An independent process must own these locks: POSIX releases a process's fcntl
# locks if that same process closes *any* descriptor pointing at the same file.
# That would make a same-process raw-copy test accidentally unlock its writer.
EXCLUSIVE_WRITER = r"""
import sqlite3
import sys

database, family, mode, now = sys.argv[1:]
now = int(now)
connection = sqlite3.connect(database)
try:
    connection.execute('PRAGMA locking_mode=EXCLUSIVE')
    connection.execute('PRAGMA journal_mode=' + ('WAL' if mode == 'wal' else 'DELETE'))
    connection.execute('PRAGMA wal_autocheckpoint=0')
    connection.execute('PRAGMA cache_size=5')
    connection.execute('PRAGMA cache_spill=ON')

    def add(index, title, timestamp):
        url = f'https://locked.example.test/{index}'
        if family == 'chromium':
            connection.execute('INSERT INTO urls VALUES (?, ?, ?, 0)', (index, url, title))
            connection.execute('INSERT INTO visits VALUES (?, ?, ?)',
                               (index, index, timestamp * 1000000 + 11644473600000000))
        else:
            connection.execute('INSERT INTO moz_places VALUES (?, ?, ?, ?, 0)',
                               (index, url, title, timestamp * 1000000))

    if mode == 'wal':
        add(2, 'Latest committed WAL', now - 2)
        connection.commit()
        connection.execute('BEGIN IMMEDIATE')
        add(3, 'Uncommitted WAL', now - 1)
    else:
        connection.execute('BEGIN EXCLUSIVE')
        if mode == 'rollback-idle':
            connection.commit()
        else:
            add(2, 'Uncommitted rollback', now - 1)

    if mode != 'rollback-idle':
        # Force dirty pages out of SQLite's small cache, so the tests exercise
        # uncommitted WAL frames / a nonzero rollback-journal header on disk.
        for index in range(10, 210):
            add(index, 'uncommitted padding ' + 'x' * 4000, now - 365 * 86400)

    print('ready', flush=True)
    for line in sys.stdin:
        if line.strip() == 'stop':
            break
finally:
    connection.rollback()
    connection.close()
"""


class BrowserHistoryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.home = Path(self.temporary.name)
        self.config_home = self.home / ".config"

    def chromium(self, directory, entries, *, index=True):
        directory.mkdir(parents=True, exist_ok=True)
        database = directory / "History"
        with closing(sqlite3.connect(database)) as connection, connection:
            connection.executescript("""
                CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT, hidden INTEGER);
                CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
                CREATE INDEX urls_url_index ON urls(url);
            """)
            if index:
                connection.execute("CREATE INDEX visits_time_index ON visits(visit_time)")
            for index, (url, title, seconds, *hidden) in enumerate(entries, 1):
                connection.execute("INSERT INTO urls VALUES (?, ?, ?, ?)",
                                   (index, url, title, hidden[0] if hidden else 0))
                connection.execute("INSERT INTO visits VALUES (?, ?, ?)",
                                   (index, index, int(seconds * 1_000_000) + history.CHROMIUM_EPOCH_US))
        return database

    def firefox(self, directory, entries):
        directory.mkdir(parents=True, exist_ok=True)
        database = directory / "places.sqlite"
        with closing(sqlite3.connect(database)) as connection, connection:
            connection.executescript("""
                CREATE TABLE moz_places(id INTEGER PRIMARY KEY, url TEXT, title TEXT,
                                        last_visit_date INTEGER, hidden INTEGER);
                CREATE INDEX moz_places_lastvisitdateindex ON moz_places(last_visit_date);
            """)
            for index, (url, title, seconds, *hidden) in enumerate(entries, 1):
                connection.execute("INSERT INTO moz_places VALUES (?, ?, ?, ?, ?)",
                                   (index, url, title, int(seconds * 1_000_000), hidden[0] if hidden else 0))
        return database

    def search(self, query="", config=None, **kwargs):
        return history.search_history(query, config or {}, home=self.home,
                                      config_home=self.config_home, now=NOW, **kwargs)

    def profiles(self, config=None):
        return list(history.discover_profiles(config or {}, self.home, self.config_home,
                                              time.monotonic() + 2))

    @contextmanager
    def exclusive_writer(self, database, family, mode):
        with subprocess.Popen(
            [sys.executable, "-c", EXCLUSIVE_WRITER, str(database), family, mode, str(NOW)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        ) as writer:
            try:
                self.assertTrue(select.select([writer.stdout], [], [], 3)[0], "writer failed to start")
                self.assertEqual(writer.stdout.readline(), "ready\n")
                yield writer
            finally:
                try:
                    _, error = writer.communicate("stop\n", timeout=3)
                except subprocess.TimeoutExpired:
                    writer.kill()
                    _, error = writer.communicate()
                self.assertEqual(writer.returncode, 0, error)

    def assert_locked(self, database):
        with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=0)) as contender:
            with self.assertRaises(sqlite3.OperationalError) as failure:
                contender.execute("SELECT count(*) FROM sqlite_master").fetchone()
            self.assertIn(failure.exception.sqlite_errorcode & 255, (sqlite3.SQLITE_BUSY, sqlite3.SQLITE_LOCKED))

    def source_bytes(self, database):
        return {
            suffix: path.read_bytes() if path.exists() else None
            for suffix in ("", "-wal", "-journal")
            for path in [Path(str(database) + suffix)]
        }

    def test_native_and_flatpak_chromium_browsers(self):
        for browser in history.BROWSERS:
            if browser.family != "chromium":
                continue
            with self.subTest(browser=browser.key):
                native = self.config_home / browser.config_dirs[0] / "Default"
                flatpak = self.home / ".var/app" / browser.flatpak_ids[0] / "config" / browser.config_dirs[0] / "Profile 2"
                self.chromium(native, [])
                self.chromium(flatpak, [])
                found = {profile.database for profile in self.profiles()}
                self.assertIn(native / "History", found)
                self.assertIn(flatpak / "History", found)

    def test_chromium_local_state_profile_names_and_fallback(self):
        root = self.config_home / "google-chrome"
        self.chromium(root / "Named", [])
        self.chromium(root / "Profile 2", [])
        self.chromium(root / "Default", [])
        (root / "Local State").write_text(json.dumps({"profile": {"info_cache": {
            "Named": {"name": "Work"}, "Default": {"name": "Personal"},
            "../../outside": {"name": "Ignore"}
        }}}))
        profiles = self.profiles()
        self.assertEqual(len(profiles), 3)
        self.assertIn("Work", [profile.name for profile in profiles])
        self.assertIn("Personal", [profile.name for profile in profiles])

    def test_opera_history_directly_in_root(self):
        database = self.chromium(self.config_home / "opera", [])
        self.assertEqual([profile.database for profile in self.profiles()], [database])

    def test_firefox_ini_relative_absolute_and_percent_name(self):
        root = self.home / ".mozilla/firefox"
        relative = self.firefox(root / "abc.default-release", [])
        absolute = self.firefox(self.home / "custom profile", [])
        (root / "profiles.ini").write_text(
            "[Profile0]\nName=100% Work\nIsRelative=1\nPath=abc.default-release\n"
            f"[Profile1]\nName=Outside\nIsRelative=0\nPath={absolute.parent}\n"
        )
        profiles = self.profiles()
        self.assertEqual({profile.database for profile in profiles}, {relative, absolute})
        self.assertIn("100% Work", [profile.name for profile in profiles])

    def test_firefox_flatpak_xdg_and_snap_roots(self):
        roots = [self.home / ".var/app/org.mozilla.firefox/.mozilla/firefox",
                 self.home / "snap/firefox/common/.mozilla/firefox",
                 self.config_home / "mozilla/firefox"]
        expected = {self.firefox(root / "abc.default", []) for root in roots}
        self.assertEqual({profile.database for profile in self.profiles()}, expected)

    def test_zen_native_xdg_flatpak_and_legacy_roots(self):
        roots = [self.home / ".zen", self.config_home / "zen",
                 self.home / ".var/app/app.zen_browser.zen/.zen",
                 self.home / ".var/app/app.zen_browser.zen/zen",
                 self.home / ".var/app/io.github.zen_browser.zen/.zen"]
        expected = {self.firefox(root / "abc.Default (release)", []) for root in roots}
        profiles = self.profiles()
        self.assertEqual({profile.database for profile in profiles}, expected)
        self.assertTrue(all(profile.browser.key == "zen" for profile in profiles))

    def test_discovery_deduplicates_symlinks_and_does_not_recurse_home(self):
        root = self.home / ".zen"
        original = self.firefox(root / "profile", [])
        (root / "alias").symlink_to(original.parent, target_is_directory=True)
        self.firefox(self.home / "unrelated/deep/profile", [])
        self.assertEqual(len(self.profiles()), 1)

    def test_disabled_browser_skips_discovery(self):
        self.firefox(self.home / ".zen/profile", [])
        self.assertEqual(self.profiles({"zen_enabled": False}), [])

    def test_epochs_recency_global_sort_and_duplicate_urls(self):
        shared = "https://example.test/shared"
        self.chromium(self.config_home / "google-chrome/Default", [
            (shared, "older Chrome", NOW - 20),
            ("https://example.test/new", "Newest", NOW - 1),
            ("https://example.test/old", "Too old", NOW - 31 * DAY),
        ])
        self.firefox(self.home / ".zen/profile", [
            (shared, "newer Zen", NOW - 10),
            ("https://example.test/middle", "Middle", NOW - 15),
        ])
        entries = self.search()
        self.assertEqual([entry.title for entry in entries], ["Newest", "newer Zen", "Middle"])
        self.assertEqual(entries[0].visited_us, (NOW - 1) * 1_000_000)

    def test_unicode_tokens_url_decoding_and_literal_wildcards(self):
        self.firefox(self.home / ".mozilla/firefox/profile", [
            ("https://example.test/%E4%B8%AD%E6%96%87", "Straße 文档", NOW - 1),
            ("https://example.test/percent", "100%_safe", NOW - 2),
            ("https://example.test/other", "1000unsafe", NOW - 3),
        ])
        self.assertEqual(len(self.search("STRASSE 中文")), 1)
        self.assertEqual(len(self.search("%E4%B8%AD")), 1)
        self.assertEqual([entry.title for entry in self.search("%_")], ["100%_safe"])
        self.assertEqual(self.search("' OR 1=1 --"), [])

    def test_only_visible_http_https_history_with_valid_dates(self):
        urls = ["https://example.test/ok", "http://example.test/ok", "file:///tmp/file",
                "javascript:alert(1)", "about:config", "https://", "https://example.test/\n"]
        entries = [(url, "title", NOW - 1) for url in urls]
        entries.extend([("https://example.test/hidden", "hidden", NOW - 1, 1),
                        ("https://example.test/future", "future", NOW + DAY)])
        self.firefox(self.home / ".zen/profile", entries)
        self.assertEqual({entry.url for entry in self.search()}, set(urls[:2]))

    def test_result_limit_and_history_days(self):
        entries = [(f"https://example.test/{n}", str(n), NOW - n * DAY) for n in range(1, 12)]
        self.chromium(self.config_home / "chromium/Default", entries)
        self.assertEqual(len(self.search(config={"max_results": 3})), 3)
        self.assertEqual(len(self.search(config={"history_days": 2})), 2)

    def test_candidate_limit_bounds_nonmatching_chromium_and_firefox(self):
        for browser in ("chrome", "firefox"):
            with self.subTest(browser=browser):
                entries = [(f"https://example.test/{browser}/{n}", "recent", NOW - n) for n in range(101)]
                entries[-1] = (f"https://example.test/{browser}/old", "needle", NOW - 100)
                if browser == "chrome":
                    self.chromium(self.config_home / "google-chrome/Default", entries)
                else:
                    self.firefox(self.home / ".mozilla/firefox/profile", entries)
        self.assertEqual(self.search("needle", {"max_history_entries": 100}), [])
        self.assertEqual(len(self.search("needle", {"max_history_entries": 200})), 2)

    def test_repeated_chromium_visits_do_not_duplicate_results(self):
        database = self.chromium(self.config_home / "chromium/Default", [
            ("https://example.test/repeat", "Repeated", NOW - 10),
            ("https://example.test/second", "Second", NOW - 20),
        ])
        with closing(sqlite3.connect(database)) as connection, connection:
            connection.execute("INSERT INTO visits VALUES (3, 1, ?)",
                               ((NOW - 1) * 1_000_000 + history.CHROMIUM_EPOCH_US,))
        entries = self.search()
        self.assertEqual(len(entries), 2)
        self.assertEqual(entries[0].visited_us, (NOW - 1) * 1_000_000)

    def test_corrupt_database_and_metadata_do_not_hide_other_profiles(self):
        root = self.config_home / "google-chrome"
        broken = root / "Default/History"
        broken.parent.mkdir(parents=True)
        broken.write_bytes(b"not a database")
        (root / "Local State").write_text("{bad json")
        self.chromium(root / "Profile 1", [("https://example.test/ok", "Good", NOW - 1)])
        self.assertEqual([entry.title for entry in self.search()], ["Good"])

    def test_idle_exclusive_rollback_is_read_without_disturbing_writer(self):
        database = self.chromium(self.config_home / "chromium/Default", [
            ("https://example.test/locked", "Locked committed", NOW - 2),
        ])
        self.firefox(self.home / ".zen/profile", [("https://example.test/ok", "Good", NOW - 1)])
        with self.exclusive_writer(database, "chromium", "rollback-idle") as writer:
            self.assert_locked(database)
            before = self.source_bytes(database)
            self.assertIsNone(before["-journal"])
            start = time.monotonic()
            entries = self.search()
            elapsed = time.monotonic() - start
            self.assertEqual([entry.title for entry in entries], ["Good", "Locked committed"])
            self.assertEqual(self.source_bytes(database), before)
            self.assertIsNone(writer.poll())
            self.assert_locked(database)
            self.assertLess(elapsed, 0.5)

    def test_exclusive_wal_reads_committed_history_but_not_uncommitted_frames(self):
        for browser in ("chromium", "zen"):
            with self.subTest(browser=browser):
                initial = [("https://example.test/base", "Base committed", NOW - 10)]
                if browser == "chromium":
                    database = self.chromium(self.config_home / "chromium/Default", initial)
                else:
                    database = self.firefox(self.home / ".zen/profile", initial)
                with self.exclusive_writer(database, browser, "wal") as writer:
                    self.assert_locked(database)
                    before = self.source_bytes(database)
                    self.assertIsNotNone(before["-wal"])
                    self.assertGreater(len(before["-wal"]), 100000)
                    # Select only this browser so the earlier subtest's fixture
                    # cannot mask a failure to recover the locked current one.
                    config = {item.key + "_enabled": item.key == browser for item in history.BROWSERS}
                    entries = self.search(config=config)
                    self.assertEqual([entry.title for entry in entries],
                                     ["Latest committed WAL", "Base committed"])
                    self.assertEqual(self.source_bytes(database), before)
                    self.assertIsNone(writer.poll())
                    self.assert_locked(database)

    def test_dirty_exclusive_rollback_is_skipped_without_hiding_other_profiles(self):
        database = self.chromium(self.config_home / "chromium/Default", [
            ("https://example.test/base", "Locked base", NOW - 2),
        ])
        self.firefox(self.home / ".zen/profile", [("https://example.test/ok", "Good", NOW - 1)])
        with self.exclusive_writer(database, "chromium", "rollback-dirty") as writer:
            self.assert_locked(database)
            before = self.source_bytes(database)
            self.assertIsNotNone(before["-journal"])
            self.assertNotEqual(before["-journal"][:8], b"\0" * 8)
            self.assertEqual([entry.title for entry in self.search()], ["Good"])
            self.assertEqual(self.source_bytes(database), before)
            self.assertIsNone(writer.poll())
            self.assert_locked(database)

    def test_live_wal_committed_rows_are_visible_without_database_copy(self):
        database = self.firefox(self.home / ".zen/profile", [])
        writer = sqlite3.connect(database)
        self.addCleanup(writer.close)
        writer.execute("PRAGMA journal_mode=WAL")
        writer.execute("PRAGMA wal_autocheckpoint=0")
        writer.execute("INSERT INTO moz_places VALUES (1, ?, ?, ?, 0)",
                       ("https://example.test/wal", "Live WAL", (NOW - 1) * 1_000_000))
        writer.commit()
        self.assertTrue(Path(str(database) + "-wal").exists())
        before = database.read_bytes()
        with patch.object(history, "copy_snapshot", side_effect=AssertionError("unlocked history must not be copied")) as snapshot:
            self.assertEqual([entry.title for entry in self.search()], ["Live WAL"])
            snapshot.assert_not_called()
        self.assertEqual(database.read_bytes(), before)
        self.assertFalse((self.home / ".cache").exists())

    def test_expired_deadline_stops_before_profile_access(self):
        with patch.object(history, "browser_roots") as roots:
            self.assertEqual(self.search(deadline=time.monotonic() - 1), [])
            roots.assert_not_called()

    def test_readonly_does_not_create_missing_database(self):
        browser = history.BROWSERS[0]
        database = self.home / "missing"
        profile = history.Profile(browser, "Missing", database)
        entries = list(history.read_history(profile, [], 0, NOW * 1_000_000, 100, 20,
                                            time.monotonic() + 1))
        self.assertEqual(entries, [])
        self.assertFalse(database.exists())

    def test_sql_uses_existing_time_indexes(self):
        chrome = self.chromium(self.config_home / "chromium/Default", [])
        firefox = self.firefox(self.home / ".zen/profile", [])
        for database, sql, expected in [(chrome, history.CHROMIUM_SQL, "visits_time_index"),
                                         (firefox, history.FIREFOX_SQL, "moz_places_lastvisitdateindex")]:
            with closing(sqlite3.connect(database)) as connection:
                plan = connection.execute("EXPLAIN QUERY PLAN " + sql, (0, NOW, 5000)).fetchall()
                self.assertIn(expected, " ".join(str(row) for row in plan))

    def test_profile_uri_encodes_special_filename_characters(self):
        self.firefox(self.home / ".zen/profile #1?", [("https://example.test/ok", "Encoded", NOW - 1)])
        self.assertEqual([entry.title for entry in self.search()], ["Encoded"])

    def test_missing_schema_is_skipped(self):
        database = self.chromium(self.config_home / "chromium/Default", [])
        with closing(sqlite3.connect(database)) as connection, connection:
            connection.execute("DROP TABLE visits")
        self.assertEqual(self.search(), [])

    def test_progress_handler_interrupts_unindexed_database(self):
        database = self.chromium(self.config_home / "chromium/Default", [], index=False)
        with closing(sqlite3.connect(database)) as connection, connection:
            connection.executemany("INSERT INTO visits VALUES (?, 1, ?)",
                                   ((n, NOW * 1_000_000 + history.CHROMIUM_EPOCH_US)
                                    for n in range(100000)))
        profile = history.Profile(history.BROWSERS[0], "Slow", database)
        started = time.monotonic()
        with patch.object(history, "DATABASE_TIMEOUT", 0.001):
            entries = list(history.read_history(profile, [], 0, NOW * 1_000_000,
                                                5000, 20, time.monotonic() + 1))
        self.assertEqual(entries, [])
        self.assertLess(time.monotonic() - started, 0.2)

    def test_bad_config_and_integer_bounds(self):
        for value in ("invalid", "[]", "null", "1"):
            with patch.dict(os.environ, {"GPOTLIGHT_PLUGIN_CONFIG": value}):
                self.assertEqual(history.load_config(), {})
        for value, expected in [(-1, 1), (900, 50), (True, 20), ("50", 20)]:
            self.assertEqual(history.integer_setting({"n": value}, "n", 20, 1, 50), expected)

    def test_plugin_json_opens_url_and_offers_copy(self):
        entry = ("https://example.test/search?q=%E4%B8%AD%E6%96%87", "中文 page", time.time() - 60)
        self.firefox(self.home / ".zen/profile", [entry])
        environment = dict(os.environ, HOME=str(self.home), XDG_CONFIG_HOME=str(self.config_home),
                           GPOTLIGHT_PLUGIN_CONFIG="{}")
        result = subprocess.run([sys.executable, history.__file__, "中文"],
                                env=environment, capture_output=True, text=True, timeout=3, check=True)
        records = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(records), 1)
        self.assertEqual(records[0]["action"], {"type": "open-uri", "uri": entry[0]})
        self.assertEqual(records[0]["buttons"][0]["action"], {"type": "copy-text", "text": entry[0]})
        self.assertIn("Zen", records[0]["subtitle"])


if __name__ == "__main__":
    unittest.main()
