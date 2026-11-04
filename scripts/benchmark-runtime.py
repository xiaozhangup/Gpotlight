#!/usr/bin/env python3
"""Compare headless idle resources and visible-window toggle latency.

Run from the repository root after building both binaries:
  python3 scripts/benchmark-runtime.py target/baseline-gpotlight target/release/gpotlight
Requires Xvfb, dbus-run-session and xdotool. Optional --trace records poll calls.
All application config/runtime files and session buses are isolated.
"""

import argparse
import json
import os
from pathlib import Path
import signal
import socket
import statistics
import subprocess
import sys
import tempfile
import time


def snapshot(pid):
    stat = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
    status = dict(
        line.split(":", 1)
        for line in Path(f"/proc/{pid}/status").read_text().splitlines()
        if ":" in line
    )
    return {
        "cpu_ticks": int(stat[11]) + int(stat[12]),
        "rss_kib": int(status["VmRSS"].split()[0]),
        "threads": int(status["Threads"]),
        "main_voluntary_switches": int(status["voluntary_ctxt_switches"]),
        "main_involuntary_switches": int(status["nonvoluntary_ctxt_switches"]),
    }


def send_command(path, command):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.connect(str(path))
        client.sendall(command)


def visible(env):
    result = subprocess.run(
        ["xdotool", "search", "--onlyvisible", "--name", "Gpotlight"],
        env=env, capture_output=True,
    )
    return result.returncode == 0


def wait_visible(env, expected, timeout=3):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if visible(env) == expected:
            return
        time.sleep(0.001)
    raise RuntimeError(f"Window did not become {'visible' if expected else 'hidden'}")


def child(args):
    binary = Path(args.binaries[0]).resolve()
    repository = Path(__file__).resolve().parent.parent
    log_path = repository / "target" / f"runtime-{binary.name}.log"
    trace_path = repository / "target" / f"runtime-{binary.name}.strace"
    with tempfile.TemporaryDirectory(prefix="gpotlight-bench-") as temp:
        temp = Path(temp)
        runtime = temp / "runtime"
        runtime.mkdir(mode=0o700)
        xvfb = subprocess.Popen(
            ["Xvfb", "-displayfd", "1", "-screen", "0", "1280x800x24", "-nolisten", "tcp"],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
        )
        app = None
        try:
            display = xvfb.stdout.readline().strip()
            if not display:
                raise RuntimeError("Xvfb could not allocate a display")
            env = dict(os.environ, DISPLAY=f":{display}", GDK_BACKEND="x11",
                       GSK_RENDERER="cairo", NO_AT_BRIDGE="1",
                       XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(temp / "config"))
            command = [str(binary)]
            if args.trace:
                command = ["strace", "-f", "-tt", "-e",
                           "trace=poll,ppoll,epoll_wait,epoll_pwait,epoll_pwait2",
                           "-o", str(trace_path)] + command
            with log_path.open("w") as log:
                started = time.monotonic()
                app = subprocess.Popen(command, env=env, stdout=log, stderr=log,
                                       start_new_session=True, cwd=repository)
                socket_path = runtime / "gpotlight.sock"
                deadline = started + 20
                while not socket_path.exists():
                    if app.poll() is not None or time.monotonic() > deadline:
                        raise RuntimeError(f"Startup failed; see {log_path}")
                    time.sleep(0.005)
                startup_ms = (time.monotonic() - started) * 1000
                pid = app.pid
                if args.trace:
                    children = Path(f"/proc/{pid}/task/{pid}/children").read_text().split()
                    pid = int(children[0])
                time.sleep(args.warmup)
                before = snapshot(pid)
                sample_started = time.monotonic()
                time.sleep(args.seconds)
                elapsed = time.monotonic() - sample_started
                after = snapshot(pid)
                latency = []
                for _ in range(args.toggles):
                    started = time.monotonic()
                    send_command(socket_path, b"toggle\n")
                    wait_visible(env, True)
                    latency.append((time.monotonic() - started) * 1000)
                    time.sleep(0.1)
                    send_command(socket_path, b"toggle\n")
                    wait_visible(env, False)
                    time.sleep(0.073)
                results = {
                    "binary": str(binary),
                    "renderer": "Xvfb/X11/cairo (software rendering)",
                    "startup_to_socket_ms": round(startup_ms, 2),
                    "sample_seconds": round(elapsed, 3),
                    "cpu_percent_one_core": round(
                        100 * (after["cpu_ticks"] - before["cpu_ticks"])
                        / os.sysconf("SC_CLK_TCK") / elapsed, 3),
                    "main_thread_wakeups_per_second": round(
                        (after["main_voluntary_switches"] - before["main_voluntary_switches"])
                        / elapsed, 3),
                    "rss_kib": after["rss_kib"],
                    "threads": after["threads"],
                    "toggle_visible_ms": [round(value, 2) for value in latency],
                    "toggle_visible_median_ms": round(statistics.median(latency), 2)
                    if latency else None,
                    "log": str(log_path),
                }
                print(json.dumps(results, ensure_ascii=False, indent=2), flush=True)
        finally:
            if app and app.poll() is None:
                os.killpg(app.pid, signal.SIGTERM)
                try:
                    app.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    os.killpg(app.pid, signal.SIGKILL)
                    app.wait()
            xvfb.terminate()
            xvfb.wait(timeout=3)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binaries", nargs="+")
    parser.add_argument("--child", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--seconds", type=float, default=10)
    parser.add_argument("--warmup", type=float, default=3)
    parser.add_argument("--toggles", type=int, default=12)
    parser.add_argument("--trace", action="store_true")
    args = parser.parse_args()
    if args.child:
        child(args)
        return
    for binary in args.binaries:
        command = ["dbus-run-session", "--", sys.executable, str(Path(__file__).resolve()),
                   "--child", binary, "--seconds", str(args.seconds), "--warmup",
                   str(args.warmup), "--toggles", str(args.toggles)]
        if args.trace:
            command.append("--trace")
        subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
