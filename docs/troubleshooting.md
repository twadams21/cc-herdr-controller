# Troubleshooting & caveats

See the [usage & configuration guide](guide.md) for everything else.

## Granting Input Monitoring (macOS)

Reading controller input is gated by **Input Monitoring**. If `--discover` keeps
printing `listening, 0 events` while you press buttons:

1. **System Settings → Privacy & Security → Input Monitoring**.
2. Add/enable the **terminal app that hosts the daemon** (Ghostty / iTerm /
   Terminal — whatever herdr runs in). Granting the host app is more reliable
   than adding the bare Python binary. (Click **+**, **⌘⇧G** to type a path.)
3. **Relaunch** the daemon — TCC re-checks only at process start.
4. The first-run permission prompt appears on the **Mac's physical screen**, not
   in an SSH terminal.

**SSH note:** Input Monitoring is tied to the GUI login session. A daemon
launched from a bare `ssh` shell may get no events even with permission granted.
Run it **inside herdr** (launched from the Mac's GUI session) for reliable input.

## `Couldn't setup USB mode` / `No controller detected`

Only one process can own a controller at a time. Stop any other instance (an old
daemon, a `--bg` run — see `--status` / `--stop`) before starting another.

If it persists with nothing else running, macOS's native driver is clashing with
SDL's Switch driver — set `SDL_JOYSTICK_HIDAPI=0` before launch to use the OS
gamepad backend instead.

## Other caveats

- **Scroll/voice need the right focused program.** Both inject into the focused
  pane's program. Scroll needs a mouse-tracking app; voice-space needs Claude
  focused. Sent to a bare shell they're just junk characters.
- **macOS also grabs the controller.** The GameController framework acts on some
  buttons system-wide (Home → Launchpad). Avoid binding those, or suppress the
  OS handler. Actions routed via herdr are unaffected.
- **Cross-platform:** the pygame layer is portable; only `dictation_command` and
  the OS-grab caveats differ between macOS and Windows.
