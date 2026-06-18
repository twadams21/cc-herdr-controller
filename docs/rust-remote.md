# Rust V2 + remote herdr (controller on one machine, herdr on another)

V1 (the Python daemon) assumes the controller and herdr live on the **same**
machine — it reads the controller locally and shells out to a local `herdr`
binary. This guide covers the **Rust V2** in `rust/`, which splits that across
two machines: the controller on a **Windows PC**, and herdr on a **Mac** reached
over SSH.

```
[ Windows PC ]                                  [ Mac ]
controller (Bluetooth/USB)                      herdr server + socket + `herdr`
        │ SDL2 (local)                                   ▲
        ▼                                                │ local socket (fast)
cc-herdr-controller.exe ── intents ──► ssh ──► relay.py ─┘
   (reads pad, maps to intents)     one persistent connection
```

Only compact **intents** (`tab_next`, `scroll 3`, `keys enter`, …) cross the
network, over a single long-lived SSH connection. All the chatty herdr work
(list workspaces → compute neighbour → focus) runs **on the Mac** against
herdr's local socket, so latency is roughly one network hop per action.

The Rust daemon and the Python daemon share the same `mapping.json`, so a
profile calibrated by either works in the other.

V2 can also run **entirely on one machine** with `--local` — the controller and
herdr on the same Mac, no SSH. It launches `relay.py` as a local subprocess
instead of over SSH; everything else (intents, dispatch) is identical. See
[Local mode](#local-mode-controller--herdr-on-one-machine) below. This is the
same single-machine shape as V1, but in Rust.

## Wire protocol (intents)

The Windows binary writes newline-delimited intents to the relay's stdin:

| Intent | Meaning |
|---|---|
| `tab_next` / `tab_prev` | focus next/prev tab |
| `workspace_next` / `workspace_prev` | focus next/prev workspace |
| `pane <left\|right\|up\|down>` | move pane focus |
| `zoom` | toggle pane zoom |
| `scroll <signed-int>` | wheel notches; + = up (older content) |
| `keys <name...>` | send key(s) to focused pane (`enter`, `esc`, arrows) |
| `text <base64>` | send literal text (used for voice spaces) |
| `ping` | keepalive / no-op |

---

## Mac setup (one time)

1. **Clone the repo** so `herdr.py` + `relay.py` are present:
   ```bash
   git clone https://github.com/twadams21/cc-herdr-controller ~/cc-herdr-controller
   ```
2. **Python 3** is already on macOS; the relay is stdlib-only (no pip install).
3. **`herdr` must be on PATH for a non-interactive SSH shell.** `ssh mac 'herdr
   --version'` must work. If it prints "command not found", either symlink herdr
   into `/usr/local/bin`, or set `relay_cmd` (below) to an absolute path, e.g.
   `cd ~/cc-herdr-controller && HERDR_BIN=/opt/homebrew/bin/herdr python3 relay.py`.
   (`herdr.py` honours the `HERDR_BIN` env var.)
4. **Run herdr from the Mac's GUI session** (e.g. in your terminal app), then
   attach to it over SSH as you already do. Input/voice behave best when herdr
   was launched from the GUI session — see [troubleshooting](troubleshooting.md).

The relay itself is launched automatically by the Windows side over SSH; you
don't start it by hand.

## Windows setup (one time)

Prereqs (already done on this machine):

- Rust **GNU** toolchain: `rustup default stable-x86_64-pc-windows-gnu`
- SDL2 MinGW dev libs: `powershell -ExecutionPolicy Bypass -File rust\setup-sdl2.ps1`

Build:

```powershell
cargo build --release --manifest-path rust\Cargo.toml
# -> rust\target\release\cc-herdr-controller.exe  (+ SDL2.dll beside it)
```

> The exe needs `SDL2.dll` next to it; the build copies it automatically. If you
> move the exe elsewhere, copy `SDL2.dll` along with it.

**Set up SSH key auth to the Mac** so reconnects never prompt for a password:

```powershell
ssh-keygen -t ed25519        # if you don't have a key
type $env:USERPROFILE\.ssh\id_ed25519.pub | ssh user@mac "cat >> ~/.ssh/authorized_keys"
ssh user@mac "echo ok"       # should print ok with no password
```

**Point the daemon at the Mac** — edit the `remote` block in `mapping.json`:

```json
"remote": {
  "ssh_host": "user@mac",
  "ssh_args": [],
  "relay_cmd": "cd ~/cc-herdr-controller && python3 relay.py"
}
```

`ssh_host` can be any valid SSH target, including a `~/.ssh/config` Host alias.
Or skip the config and pass `--host user@mac` on the command line.

## Run it

```powershell
$exe = "rust\target\release\cc-herdr-controller.exe"

& $exe --list         # detected controllers
& $exe --calibrate    # press each control -> writes profile to mapping.json
& $exe --discover     # raw index + the name your profile assigns it
& $exe --dry-run      # full mapping, prints intents instead of sending (no Mac)
& $exe --monitor      # like --dry-run but labelled MONITOR; add --perform to act
& $exe                # drive herdr for real (needs remote.ssh_host or --host)
```

**Calibrate first on Windows** — SDL's raw indices differ from macOS, and your
Switch Pro's D-pad reports as buttons here (`hats=0`), not a hat. In calibrate,
press Enter to skip a control, or type `q`+Enter to stop early.

Test the mapping with `--dry-run` (no Mac needed), then go live with no args
once `remote.ssh_host` is set.

## Local mode (controller + herdr on one machine)

When the controller and herdr live on the **same** machine (e.g. a Switch Pro
paired to the Mac over Bluetooth, driving herdr on that Mac), pass `--local`.
There's no SSH: the daemon runs `relay.py` as a child process, piping intents to
its stdin, and the relay drives herdr over its local socket. The relay inherits
the daemon's environment, so launch the daemon from a shell that can reach herdr
(e.g. inside a herdr session, or anywhere `herdr` is on `PATH`).

**Build** (macOS/Linux — needs the system SDL2):

```bash
brew install sdl2                       # macOS; Linux: apt install libsdl2-dev
cargo build --release --manifest-path rust/Cargo.toml
# -> rust/target/release/cc-herdr-controller
```

The `sdl2` crate is resolved via `pkg-config` on these platforms (the Windows
MinGW vendoring in `build.rs` is skipped). No `SDL2.dll` to ship.

**Configure** — the `local` block in `mapping.json` (defaults are fine if you run
from the repo root, where `relay.py`/`herdr.py` live):

```json
"local": {
  "relay_cmd": "python3 relay.py"
}
```

`relay_cmd` runs via the shell from the directory holding `mapping.json`.

**Run** (from the repo root):

```bash
bin=rust/target/release/cc-herdr-controller
"$bin" --list             # detected controllers
"$bin" --dry-run          # full mapping, prints intents (no herdr)
"$bin" --monitor          # input -> action, does nothing; add --perform to act
"$bin" --local            # drive herdr on this machine for real
```

## Notes & caveats

- **Voice mic is on the Mac.** Voice mode streams spaces to trigger Claude's
  hold-space; Claude then listens on the **Mac's** mic, not the Windows PC's. In
  a remote setup that makes in-app voice impractical. `dictation` instead runs
  `settings.dictation_command` **locally on Windows** (e.g. to trigger Win+H).
- **Latency.** Navigation is ~one network hop per action. Rapid analog scroll
  fires up to every `settings.scroll.tick_ms`; on a high-latency link, raise
  `tick_ms` if it feels like it's queueing.
- **Reconnect.** If the SSH connection drops, the daemon respawns it on the next
  intent. `ServerAliveInterval` is set so dead connections are noticed quickly.
- **Background mode** (`--bg`/`--status`/`--stop`) from the Python daemon is not
  yet ported to Rust; run it in a terminal window for now.
- **Single binary.** V2 currently ships `exe` + `SDL2.dll`. A fully static
  single binary needs a complete mingw-w64 (for `imm32`/`version`/`dinput8`/
  `dxguid`/`cfgmgr32`/`hid` import libs, which rustup's bundled linker lacks).
