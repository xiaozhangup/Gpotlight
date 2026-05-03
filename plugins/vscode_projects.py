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
        for project in recent_projects():
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
                    name = path.name
                    if name.endswith(".code-workspace"):
                        name = name[:-15] + " (Workspace)"
                    projects.append(
                        Project(
                            name=name,
                            path=str(path),
                            source="recent",
                            priority=20,
                            sort_index=sort_index,
                        )
                    )
    except (OSError, sqlite3.Error, json.JSONDecodeError):
        return []

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

    parsed = urlparse(uri)
    if parsed.scheme != "file":
        return None

    return Path(unquote(parsed.path))


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
        "icon": "com.visualstudio.code",
        "action": {
            "type": "launch-command",
            "command": "code",
            "args": ["-r", project.path],
        },
    }


if __name__ == "__main__":
    raise SystemExit(main())
