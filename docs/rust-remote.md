# Remote setup (controller on one machine, multiplexer on another)

`cc-controller remote` splits the rig across two machines: the controller on a
**Windows PC**, and herdr/tmux on a **Mac** (or any host) reached over SSH.

```
[ Windows PC ]                                      [ Mac ]
controller (Bluetooth/USB)                          herdr / tmux + socket
        │ SDL2 (local)                                       ▲
        ▼                                                    │ local socket (fast)
cc-controller remote ── intents ──► ssh ──► cc-controller host ─┘
   (reads pad, maps to intents)   one persistent connection
```

Only compact **intents** (`tab_next`, `scroll 3`, `keys enter`, …) cross the
network, over a single long-lived SSH connection. All the chatty work (herdr:
list workspaces → compute neighbour → focus) runs **on the host** against the
local socket, so latency is roughly one network hop per action. SSH provides the
auth and encryption — there are no open ports.

Both machines run the **same** `cc-controller` binary and share the same
`mapping.json`, so a profile calibrated on either works on the other.

## Wire protocol (intents)

The controller side writes newline-delimited intents to `cc-controller host`'s
stdin:

| Intent | Meaning |
|---|---|
| `tab_next` / `tab_prev` | focus next/prev tab (herdr) / window (tmux) |
| `workspace_next` / `workspace_prev` | focus next/prev workspace / session |
| `pane <left\|right\|up\|down>` | move pane focus |
| `zoom` | toggle pane zoom |
| `scroll <signed-int>` | wheel notches; + = up (older content) |
| `keys <name...>` | send key(s) to the focused pane (`enter`, `esc`, arrows) |
| `text <base64>` | send literal text (used for voice spaces) |
| `ping` | keepalive / no-op |

---

## Host (Mac) setup — one time

1. **Install `cc-controller` on the host** so SSH can run it:
   ```bash
   git clone https://github.com/twadams21/cc-herdr-controller ~/cc-herdr-controller
   cd ~/cc-herdr-controller/rust
   brew install sdl2            # the host binary still links libSDL2
   cargo build --release
   cp target/release/cc-controller /usr/local/bin/   # or anywhere on PATH
   ```
2. **`herdr` / `tmux` must be on PATH for a non-interactive SSH shell.** Check
   with `ssh mac 'herdr --version'` (or `tmux -V`). If it prints "command not
   found", either symlink it onto `/usr/local/bin`, or point cc-controller at an
   absolute path via env: it honours `CC_HERDR_BIN` and `CC_TMUX_BIN`.
3. **Run herdr from the Mac's GUI session** (e.g. your terminal app), then attach
   over SSH as usual. Input/voice behave best when herdr was launched from the
   GUI session — see [troubleshooting](troubleshooting.md).

`cc-controller host` is launched automatically by the controller side over SSH —
you don't start it by hand.

## Controller (Windows) setup — one time

- Rust **GNU** toolchain: `rustup default stable-x86_64-pc-windows-gnu`
- SDL2 MinGW dev libs: `powershell -ExecutionPolicy Bypass -File rust\setup-sdl2.ps1`

Build:

```powershell
cargo build --release --manifest-path rust\Cargo.toml
# -> rust\target\release\cc-controller.exe  (+ SDL2.dll beside it)
```

> The exe needs `SDL2.dll` next to it; the build copies it automatically. If you
> move the exe, copy `SDL2.dll` along with it.

**Set up SSH key auth to the host** so reconnects never prompt for a password:

```powershell
ssh-keygen -t ed25519        # if you don't have a key
type $env:USERPROFILE\.ssh\id_ed25519.pub | ssh user@mac "cat >> ~/.ssh/authorized_keys"
ssh user@mac "echo ok"       # should print ok with no password
```

**Point it at the host** — the `remote` block in `mapping.json`:

```json
"remote": {
  "ssh_host": "user@mac",
  "ssh_args": [],
  "host_bin": "cc-controller"
}
```

`ssh_host` is any valid SSH target (incl. a `~/.ssh/config` alias); or skip the
config and pass `--host user@mac`. `host_bin` is the binary name/path on the
host — make it an absolute path if it isn't on the non-interactive SSH PATH.

## Run it

```powershell
$exe = "rust\target\release\cc-controller.exe"

& $exe list                  # detected controllers
& $exe calibrate             # press each control -> writes profile to mapping.json
& $exe discover              # raw index + the name your profile assigns it
& $exe remote --dry-run      # full mapping, prints intents instead of sending (no host)
& $exe remote                # drive the host for real (needs remote.ssh_host or --host)
& $exe remote --tmux         # …driving tmux on the host
& $exe remote start          # run detached; `remote status` / `remote stop` to manage
```

**Calibrate first on Windows** — SDL's raw indices differ from macOS, and your
Switch Pro's D-pad reports as buttons here (`hats=0`), not a hat. Test the
mapping with `remote --dry-run` (no host needed), then go live.

## Local mode (controller + multiplexer on one machine)

When both live on the **same** machine (e.g. a Switch Pro paired to the Mac that
runs herdr), use `local` — no SSH, intents dispatch in-process:

```bash
brew install sdl2                                  # macOS; Linux: apt install libsdl2-dev
cargo build --release --manifest-path rust/Cargo.toml
bin=rust/target/release/cc-controller
"$bin" local --dry-run        # full mapping, prints intents (no multiplexer)
"$bin" local                  # drive the multiplexer on this machine
"$bin" local --tmux           # …driving tmux
```

Launch from a shell that can reach the multiplexer (e.g. inside a herdr session,
or anywhere `herdr`/`tmux` is on `PATH`).

## Notes & caveats

- **Voice mic is on the host.** Voice mode streams spaces to trigger Claude's
  hold-space; Claude then listens on the **host's** mic, not the controller PC's.
  In a remote setup that makes in-app voice impractical — use `dictation`
  instead, which runs `settings.dictation_command` **on the controller machine**
  (e.g. Win+H).
- **Latency.** Navigation is ~one network hop per action. Rapid analog scroll
  fires up to every `settings.scroll.tick_ms`; raise it on a high-latency link if
  it feels like it's queueing.
- **Reconnect.** If the SSH connection drops, the daemon respawns it on the next
  intent. `ServerAliveInterval` is set so dead connections are noticed quickly.
- **Single binary on Windows** currently ships `cc-controller.exe` + `SDL2.dll`.
  A fully static exe needs a complete mingw-w64 (for `imm32`/`version`/`dinput8`/
  `dxguid`/`cfgmgr32`/`hid` import libs, which rustup's bundled linker lacks).
