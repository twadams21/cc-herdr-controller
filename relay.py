#!/usr/bin/env python3
"""herdr relay — runs ON THE MAC, next to the herdr server.

The Rust controller daemon (on the Windows PC, where the controller is paired)
streams compact, newline-delimited *intents* into this process's stdin over a
single long-lived SSH connection:

    ssh <mac> 'cd ~/cc-herdr-controller && python3 relay.py'

Each intent is dispatched to the local `herdr` CLI via the existing herdr.py
helpers. Keeping the dispatch here means the chatty list -> compute-neighbour ->
focus round-trips all hit herdr's *local* socket; only the tiny intent crosses
the network. One newline per intent; the daemon never waits for a reply, so the
hot path is a single network hop.

Protocol (one per line; tokens are whitespace-separated):
    tab_next | tab_prev
    workspace_next | workspace_prev
    pane <left|right|up|down>
    zoom
    scroll <signed-int>          # +N = up (older), -N = down, in wheel notches
    keys <name...>               # e.g. `keys enter`, `keys esc`, `keys up`
    text <base64>                # base64 of literal text (used for voice spaces)
    ping                         # health check / keepalive (no-op)

Stdlib only. Errors are logged to stderr (which surfaces back through SSH to the
daemon's log) and never kill the loop.
"""

from __future__ import annotations

import base64
import sys

import herdr


def _dispatch(line: str) -> None:
    parts = line.split()
    if not parts:
        return
    cmd, args = parts[0], parts[1:]

    if cmd == "ping":
        return
    elif cmd == "tab_next":
        herdr.tab_step(+1)
    elif cmd == "tab_prev":
        herdr.tab_step(-1)
    elif cmd == "workspace_next":
        herdr.workspace_step(+1)
    elif cmd == "workspace_prev":
        herdr.workspace_step(-1)
    elif cmd == "pane" and args:
        herdr.pane_focus(args[0])
    elif cmd == "zoom":
        herdr.pane_zoom_toggle()
    elif cmd == "scroll" and args:
        herdr.scroll(int(args[0]))
    elif cmd == "keys" and args:
        herdr.send_keys(*args)
    elif cmd == "text" and args:
        herdr.send_text(base64.b64decode(args[0]).decode("utf-8", "replace"))
    else:
        print(f"relay: ignoring unknown intent: {line!r}", file=sys.stderr, flush=True)


def main() -> None:
    print("relay: ready", file=sys.stderr, flush=True)
    for raw in sys.stdin:
        line = raw.lstrip("﻿").strip()  # tolerate a stray leading BOM
        if not line:
            continue
        try:
            _dispatch(line)
        except herdr.HerdrError as exc:
            print(f"relay: {line!r}: {exc}", file=sys.stderr, flush=True)
        except Exception as exc:  # never let one bad intent kill the relay
            print(f"relay: {line!r}: unexpected {exc!r}", file=sys.stderr, flush=True)
    print("relay: stdin closed, exiting", file=sys.stderr, flush=True)


if __name__ == "__main__":
    main()
