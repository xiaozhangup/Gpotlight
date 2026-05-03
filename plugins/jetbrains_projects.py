#!/usr/bin/env python3
from __future__ import annotations

import json
import os
import shlex
import sys
from dataclasses import dataclass
from pathlib import Path
from shutil import which
from xml.etree import ElementTree


@dataclass
class Ide:
    name: str
    config_prefixes: list[str]
    binaries: list[str]

    @property
    def binary(self) -> str | None:
        for binary in self.binaries:
            found = which(binary)
            if found:
                return found
        return None


@dataclass
class Launcher:
    command: str
    args: list[str]
    icon: str


@dataclass
class Project:
    name: str
    path: str
    last_opened: int
    ide_name: str
    command: str
    args: list[str]
    icon: str


IDES = [
    Ide("Android Studio", ["Google/AndroidStudio"], ["studio", "androidstudio", "android-studio"]),
    Ide("CLion", ["JetBrains/CLion"], ["clion", "clion-eap"]),
    Ide("DataGrip", ["JetBrains/DataGrip"], ["datagrip", "datagrip-eap"]),
    Ide("DataSpell", ["JetBrains/DataSpell"], ["dataspell", "dataspell-eap"]),
    Ide("GoLand", ["JetBrains/GoLand"], ["goland", "goland-eap"]),
    Ide(
        "IntelliJ IDEA",
        ["JetBrains/IntelliJIdea", "JetBrains/Idea"],
        ["idea", "idea.sh", "idea-ultimate", "intellij-idea-ce", "intellij-idea-ultimate-edition"],
    ),
    Ide("PhpStorm", ["JetBrains/PhpStorm"], ["phpstorm", "phpstorm-eap"]),
    Ide("PyCharm", ["JetBrains/PyCharm"], ["charm", "pycharm", "pycharm-eap", "pycharm-professional"]),
    Ide("Rider", ["JetBrains/Rider"], ["rider", "rider-eap"]),
    Ide("RubyMine", ["JetBrains/RubyMine"], ["rubymine", "rubymine-eap"]),
    Ide("RustRover", ["JetBrains/RustRover"], ["rustrover", "rustrover-eap"]),
    Ide("WebStorm", ["JetBrains/WebStorm"], ["webstorm", "webstorm-eap"]),
    Ide("Writerside", ["JetBrains/Writerside"], ["writerside", "writerside-eap"]),
]


def main() -> int:
    query = sys.argv[1].strip().lower() if len(sys.argv) > 1 else ""
    config = load_config()
    match_path = bool(config.get("match_path", False))

    matches = [
        project
        for project in list_projects()
        if matches_query(project, query, match_path)
    ]
    matches.sort(key=lambda item: item.last_opened, reverse=True)

    for project in matches[:20]:
        print(json.dumps(result_for_project(project), ensure_ascii=False))

    return 0


def load_config() -> dict:
    try:
        return json.loads(os.environ.get("GPOTLIGHT_PLUGIN_CONFIG", "{}"))
    except json.JSONDecodeError:
        return {}


def list_projects() -> list[Project]:
    projects = []
    for ide in IDES:
        launcher = ide_launcher(ide)
        for config_dir in ide_config_dirs(ide):
            projects.extend(parse_recent_projects(ide, launcher, config_dir))
    return projects


def ide_launcher(ide: Ide) -> Launcher:
    desktop_launcher = launcher_from_desktop_file(ide)
    if desktop_launcher:
        return desktop_launcher

    for binary in ide.binaries:
        script = Path.home() / ".local/share/JetBrains/Toolbox/scripts" / binary
        if script.exists():
            return Launcher(str(script), [], default_icon(ide))

    binary = ide.binary
    if binary:
        return Launcher(binary, [], default_icon(ide))

    return Launcher(ide.binaries[0], [], default_icon(ide))


def launcher_from_desktop_file(ide: Ide) -> Launcher | None:
    for desktop_file in desktop_files():
        values = read_desktop_entry(desktop_file)
        exec_value = values.get("Exec", "")
        if not desktop_matches_ide(ide, values, exec_value):
            continue
        command = parse_exec(exec_value)
        if not command:
            continue
        return Launcher(command[0], command[1:], values.get("Icon", default_icon(ide)))
    return None


def desktop_files() -> list[Path]:
    data_dirs = [Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share"))]
    data_dirs.extend(Path(path) for path in os.environ.get("XDG_DATA_DIRS", "/usr/local/share:/usr/share").split(":") if path)

    files = []
    for base in data_dirs:
        files.extend((base / "applications").glob("jetbrains-*.desktop"))
    return sorted(files)


def read_desktop_entry(path: Path) -> dict[str, str]:
    values = {}
    try:
        for line in path.read_text(encoding="utf-8").splitlines():
            if "=" not in line or line.startswith("#"):
                continue
            key, value = line.split("=", 1)
            values[key] = value
    except OSError:
        pass
    return values


def desktop_matches_ide(ide: Ide, values: dict[str, str], exec_value: str) -> bool:
    haystack = " ".join([values.get("Name", ""), values.get("StartupWMClass", ""), exec_value]).lower()
    if ide.name == "IntelliJ IDEA":
        return "intellij" in haystack or "idea" in haystack
    return any(alias.lower().replace(".sh", "") in haystack for alias in ide.binaries)


def parse_exec(exec_value: str) -> list[str]:
    try:
        parts = shlex.split(exec_value)
    except ValueError:
        return []
    return [part for part in parts if not part.startswith("%")]


def default_icon(ide: Ide) -> str:
    return {
        "Android Studio": "android-studio",
        "CLion": "clion",
        "DataGrip": "datagrip",
        "DataSpell": "dataspell",
        "GoLand": "goland",
        "IntelliJ IDEA": "idea",
        "PhpStorm": "phpstorm",
        "PyCharm": "pycharm",
        "Rider": "rider",
        "RubyMine": "rubymine",
        "RustRover": "rustrover",
        "WebStorm": "webstorm",
        "Writerside": "writerside",
    }.get(ide.name, "applications-development-symbolic")


def ide_config_dirs(ide: Ide) -> list[Path]:
    base = Path.home() / ".config"
    dirs = []
    for prefix in ide.config_prefixes:
        dirs.extend(base.glob(f"{prefix}*/"))
    return sorted(dirs, reverse=True)[:1]


def parse_recent_projects(ide: Ide, launcher: Launcher, config_dir: Path) -> list[Project]:
    if ide.name == "Rider":
        file_name = "recentSolutions.xml"
        component = "RiderRecentProjectsManager"
    else:
        file_name = "recentProjects.xml"
        component = "RecentProjectsManager"

    recent_file = config_dir / "options" / file_name
    try:
        root = ElementTree.parse(recent_file).getroot()
    except (ElementTree.ParseError, FileNotFoundError, OSError):
        return []

    projects = []
    for entry in root.findall(f".//component[@name='{component}']//entry[@key]"):
        path = entry.attrib.get("key", "").replace("$USER_HOME$", str(Path.home()))
        opened = entry.find(".//option[@name='projectOpenTimestamp']")
        last_opened = int(opened.attrib.get("value", "0")) if opened is not None else 0
        if not path or not Path(path).exists():
            continue
        projects.append(
            Project(
                name=Path(path).name,
                path=path,
                last_opened=last_opened,
                ide_name=ide.name,
                command=launcher.command,
                args=launcher.args,
                icon=launcher.icon,
            )
        )
    return projects


def matches_query(project: Project, query: str, match_path: bool) -> bool:
    if not query:
        return True
    if query in project.name.lower():
        return True
    return match_path and query in project.path.lower()


def result_for_project(project: Project) -> dict:
    return {
        "title": project.name,
        "subtitle": f"{project.ide_name} - {project.path}",
        "icon": project.icon,
        "action": {
            "type": "launch-command",
            "command": project.command,
            "args": [*project.args, project.path],
        },
    }


if __name__ == "__main__":
    raise SystemExit(main())
