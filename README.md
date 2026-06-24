# cc-controller

Drive [herdr](https://herdr.dev) or **tmux** — and the AI agents running inside
them — from a game controller. Map a Nintendo Switch Pro (or any SDL controller:
Xbox, DualSense, …) to multiplexer actions: switch windows/tabs and
sessions/workspaces, move between panes, scroll, send keys, and trigger Claude's
voice mode — all without reaching for the keyboard.

Every action goes through the multiplexer's own **socket** (`herdr` / `tmux`
CLI), so it works even with the host **headless / over SSH** — no OS-level input
injection, no extra permissions beyond reading the controller.

One Rust binary, three modes:

| Mode | What it does |
|---|---|
| `cc-controller local` | controller + multiplexer on **this** machine; dispatch in-process |
| `cc-controller remote` | controller here → one SSH connection → multiplexer on **another** machine |
| `cc-controller host` | the multiplexer-side dispatcher `remote` runs over SSH (rarely run by hand) |

## Requirements

- A controller your OS recognises (Switch Pro, Xbox, DualSense, …)
- [herdr](https://herdr.dev) and/or `tmux` on the machine that runs the multiplexer
- A Rust toolchain to build (`cargo`), and SDL2 (`brew install sdl2`, or
  `apt install libsdl2-dev`; Windows uses vendored MinGW SDL2)

## Install

```bash
git clone https://github.com/twadams21/cc-herdr-controller
cd cc-herdr-controller/rust
cargo build --release
# -> target/release/cc-controller   (put it on your PATH)
```

On Windows, fetch SDL2 first: `powershell -ExecutionPolicy Bypass -File setup-sdl2.ps1`
(the build copies `SDL2.dll` next to the exe). See
[remote setup](docs/rust-remote.md) for the controller-on-Windows / mux-on-Mac case.

## Quick start

```bash
cc-controller calibrate          # press each control to learn your pad
cc-controller local --dry-run    # watch input → intents, perform NOTHING
cc-controller local              # drive the multiplexer on this machine
cc-controller local --tmux       # …driving tmux instead of herdr
```

Run it detached with `cc-controller local start` (then `… status` / `… stop`).

Default layout: **bumpers** = workspaces/sessions, **triggers** = tabs/windows,
**D-pad** = panes, **left stick** = arrow keys, **right stick** = scroll, **A/B**
= enter/esc, **Y** = voice. Remap buttons with `cc-controller config bind`, and
set each **stick's behavior** (keys / panes / scroll / off) in `config edit` —
e.g. left stick → pane navigation. Or edit `mapping.json` directly.

**Prefer tmux?** The same bindings drive it — `--tmux` (or `cc-controller config
set backend tmux`). Workspaces map to tmux sessions, tabs to windows, panes to
panes. See [the guide](docs/guide.md#backends-herdr-or-tmux).

On macOS you'll likely need to grant **Input Monitoring** — see
[troubleshooting](docs/troubleshooting.md) if no inputs register.

## Docs

- **[Usage & configuration](docs/guide.md)** — commands, modes, calibration, bindings, config editing, scrolling, voice
- **[Remote setup](docs/rust-remote.md)** — controller on one machine, multiplexer on another (over SSH)
- **[Troubleshooting & caveats](docs/troubleshooting.md)** — macOS Input Monitoring, controller conflicts, known limitations
