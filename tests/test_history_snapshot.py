"""Bounds and race handling for temporary history copies; synthetic files only."""

from pathlib import Path
import sqlite3
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "plugins"))
import browser_history as history


class HistorySnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.database = self.root / "History"
        self.database.write_bytes(b"synthetic database")
        self.output = self.root / "output"
        self.output.mkdir()

    def copy(self):
        return history.copy_snapshot(self.database, self.output, time.monotonic() + 1)

    def test_copy_includes_wal_but_does_not_reuse_browser_shared_memory(self):
        Path(str(self.database) + "-wal").write_bytes(b"synthetic wal")
        Path(str(self.database) + "-shm").write_bytes(b"browser shared memory")
        snapshot = self.copy()
        self.assertEqual(snapshot.read_bytes(), self.database.read_bytes())
        self.assertEqual(Path(str(snapshot) + "-wal").read_bytes(), b"synthetic wal")
        self.assertFalse(Path(str(snapshot) + "-shm").exists())
        self.assertEqual(snapshot.stat().st_mode & 0o777, 0o600)

    def test_combined_size_limit_rejects_copy_before_allocating_files(self):
        Path(str(self.database) + "-wal").write_bytes(b"large wal" * 128)
        with patch.object(history, "SNAPSHOT_MAX_BYTES", 1024):
            with self.assertRaises(OSError):
                self.copy()
        self.assertEqual(list(self.output.iterdir()), [])

    def test_deadline_interrupts_between_copy_chunks(self):
        self.database.write_bytes(b"x" * 100)
        with patch.object(history, "SNAPSHOT_COPY_CHUNK", 10), \
                patch.object(history.time, "monotonic", side_effect=[0, 0, 2]):
            with self.assertRaises(TimeoutError):
                history.copy_snapshot(self.database, self.output, 1)
        self.assertEqual((self.output / "history.sqlite").stat().st_size, 10)

    def test_detects_wal_created_or_rewritten_during_copy(self):
        wal = Path(str(self.database) + "-wal")
        original_stamp = history.file_stamp
        for initially_present in (False, True):
            with self.subTest(initially_present=initially_present):
                destination = self.output / str(initially_present)
                destination.mkdir()
                if initially_present:
                    wal.write_bytes(b"old wal")
                calls = 0

                def change_before_recheck(path):
                    nonlocal calls
                    calls += 1
                    if calls == 4:
                        wal.write_bytes(b"new wal transaction")
                    return original_stamp(path)

                with patch.object(history, "file_stamp", side_effect=change_before_recheck):
                    with self.assertRaises(history.SnapshotChanged):
                        history.copy_snapshot(self.database, destination, time.monotonic() + 1)

    def test_detects_database_replaced_with_same_size_during_copy(self):
        original_stamp = history.file_stamp
        calls = 0

        def replace_before_recheck(path):
            nonlocal calls
            calls += 1
            if calls == 4:
                replacement = self.root / "replacement"
                replacement.write_bytes(self.database.read_bytes())
                replacement.replace(self.database)
            return original_stamp(path)

        with patch.object(history, "file_stamp", side_effect=replace_before_recheck):
            with self.assertRaises(history.SnapshotChanged):
                self.copy()

    def test_invalidated_persist_journal_is_allowed(self):
        Path(str(self.database) + "-journal").write_bytes(b"\0" * 8 + b"old journal content")
        self.assertEqual(self.copy().read_bytes(), self.database.read_bytes())

    def run_fallback(self, copy_action):
        busy = sqlite3.OperationalError("database is locked")
        busy.sqlite_errorcode = sqlite3.SQLITE_BUSY
        profile = history.Profile(history.BROWSERS[0], "Synthetic", self.database)
        expected = history.HistoryEntry("Test", "https://example.test/", 1, profile)
        directories = []
        original_copy = history.copy_snapshot

        def copy(database, directory, deadline):
            directories.append(directory)
            self.assertEqual(directory.stat().st_mode & 0o777, 0o700)
            snapshot = original_copy(database, directory, deadline)
            return copy_action(snapshot, len(directories))

        with patch.object(history, "copy_snapshot", side_effect=copy), \
                patch.object(history, "query_history", side_effect=[busy, [expected]]) as query:
            entries = list(history.read_history(profile, [], 0, 2, 100, 20,
                                                time.monotonic() + 1))
        self.assertTrue(directories)
        self.assertTrue(all(not directory.exists() for directory in directories))
        return entries, expected, directories, query.call_count

    def test_changed_snapshot_is_retried_and_all_copies_are_cleaned(self):
        def change_once(snapshot, attempt):
            if attempt == 1:
                raise history.SnapshotChanged("writer changed the WAL")
            return snapshot

        entries, expected, directories, calls = self.run_fallback(change_once)
        self.assertEqual(entries, [expected])
        self.assertEqual(len(directories), 2)
        self.assertEqual(calls, 2)

    def test_continuous_changes_stop_after_two_attempts(self):
        def change_always(snapshot, attempt):
            raise history.SnapshotChanged("writer is busy")

        entries, _, directories, calls = self.run_fallback(change_always)
        self.assertEqual(entries, [])
        self.assertEqual(len(directories), 2)
        self.assertEqual(calls, 1)

    def test_copy_failure_cleans_partial_files_without_retry(self):
        def fail(snapshot, attempt):
            raise OSError("write failed")

        entries, _, directories, calls = self.run_fallback(fail)
        self.assertEqual(entries, [])
        self.assertEqual(len(directories), 1)
        self.assertEqual(calls, 1)


if __name__ == "__main__":
    unittest.main()
