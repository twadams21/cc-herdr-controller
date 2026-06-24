# Usage & configuration

Full reference for cc-controller. See the [README](../README.md) for install and
quick start, and [troubleshooting](troubleshooting.md) for macOS permissions and
known caveats.

## How it works

```
Controller (USB/Bluetooth)
        │  SDL2
        ▼
cc-controller ──► dispatch ──► herdr / tmux CLI ──► multiplexer socket
   (reads pad,                  (tabs/windows · workspaces/sessions · panes · keys · text)
    maps to intents)
```

You bind **friendly control names** (`A`, `ZL`, `dpad_up`, `right_y`) to abstract
**actions** (`tab_next`, `pane_left`, …). A per-controller `profile` translates
names to raw SDL indices, so `mapping.json` stays readable and `calibrate` keeps
the indices right for your unit. Each action becomes a compact **intent** that
the chosen backend turns into herdr/tmux CLI calls against the multiplexer's
local socket — **no OS-level input injection**, so it all works with the host
headless / over SSH.

Three modes, run from one binary:

- **`local`** — controller and multiplexer on the same machine. Intents dispatch
  in-process; no SSH, no network.
- **`remote`** — controller here, multiplexer on another machine. Intents stream
  over one persistent SSH connection to `cc-controller host` on the far end,
  which runs the chatty herdr/tmux work against the *local* socket. See
  [remote setup](rust-remote.md).
- **`host`** — the dispatcher `remote` spawns over SSH. Reads intents on stdin
  and drives the local multiplexer. You rarely run it by hand (handy for
  debugging: `echo 'tab_next' | cc-controller host --backend tmux`).

> Only one process can own a controller at a time. Stop any other instance
> before running this one, or you'll get `No controller detected`.

## Commands

```bash
cc-controller local   [run|start|stop|status] [--dry-run] [--backend herdr|tmux]
cc-controller remote  [run|start|stop|status] [--dry-run] [--host user@mac] [--backend …]
cc-controller host    [--backend herdr|tmux]      # usually spawned over SSH by `remote`

cc-controller list                                 # detected controllers
cc-controller discover                             # raw input + the name each maps to
cc-controller calibrate                            # press each control -> build the profile

cc-controller config show | get <path> | set <path> <value> | bind | edit | path

cc-controller --config <path> …                    # use a specific mapping.json (global)
```

The action defaults to `run` (foreground). `--dry-run` performs nothing — it
prints the intents each input would send, so you can test a mapping without a
multiplexer. `--tmux` is shorthand for `--backend tmux`.

## Backends: herdr or tmux

The same bindings drive **either** multiplexer. Actions are abstract, so each
backend just reinterprets them through its own vocabulary — you don't re-bind
anything:

| Abstract action | herdr | tmux |
|---|---|---|
| `tab_next` / `tab_prev` (ZL/ZR) | next/prev tab | `next-window` / `previous-window` |
| `workspace_next` / `workspace_prev` (L/R) | next/prev workspace | `switch-client -n` / `-p` (session) |
| `pane_left/right/up/down` (D-pad) | pane focus | `select-pane -L/-R/-U/-D` |
| `pane_zoom` (X) | toggle zoom | `resize-pane -Z` |
| `enter` / `escape`, arrows, voice | `send-keys` / `send-text` | `send-keys` (literal for text) |
| scroll (right stick) | SGR wheel via `send-text` | SGR wheel via `send-keys -l` |

Pick the backend with `--backend tmux` / `--tmux` per run, or make it the default
with `cc-controller config set backend tmux`. For `remote`, the controller
forwards the resolved backend to `cc-controller host`, so set it on the
controller side.

> tmux window/session/pane commands act on the **current** target. With one
> attached client that's the session and pane you're looking at; with several,
> tmux acts on the most recently active one. `workspace_*` (session switch) needs
> an attached client — headless it's a no-op.

## Calibrate (recommended, one time per controller)

Raw SDL indices vary by OS/driver, so let the tool learn yours:

```bash
cc-controller calibrate
```

Press each control as prompted (Enter to skip one you don't have, `q`+Enter to
stop). It writes the `profile` in `mapping.json`. Buttons are stored as buttons,
analog triggers (ZL/ZR) as axes, and the D-pad is handled automatically whether
it reports as buttons or a hat.

## Binding actions

Bind interactively:

```bash
cc-controller config bind     # pick a control, then pick an action
```

…or set one directly, or hand-edit the `bindings` block in `mapping.json`
(friendly name → action):

```bash
cc-controller config set bindings.A enter
```

| Action | Effect |
|---|---|
| `tab_next` / `tab_prev` | next/prev tab (herdr) · window (tmux) |
| `workspace_next` / `workspace_prev` | next/prev workspace (herdr) · session (tmux) |
| `pane_left/right/up/down` | move pane focus directionally |
| `pane_zoom` | toggle pane fullscreen |
| `scroll_up` / `scroll_down` | scroll the focused program (mouse-wheel)¹ |
| `enter` / `escape` | send Enter/Esc to the focused pane |
| `voice` | hold-space for Claude's voice mode (see Voice) |
| `dictation` | run `settings.dictation_command` (OS dictation) |
| `noop` | ignore |

¹ Scrolling sends SGR mouse-wheel escape sequences, so it works over the socket
(host headless). It scrolls the **focused program** and only when that program
has mouse-tracking on (Claude Code, `less --mouse`, `htop`, `lazygit`, …). It
does **not** scroll the multiplexer's own scrollback — see Scrolling below.

### Default bindings

| Control | Action |
|---|---|
| A / B | enter / escape |
| X | pane zoom |
| Y | voice (hold-space) |
| L / R bumpers | workspace / session prev / next |
| ZL / ZR triggers | tab / window prev / next |
| D-pad | pane focus (directional) |
| Left stick | arrow keys (4-way, repeats while held) |
| Right stick | scroll (2D analog, accelerates) |

## Sticks

Each analog stick has a configurable **behavior** — change them in `config edit`
(the "left stick" / "right stick" rows) or with `config set`:

| Behavior | Effect |
|---|---|
| `keys` | 4-way arrow keys to the focused pane (repeats while held) |
| `panes` | 4-way pane focus (`pane_left/right/up/down`, one move per push) |
| `scroll` | 2D wheel — push ↕ = vertical, ↔ = horizontal (accelerates) |
| `off` | disabled |

Defaults: **left = `keys`**, **right = `scroll`**. So to drive panes with the
left stick: `cc-controller config set settings.sticks.left.behavior panes` (or
flip it in `config edit`). Per-stick params live under
`settings.sticks.{left,right}`:

```json
"sticks": {
  "left":  { "behavior": "panes", "deadzone": 0.5, "repeat_ms": 150 },
  "right": { "behavior": "scroll", "deadzone": 0.18, "tick_ms": 30, "max_lines": 6, "invert": false }
}
```

`keys`/`panes` use `deadzone` + `repeat_ms`; `scroll` uses `deadzone` +
`tick_ms` + `max_lines` + `invert`. The left stick maps to the `left_x`/`left_y`
profile axes, the right stick to `right_x`/`right_y`.

## Editing config from the CLI

`mapping.json` is the source of truth, but you rarely need to open it:

```bash
cc-controller config show                       # styled view of bindings + settings
cc-controller config get bindings.ZR            # read a value (dotted path)
cc-controller config set backend tmux           # write a value (coerces bool/number/string)
cc-controller config set settings.sticks.left.behavior panes
cc-controller config bind                        # quick: pick a control, pick an action
cc-controller config edit                        # full-screen grid editor (below)
cc-controller config path                        # where is mapping.json?
```

`config edit` is an arrow-key grid for every button at once: **↑/↓** moves
between buttons (and the cyclable settings — left/right stick behavior, backend,
voice mode), **←/→** changes the highlighted one's action/value (including `—` to
unbind), **Enter** saves, **Esc** cancels. `config bind` is the quicker
two-prompt path for a single control.

`config set` / `bind` / `edit` rewrite the file with key order and the `_comment`
documentation keys preserved (they do normalise whitespace to canonical JSON).
`bind` and `edit` need an interactive terminal.

## Background daemon

`local` and `remote` can run detached (they share one PID file, since only one
process can own the controller):

```bash
cc-controller local start      # detached; logs to .cc-controller.log next to mapping.json
cc-controller local status     # running?
cc-controller local stop       # stop it
```

## Scrolling

A stick set to `scroll` (right stick by default) sends mouse-wheel events: the
further you push, the more wheel notches per `tick_ms` (acceleration) up to
`max_lines`. It's **2D** — vertical *and* horizontal (the latter for programs
that handle horizontal wheel). Flip vertical with `config set
settings.sticks.right.invert true`.

**Why mouse-wheel and not PageUp?** A mouse wheel is just an escape sequence on
the program's input, which both backends can deliver over the socket. The
trade-off: it drives the focused **program's** scroll (e.g. Claude's transcript),
not the multiplexer's own scrollback.

## Voice

The Switch Pro Controller has **no microphone** — voice input uses the
multiplexer machine's mic. Two approaches, pick per button:

- **`voice`** (default on Y) — emulates Claude Code's **hold-space** by streaming
  spaces to the focused pane while active. `settings.voice`:

  ```json
  "voice": { "mode": "both", "tap_max_ms": 300, "repeat_ms": 90, "char": " " }
  ```

  `mode`: `both` (quick **tap** = toggle on/off, **hold** = momentary),
  `hold` (momentary only), or `toggle` (tap on/off only).

- **`dictation`** — fires an OS dictation hotkey via `settings.dictation_command`
  (macOS `shortcuts run`/`osascript`; Windows `Win+H`). Runs on the machine where
  the **controller** is read (handy in remote mode, where the mic is elsewhere).
