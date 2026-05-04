#!/usr/bin/env python3
import json
import os
import sqlite3
import sys
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import unquote, urlparse


@dataclass
class Project:
    name: str
    path: str
    source: str
    priority: int
    sort_index: int


def main() -> int:
    query = sys.argv[1].strip().lower() if len(sys.argv) > 1 else ""
    config = load_config()
    projects = {}

    if config.get("recent_enabled", True):
        for project in storage_json_projects():
            add_match(projects, project, query)
        for project in recent_projects():
            add_match(projects, project, query)
        for project in git_repository_projects():
            add_match(projects, project, query)

    if config.get("project_manager_enabled", True):
        for project in project_manager_projects():
            add_match(projects, project, query)

    results = sorted(projects.values(), key=lambda item: (item.priority, item.sort_index, item.name.lower()))
    for project in results[:20]:
        print(json.dumps(result_for_project(project), ensure_ascii=False))

    return 0


def load_config() -> dict:
    try:
        return json.loads(os.environ.get("GPOTLIGHT_PLUGIN_CONFIG", "{}"))
    except json.JSONDecodeError:
        return {}


def add_match(projects: dict[str, Project], project: Project, query: str) -> None:
    if query and query not in project.name.lower() and query not in project.path.lower():
        return

    key = str(Path(project.path).resolve())
    current = projects.get(key)
    if current is None or (project.priority, project.sort_index) < (current.priority, current.sort_index):
        projects[key] = project


def recent_projects() -> list[Project]:
    state_db = Path.home() / ".config/Code/User/globalStorage/state.vscdb"
    if not state_db.exists():
        return []

    projects = []
    try:
        with sqlite3.connect(state_db) as con:
            rows = con.execute(
                'SELECT value FROM ItemTable WHERE key = "history.recentlyOpenedPathsList"'
            )
            for row in rows:
                data = json.loads(row[0])
                for sort_index, entry in enumerate(data.get("entries", [])):
                    path = recent_entry_path(entry)
                    if path is None or not path.exists():
                        continue
                    projects.append(
                        Project(
                            name=project_name(path),
                            path=str(path),
                            source="recent",
                            priority=20,
                            sort_index=sort_index,
                        )
                    )
    except (OSError, sqlite3.Error, json.JSONDecodeError):
        return []

    return projects


def storage_json_projects() -> list[Project]:
    storage = Path.home() / ".config/Code/User/globalStorage/storage.json"
    if not storage.exists():
        return []

    try:
        data = json.loads(storage.read_text())
    except (OSError, json.JSONDecodeError):
        return []

    projects = []
    seen = set()
    for sort_index, uri in enumerate(storage_project_uris(data)):
        path = uri_to_path(uri)
        if path is None or not path.exists():
            continue
        resolved = str(path.resolve())
        if resolved in seen:
            continue
        seen.add(resolved)
        projects.append(
            Project(
                name=project_name(path),
                path=str(path),
                source="storage",
                priority=12,
                sort_index=sort_index,
            )
        )
    return projects


def storage_project_uris(data: dict) -> list[str]:
    uris = []

    backup = data.get("backupWorkspaces", {})
    for item in backup.get("folders", []):
        if isinstance(item, dict) and item.get("folderUri"):
            uris.append(item["folderUri"])
    for item in backup.get("workspaces", []):
        if isinstance(item, dict) and item.get("workspaceUri"):
            uris.append(item["workspaceUri"])

    associations = data.get("profileAssociations", {})
    workspaces = associations.get("workspaces", {})
    if isinstance(workspaces, dict):
        uris.extend(workspaces.keys())

    windows = data.get("windowsState", {})
    last_active = windows.get("lastActiveWindow", {})
    if isinstance(last_active, dict):
        for key in ("folder", "workspace"):
            if last_active.get(key):
                uris.append(last_active[key])
    for window in windows.get("openedWindows", []):
        if isinstance(window, dict):
            for key in ("folder", "workspace"):
                if window.get(key):
                    uris.append(window[key])

    return uris


def git_repository_projects() -> list[Project]:
    state_db = Path.home() / ".config/Code/User/globalStorage/state.vscdb"
    if not state_db.exists():
        return []

    try:
        with sqlite3.connect(state_db) as con:
            row = con.execute('SELECT value FROM ItemTable WHERE key = "vscode.git"').fetchone()
    except (OSError, sqlite3.Error):
        return []

    if row is None:
        return []

    try:
        data = json.loads(row[0])
    except json.JSONDecodeError:
        return []

    projects = []
    cache = data.get("git.repositoryCache", [])
    for remote_index, remote_entry in enumerate(cache):
        if not isinstance(remote_entry, list) or len(remote_entry) < 2:
            continue
        repositories = remote_entry[1]
        if not isinstance(repositories, list):
            continue
        for repo_index, repo_entry in enumerate(repositories):
            if not isinstance(repo_entry, list) or len(repo_entry) < 2:
                continue
            metadata = repo_entry[1]
            if not isinstance(metadata, dict):
                continue
            path = Path(metadata.get("workspacePath") or metadata.get("repositoryPath") or repo_entry[0])
            if not path.exists():
                continue
            projects.append(
                Project(
                    name=project_name(path),
                    path=str(path),
                    source="git",
                    priority=18,
                    sort_index=remote_index * 1000 + repo_index,
                )
            )
    return projects


def recent_entry_path(entry: dict) -> Path | None:
    uri = None
    if "folderUri" in entry:
        uri = entry["folderUri"]
    elif "workspace" in entry and "configPath" in entry["workspace"]:
        uri = entry["workspace"]["configPath"]
    elif "fileUri" in entry:
        uri = entry["fileUri"]

    if uri is None:
        return None

    return uri_to_path(uri)


def uri_to_path(uri: str) -> Path | None:
    parsed = urlparse(uri)
    if parsed.scheme != "file":
        return None

    return Path(unquote(parsed.path))


def project_name(path: Path) -> str:
    name = path.name
    if name.endswith(".code-workspace"):
        return name[:-15] + " (Workspace)"
    return name


def project_manager_projects() -> list[Project]:
    projects_path = (
        Path.home()
        / ".config/Code/User/globalStorage/alefragnani.project-manager/projects.json"
    )
    if not projects_path.exists():
        return []

    try:
        raw_projects = json.loads(projects_path.read_text())
    except (OSError, json.JSONDecodeError):
        return []

    projects = []
    for sort_index, item in enumerate(raw_projects):
        if not item.get("enabled", True):
            continue
        root = item.get("rootPath")
        name = item.get("name") or (Path(root).name if root else "")
        if not root or not name or not Path(root).exists():
            continue
        projects.append(
            Project(
                name=name,
                path=root,
                source="project-manager",
                priority=10,
                sort_index=sort_index,
            )
        )
    return projects


def result_for_project(project: Project) -> dict:
    return {
        "title": project.name,
        "subtitle": f"VSCode {project.source} - {project.path}",
        "icon": vscode_icon_name(),
        "action": {
            "type": "launch-command",
            "command": "code",
            "args": ["-r", project.path],
        },
        "buttons": [
            {
                "title": "打开项目根目录",
                "icon": "folder-open-symbolic",
                "action": {
                    "type": "open-uri",
                    "uri": Path(project.path).resolve().as_uri(),
                },
            }
        ],
    }


def vscode_icon_name() -> str:
    for path in desktop_file_paths():
        try:
            for line in path.read_text().splitlines():
                if line.startswith("Icon="):
                    icon = line.removeprefix("Icon=").strip()
                    if icon:
                        return icon
        except OSError:
            continue
    return "vscode"


def desktop_file_paths() -> list[Path]:
    paths = []
    data_home = os.environ.get("XDG_DATA_HOME")
    if data_home:
        paths.append(Path(data_home) / "applications/code.desktop")
    else:
        paths.append(Path.home() / ".local/share/applications/code.desktop")

    data_dirs = os.environ.get("XDG_DATA_DIRS", "/usr/local/share:/usr/share")
    paths.extend(Path(path) / "applications/code.desktop" for path in data_dirs.split(":"))
    return paths


if __name__ == "__main__":
    raise SystemExit(main())
