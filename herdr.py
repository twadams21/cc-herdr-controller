"""Thin wrapper around the `herdr` CLI.

All herdr socket commands return JSON of the shape:
    {"id": "...", "result": {"type": "...", "<collection>": [...]}}

This module shells out to `herdr`, parses that JSON, and exposes the handful
of actions the controller daemon needs. herdr has no native "next tab" /
"next workspace" command, so we list + compute the neighbour + focus.
"""

from __future__ import annotations

import json
import shutil
import subprocess
from typing import Any, Optional

HERDR_BIN = shutil.which("herdr") or "herdr"


class HerdrError(RuntimeError):
    pass


def _run(*args: str) -> dict[str, Any]:
    """Run `herdr <args>` and return the parsed `result` object."""
    try:
        proc = subprocess.run(
            [HERDR_BIN, *args],
            capture_output=True,
            text=True,
            timeout=5,
        )
    except FileNotFoundError as exc:
        raise HerdrError(f"herdr binary not found: {HERDR_BIN}") from exc
    except subprocess.TimeoutExpired as exc:
        raise HerdrError(f"herdr {' '.join(args)} timed out") from exc

    if proc.returncode != 0:
        raise HerdrError(
            f"herdr {' '.join(args)} exited {proc.returncode}: {proc.stderr.strip()}"
        )

    out = proc.stdout.strip()
    if not out:
        return {}
    try:
        payload = json.loads(out)
    except json.JSONDecodeError:
        # Some commands print plain text; pass it through.
        return {"_text": out}
    return payload.get("result", payload)


# ---- queries ---------------------------------------------------------------

def list_workspaces() -> list[dict[str, Any]]:
    return _run("workspace", "list").get("workspaces", [])


def list_tabs() -> list[dict[str, Any]]:
    return _run("tab", "list").get("tabs", [])


def list_panes() -> list[dict[str, Any]]:
    return _run("pane", "list").get("panes", [])


def focused_pane() -> Optional[dict[str, Any]]:
    for pane in list_panes():
        if pane.get("focused"):
            return pane
    return None


def focused_workspace_id() -> Optional[str]:
    for ws in list_workspaces():
        if ws.get("focused"):
            return ws.get("workspace_id")
    return None


# ---- navigation actions ----------------------------------------------------

def _neighbor(items: list[dict[str, Any]], id_key: str, step: int) -> Optional[str]:
    """Return the id of the item `step` positions from the focused one (wraps)."""
    if not items:
        return None
    focused_idx = next(
        (i for i, it in enumerate(items) if it.get("focused")), None
    )
    if focused_idx is None:
        return items[0].get(id_key)
    return items[(focused_idx + step) % len(items)].get(id_key)


def tab_step(step: int) -> None:
    """Focus the next (step=+1) / previous (step=-1) tab in the current workspace."""
    ws_id = focused_workspace_id()
    tabs = [t for t in list_tabs() if t.get("workspace_id") == ws_id]
    tabs.sort(key=lambda t: t.get("number", 0))
    target = _neighbor(tabs, "tab_id", step)
    if target:
        _run("tab", "focus", target)


def workspace_step(step: int) -> None:
    """Focus the next / previous workspace by number (wraps)."""
    workspaces = sorted(list_workspaces(), key=lambda w: w.get("number", 0))
    target = _neighbor(workspaces, "workspace_id", step)
    if target:
        _run("workspace", "focus", target)


def pane_focus(direction: str) -> None:
    """Move pane focus left|right|up|down within the current workspace."""
    pane = focused_pane()
    args = ["pane", "focus", "--direction", direction]
    if pane:
        args += ["--pane", pane["pane_id"]]
    else:
        args += ["--current"]
    _run(*args)


def pane_zoom_toggle() -> None:
    pane = focused_pane()
    args = ["pane", "zoom", "--toggle"]
    if pane:
        args += ["--pane", pane["pane_id"]]
    else:
        args += ["--current"]
    _run(*args)


def send_keys(*keys: str) -> None:
    """Send key name(s) to the focused pane (e.g. 'pageup', 'enter', 'esc')."""
    pane = focused_pane()
    if not pane:
        return
    _run("pane", "send-keys", pane["pane_id"], *keys)


def send_text(text: str) -> None:
    pane = focused_pane()
    if not pane:
        return
    _run("pane", "send-text", pane["pane_id"], text)


# SGR mouse-wheel escape sequences. A mouse-tracking program (Claude Code,
# less --mouse, htop, ...) reads these as wheel notches. Goes over the socket,
# so it works with the Mac headless. Does NOT drive herdr's own scrollback.
_WHEEL_UP = "\x1b[<64;1;1M"
_WHEEL_DOWN = "\x1b[<65;1;1M"


def scroll(lines: int) -> None:
    """Scroll the focused pane's program by `lines` wheel notches.

    Positive = up (older content), negative = down. Sends all notches in one
    send-text call to keep it to a single socket round-trip.
    """
    if lines == 0:
        return
    pane = focused_pane()
    if not pane:
        return
    seq = _WHEEL_UP if lines > 0 else _WHEEL_DOWN
    _run("pane", "send-text", pane["pane_id"], seq * abs(lines))
