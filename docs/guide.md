# Usage & configuration

Full reference for cc-herdr-controller. See the [README](../README.md) for
install and quick start, and [troubleshooting](troubleshooting.md) for macOS
permissions and known caveats.

## How it works

```
Controller (USB/Bluetooth)
        │  SDL / pygame
        ▼
controller_daemon.py ──► herdr CLI ──► herdr socket ──► herdr server
                                       (tabs / workspaces / panes / keys / text)
```

**Everything goes through herdr's socket** — navigation, scroll (as mouse-wheel
escape sequences via `send-text`), and voice (streamed spaces). Nothing uses
OS-level input injection, so it all works with the Mac **headless / over SSH**
and needs no Accessibility permission. The daemon just needs to read the
controller (Input Monitoring) on whichever machine the controller is plugged in.

You bind **friendly control names** (`A`, `ZL`, `dpad_up`, `right_y`) to actions.
A per-controller `profile` translates those names to raw SDL indices, so the
config stays readable and `--calibrate` keeps the indices correct for your unit.

> Only one process can own a controller at a time. Stop any other instance
> (or your old daemon) before running this one, or you'll get
> `Couldn't setup USB mode` / `No controller detected`.

The daemon must run on the **same machine as herdr** (it reads
`HERDR_SOCKET_PATH`), with the controller plugged into that machine.

## Modes

```bash
.venv/bin/python controller_daemon.py --list        # detected controllers
.venv/bin/python controller_daemon.py --calibrate    # press each control -> builds the profile
.venv/bin/python controller_daemon.py --discover      # raw index + the name it maps to
.venv/bin/python controller_daemon.py --monitor       # show input -> action, do NOTHING
.venv/bin/python controller_daemon.py --monitor --perform   # show AND drive herdr
.venv/bin/python controller_daemon.py                 # run (drives herdr)
.venv/bin/python controller_daemon.py --dry-run       # run, but perform nothing
```

Background control:

```bash
.venv/bin/python controller_daemon.py --bg      # start detached (logs to .daemon.log)
.venv/bin/python controller_daemon.py --status  # is it running?
.venv/bin/python controller_daemon.py --stop    # stop the background daemon
```

`--monitor` is "watch what it would do"; add `--perform` to let it act. Both
`--discover` and the run loop print a heartbeat so a silent screen tells you
whether input is arriving at all.

## Calibrate (recommended, one time per controller)

Raw indices vary by OS/driver, so let the tool learn yours:

```bash
.venv/bin/python controller_daemon.py --calibrate
```

Press each control as prompted (Enter to skip one you don't have). It writes the
`profile` in `mapping.json`. Buttons are stored as buttons, analog triggers
(ZL/ZR) as axes, and the D-pad is handled automatically whether it reports as
buttons or a hat.

## Binding actions

Edit only the `bindings` block in `mapping.json` — friendly name → action:

```json
"bindings": {
  "A": "enter",
  "ZL": "tab_prev",
  "ZR": "tab_next",
  "Y": "voice",
  "dpad_up": "pane_up"
}
```

| Action | Effect |
|---|---|
| `tab_next` / `tab_prev` | focus next/prev tab in current workspace |
| `workspace_next` / `workspace_prev` | focus next/prev workspace |
| `pane_left/right/up/down` | move pane focus directionally |
| `pane_zoom` | toggle pane fullscreen |
| `scroll_up` / `scroll_down` | scroll the focused program (mouse-wheel)¹ |
| `enter` / `escape` | send Enter/Esc to focused pane |
| `voice` | hold-space for Claude's voice mode (see Voice) |
| `dictation` | run `settings.dictation_command` (OS dictation) |
| `noop` | ignore |

¹ Scrolling sends SGR mouse-wheel escape sequences through `send-text`, so it
works over the socket (Mac headless). It scrolls the **focused program** and
only when that program has mouse-tracking on (Claude Code, `less --mouse`,
`htop`, `lazygit`, …). It does **not** scroll herdr's own scrollback buffer or a
bare shell — see Scrolling below.

### Default bindings

| Control | Action |
|---|---|
| A / B | enter / escape |
| X | pane zoom |
| Y | voice (hold-space) |
| L / R bumpers | workspace prev / next |
| ZL / ZR triggers | tab prev / next |
| D-pad | pane focus (directional) |
| Left stick | arrow keys (4-way, repeats while held) |
| Right stick ↕ | scroll (analog, accelerates) |

The left stick sends `up/down/left/right` to the focused pane (4-way, dominant
axis, key-repeat while held) — configured under `settings.arrows`. Remove that
block to disable.

## Scrolling

`settings.scroll` controls the analog right-stick scroll:

```json
"scroll": { "axis": "right_y", "invert": false, "deadzone": 0.18, "tick_ms": 30, "max_lines": 6 }
```

The further you push, the more wheel notches per tick (acceleration), up to
`max_lines`. Flip `invert` if up/down feel backwards.

**Why mouse-wheel and not PageUp?** herdr rejects `pageup`/`pagedown` over the
socket and exposes no scrollback-scroll command. A mouse wheel, though, is just
an escape sequence on the program's input, which `send-text` delivers over the
socket. The trade-off: it drives the focused **program's** scroll (e.g. Claude's
transcript), not herdr's multiplexer scrollback. True scrollback scrolling would
need a herdr `pane scroll` socket command.

## Voice

The Switch Pro Controller has **no microphone** — voice input uses your machine's
mic. Two approaches, pick per button:

- **`voice`** (default on Y) — emulates Claude Code's **hold-space** by streaming
  spaces to the focused pane while active. `settings.voice`:

  ```json
  "voice": { "mode": "both", "tap_max_ms": 300, "repeat_ms": 90, "char": " " }
  ```

  `mode`: `both` (quick **tap** = toggle on/off, **hold** = momentary),
  `hold` (momentary only), or `toggle` (tap on/off only).

- **`dictation`** — fires an OS dictation hotkey via `settings.dictation_command`
  (macOS `shortcuts run`/`osascript`; Windows `Win+H`). For OS-level dictation
  rather than Claude's in-app voice.
