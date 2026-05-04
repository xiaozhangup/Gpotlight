#!/usr/bin/env python3
from __future__ import annotations

import base64
import hashlib
import json
import mimetypes
import os
import subprocess
import sys
import time
from pathlib import Path
from urllib.parse import urlparse
from urllib.request import Request, urlopen

try:
    import gi

    gi.require_version("Gio", "2.0")
    gi.require_version("GLib", "2.0")
    from gi.repository import Gio, GLib
except (ImportError, ValueError):
    raise SystemExit(0)


MPRIS_PREFIX = "org.mpris.MediaPlayer2."
PLAYER_PATH = "/org/mpris/MediaPlayer2"
ROOT_IFACE = "org.mpris.MediaPlayer2"
PLAYER_IFACE = "org.mpris.MediaPlayer2.Player"
PROPERTIES_IFACE = "org.freedesktop.DBus.Properties"


def main() -> int:
    if len(sys.argv) >= 3 and sys.argv[1] == "--cache-art":
        return cache_art_command(sys.argv[2])

    query = sys.argv[1].strip().lower() if len(sys.argv) > 1 else ""
    try:
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    except GLib.Error:
        return 0

    for name in player_names(bus):
        player = player_info(bus, name)
        if player and matches_query(player, query):
            print(json.dumps(result_for_player(player), ensure_ascii=False))
    return 0


def player_names(bus: Gio.DBusConnection) -> list[str]:
    try:
        result = bus.call_sync(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "ListNames",
            None,
            GLib.VariantType.new("(as)"),
            Gio.DBusCallFlags.NONE,
            1000,
            None,
        )
    except GLib.Error:
        return []

    names = result.unpack()[0]
    return sorted(name for name in names if name.startswith(MPRIS_PREFIX))


def player_info(bus: Gio.DBusConnection, bus_name: str) -> dict | None:
    metadata = get_property(bus, bus_name, PLAYER_IFACE, "Metadata")
    if not isinstance(metadata, dict):
        return None

    identity = variant_text(get_property(bus, bus_name, ROOT_IFACE, "Identity"))
    desktop_entry = variant_text(get_property(bus, bus_name, ROOT_IFACE, "DesktopEntry"))
    label = identity or player_label(bus_name)
    title = variant_text(metadata.get("xesam:title")) or player_label(bus_name)
    artists = variant_strv(metadata.get("xesam:artist"))
    album = variant_text(metadata.get("xesam:album"))
    art_url = variant_text(metadata.get("mpris:artUrl"))
    length_us = variant_int(metadata.get("mpris:length"))
    position_us = variant_int(get_property(bus, bus_name, PLAYER_IFACE, "Position"))
    status = variant_text(get_property(bus, bus_name, PLAYER_IFACE, "PlaybackStatus")) or "Stopped"

    source_icon = desktop_entry_icon(desktop_entry) or desktop_entry or player_icon_name(bus_name)
    return {
        "bus_name": bus_name,
        "label": label,
        "title": title,
        "artists": artists,
        "album": album,
        "icon": media_icon(art_url) or "audio-x-generic-symbolic",
        "source_icon": source_icon,
        "length_us": length_us,
        "position_us": position_us,
        "status": status,
    }


def get_property(bus: Gio.DBusConnection, bus_name: str, interface: str, prop: str):
    try:
        result = bus.call_sync(
            bus_name,
            PLAYER_PATH,
            PROPERTIES_IFACE,
            "Get",
            GLib.Variant("(ss)", (interface, prop)),
            GLib.VariantType.new("(v)"),
            Gio.DBusCallFlags.NONE,
            1000,
            None,
        )
    except GLib.Error:
        return None

    return result.unpack()[0]


def call_action(bus_name: str, method: str) -> dict:
    return {
        "type": "launch-command",
        "command": "gdbus",
        "args": [
            "call",
            "--session",
            "--dest",
            bus_name,
            "--object-path",
            PLAYER_PATH,
            "--method",
            f"{PLAYER_IFACE}.{method}",
        ],
    }


def result_for_player(player: dict) -> dict:
    title = player["title"]
    artists = ", ".join(player["artists"])
    status = player["status"]
    progress = format_progress(player["position_us"], player["length_us"])
    subtitle_parts = [part for part in [artists, player["album"], progress, status] if part]
    subtitle = " - ".join(subtitle_parts)

    return {
        "title": title,
        "subtitle": subtitle,
        "icon": player["icon"],
        "pinned": True,
        "refresh_key": player["bus_name"],
        "refresh_interval_ms": 1000,
        "action": call_action(player["bus_name"], "PlayPause"),
        "buttons": [
            {
                "title": player["label"],
                "icon": player["source_icon"],
                "close_on_activate": False,
                "action": {"type": "noop"},
            },
            {
                "title": "上一首",
                "icon": "media-skip-backward-symbolic",
                "close_on_activate": False,
                "refresh_after_ms": 180,
                "action": call_action(player["bus_name"], "Previous"),
            },
            {
                "title": "播放/暂停",
                "icon": play_pause_icon(status),
                "close_on_activate": False,
                "refresh_after_ms": 180,
                "action": call_action(player["bus_name"], "PlayPause"),
            },
            {
                "title": "下一首",
                "icon": "media-skip-forward-symbolic",
                "close_on_activate": False,
                "refresh_after_ms": 180,
                "action": call_action(player["bus_name"], "Next"),
            },
        ],
    }


def matches_query(player: dict, query: str) -> bool:
    if not query:
        return True
    haystack = " ".join(
        [
            player["label"],
            player["title"],
            player["album"],
            " ".join(player["artists"]),
            player["status"],
        ]
    ).lower()
    return query in haystack


def player_label(bus_name: str) -> str:
    label = bus_name.removeprefix(MPRIS_PREFIX)
    return label.replace(".instance", "").replace("_", " ").title()


def player_icon_name(bus_name: str) -> str:
    return bus_name.removeprefix(MPRIS_PREFIX).split(".")[0]


def media_icon(art_url: str) -> str:
    if art_url.startswith("file://"):
        try:
            return GLib.filename_from_uri(art_url)[0]
        except GLib.Error:
            return ""
    if art_url.startswith("http://") or art_url.startswith("https://"):
        return cached_remote_art(art_url)
    if art_url.startswith("data:image/"):
        return cached_data_art(art_url)
    return ""


def cached_remote_art(art_url: str) -> str:
    path = remote_art_path(art_url)
    if path.exists():
        return str(path)

    spawn_art_cache_worker(art_url)
    return ""


def cache_art_command(art_url: str) -> int:
    path = remote_art_path(art_url)
    pending = pending_path(path)
    if path.exists():
        clear_pending(pending)
        return 0

    try:
        request = Request(art_url, headers={"User-Agent": "Gpotlight/0.1"})
        with urlopen(request, timeout=1.5) as response:
            data = response.read(2_000_000)
    except OSError:
        clear_pending(pending)
        return 0

    write_art_cache(path, data)
    clear_pending(pending)
    return 0


def spawn_art_cache_worker(art_url: str) -> None:
    path = remote_art_path(art_url)
    pending = pending_path(path)
    if pending.exists() and time.time() - pending.stat().st_mtime < 30:
        return

    try:
        pending.parent.mkdir(parents=True, exist_ok=True)
        pending.touch()
        subprocess.Popen(
            [sys.executable, str(Path(__file__).resolve()), "--cache-art", art_url],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError:
        clear_pending(pending)


def remote_art_path(art_url: str) -> Path:
    parsed = urlparse(art_url)
    suffix = Path(parsed.path).suffix.lower()
    if suffix not in {".jpg", ".jpeg", ".png", ".webp"}:
        suffix = ".img"
    return art_cache_dir() / f"{sha256_text(art_url)}{suffix}"


def pending_path(path: Path) -> Path:
    return path.with_name(f"{path.name}.pending")


def clear_pending(path: Path) -> None:
    try:
        path.unlink()
    except OSError:
        pass


def cached_data_art(art_url: str) -> str:
    header, separator, payload = art_url.partition(",")
    if not separator or ";base64" not in header:
        return ""

    content_type = header.removeprefix("data:").split(";", 1)[0]
    suffix = suffix_for_content_type(content_type) or ".img"
    path = art_cache_dir() / f"{sha256_text(art_url)}{suffix}"
    if path.exists():
        return str(path)

    try:
        data = base64.b64decode(payload, validate=True)
    except ValueError:
        return ""
    return write_art_cache(path, data)


def write_art_cache(path: Path, data: bytes) -> str:
    if not data:
        return ""
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    except OSError:
        return ""
    return str(path)


def art_cache_dir() -> Path:
    cache_home = os.environ.get("XDG_CACHE_HOME")
    if cache_home:
        return Path(cache_home) / "gpotlight" / "mpris-art"
    return Path.home() / ".cache" / "gpotlight" / "mpris-art"


def suffix_for_content_type(content_type: str) -> str:
    if content_type == "image/jpeg":
        return ".jpg"
    if content_type == "image/png":
        return ".png"
    if content_type == "image/webp":
        return ".webp"
    return mimetypes.guess_extension(content_type) or ""


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def desktop_entry_icon(desktop_entry: str) -> str:
    if not desktop_entry:
        return ""
    desktop_id = desktop_entry if desktop_entry.endswith(".desktop") else f"{desktop_entry}.desktop"
    for path in desktop_file_paths(desktop_id):
        try:
            for line in path.read_text(encoding="utf-8").splitlines():
                if line.startswith("Icon="):
                    icon = line.removeprefix("Icon=").strip()
                    if icon:
                        return icon
        except OSError:
            continue
    return ""


def desktop_file_paths(desktop_id: str) -> list[Path]:
    paths = []
    data_home = os.environ.get("XDG_DATA_HOME")
    if data_home:
        paths.append(Path(data_home) / "applications" / desktop_id)
    else:
        paths.append(Path.home() / ".local/share/applications" / desktop_id)

    data_dirs = os.environ.get("XDG_DATA_DIRS", "/usr/local/share:/usr/share")
    paths.extend(Path(path) / "applications" / desktop_id for path in data_dirs.split(":") if path)
    return paths


def play_pause_icon(status: str) -> str:
    if status == "Playing":
        return "media-playback-pause-symbolic"
    return "media-playback-start-symbolic"


def format_duration(length_us: int | None) -> str:
    if not length_us or length_us <= 0:
        return ""
    seconds = length_us // 1_000_000
    minutes, seconds = divmod(seconds, 60)
    hours, minutes = divmod(minutes, 60)
    if hours:
        return f"{hours}:{minutes:02}:{seconds:02}"
    return f"{minutes}:{seconds:02}"


def format_progress(position_us: int | None, length_us: int | None) -> str:
    if position_us is None and length_us is None:
        return ""
    if length_us and length_us > 0:
        return f"{format_duration(position_us or 0)} / {format_duration(length_us)}"
    return format_duration(position_us)


def variant_text(value) -> str:
    if value is None:
        return ""
    unpacked = value.unpack() if hasattr(value, "unpack") else value
    return unpacked if isinstance(unpacked, str) else ""


def variant_strv(value) -> list[str]:
    if value is None:
        return []
    unpacked = value.unpack() if hasattr(value, "unpack") else value
    return [item for item in unpacked if isinstance(item, str)] if isinstance(unpacked, list) else []


def variant_int(value) -> int | None:
    if value is None:
        return None
    unpacked = value.unpack() if hasattr(value, "unpack") else value
    return unpacked if isinstance(unpacked, int) else None


if __name__ == "__main__":
    raise SystemExit(main())
