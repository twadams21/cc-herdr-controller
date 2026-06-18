# cc-herdr-controller

Drive [herdr](https://herdr.dev) — and the AI agents running inside it — from a
game controller. Map a Nintendo Switch Pro (or any SDL controller: Xbox,
DualSense, …) to herdr actions: switch tabs and workspaces, move between panes,
scroll, send arrow keys, and trigger Claude's voice mode — all without reaching
for the keyboard.

Every action is sent through herdr's **socket**, so it works even with the host
machine **headless / over SSH** — no OS-level input injection, no extra
permissions beyond reading the controller.

## Status — V1

A Python/SDL prototype, and genuinely a joy to use. Also rough in places: a
little buggy, voice mode is experimental, and the CLI needs work.

A **Rust V2** now lives in [`rust/`](rust/) — lower latency, and it adds a
**remote** mode so the controller can live on one machine (e.g. a Windows PC)
while herdr runs on another (e.g. a Mac) reached over SSH. It shares
`mapping.json` with V1. See **[Rust V2 + remote herdr](docs/rust-remote.md)**.

## Requirements

- A controller your OS recognises (Switch Pro, Xbox, DualSense, …)
- [herdr](https://herdr.dev), running on the same machine as the daemon
- Python 3.9+ (macOS or Windows)

## Install

```bash
git clone https://github.com/twadams21/cc-herdr-controller
cd cc-herdr-controller
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
```

## Quick start

```bash
# 1. Learn your controller's buttons (press each when prompted)
.venv/bin/python controller_daemon.py --calibrate

# 2. Watch inputs map to actions WITHOUT performing them
.venv/bin/python controller_daemon.py --monitor

# 3. Drive herdr for real
.venv/bin/python controller_daemon.py
```

Default layout: **bumpers** = workspaces, **triggers** = tabs, **D-pad** =
panes, **left stick** = arrow keys, **right stick** = scroll, **A/B** =
enter/esc, **Y** = voice. Remap anything in `mapping.json`. Run it detached with
`--bg` (then `--status` / `--stop`).

On macOS you'll likely need to grant **Input Monitoring** — see
[troubleshooting](docs/troubleshooting.md) if no inputs register.

## Docs

- **[Usage & configuration](docs/guide.md)** — how it works, modes, calibration, bindings, scrolling, voice
- **[Rust V2 + remote herdr](docs/rust-remote.md)** — controller on one machine, herdr on another (over SSH)
- **[Troubleshooting & caveats](docs/troubleshooting.md)** — macOS Input Monitoring, controller conflicts, known limitations
