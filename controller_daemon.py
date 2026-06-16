#!/usr/bin/env python3
"""Drive herdr from a game controller (Switch Pro and friends).

Bindings use friendly control names (A, ZL, dpad_up, ...) mapped to herdr
actions; a per-controller `profile` translates names to raw SDL indices.
Analog scrolling (right stick) is emulated as real mouse-wheel events with
acceleration. The voice button emulates Claude's hold-space.

    python controller_daemon.py --list        # detected controllers
    python controller_daemon.py --calibrate    # press each control to build the profile
    python controller_daemon.py --discover      # raw input + the name it maps to
    python controller_daemon.py --monitor       # show input -> action, do NOTHING
    python controller_daemon.py --monitor --perform   # show AND drive herdr
    python controller_daemon.py                 # run (drives herdr)
    python controller_daemon.py --dry-run       # run, but perform nothing

Background control:
    python controller_daemon.py --bg            # start detached (logs to .daemon.log)
    python controller_daemon.py --status        # is it running?
    python controller_daemon.py --stop          # stop the background daemon
"""

from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import time
from pathlib import Path

os.environ.setdefault("PYGAME_HIDE_SUPPORT_PROMPT", "1")
import pygame  # noqa: E402

import herdr  # noqa: E402

CONFIG_PATH = Path(__file__).with_name("mapping.json")
PIDFILE = Path(__file__).with_name(".daemon.pid")
LOGFILE = Path(__file__).with_name(".daemon.log")

HAT_DIRECTIONS = {(0, 1): "up", (0, -1): "down", (-1, 0): "left", (1, 0): "right"}

CALIBRATION_ORDER = [
    "A", "B", "X", "Y",
    "L", "R", "ZL", "ZR",
    "minus", "plus", "home", "capture",
    "lstick", "rstick",
    "dpad_up", "dpad_down", "dpad_left", "dpad_right",
]

_STOP = False  # set by SIGTERM so the background daemon exits cleanly


def _handle_term(signum, frame):  # noqa: ARG001
    global _STOP
    _STOP = True


# ---- config ----------------------------------------------------------------

def load_config() -> dict:
    with open(CONFIG_PATH) as fh:
        return json.load(fh)


def save_config(cfg: dict) -> None:
    with open(CONFIG_PATH, "w") as fh:
        json.dump(cfg, fh, indent=2)
        fh.write("\n")


def name_maps(cfg: dict):
    """Return (button_index -> name, axis_name -> index) from the profile."""
    profile = cfg.get("profile", {})
    btn_name = {int(i): n for n, i in profile.get("buttons", {}).items()
                if str(n) != "_comment"}
    axis_index = {n: int(i) for n, i in profile.get("axes", {}).items()
                  if str(n) != "_comment"}
    return btn_name, axis_index


def scroll_lines(value: float, deadzone: float, max_lines: int) -> int:
    """Map an analog deflection to lines/tick (acceleration). Pure; tested."""
    a = abs(value)
    if a < deadzone:
        return 0
    t = (a - deadzone) / (1.0 - deadzone)
    t = max(0.0, min(1.0, t))
    return max(1, round(t * max_lines))


# ---- action dispatch -------------------------------------------------------

def make_dispatch(cfg: dict):
    settings = cfg.get("settings", {})
    invert = bool(settings.get("scroll", {}).get("invert", False))
    notch = 3

    def dictation() -> None:
        cmd = settings.get("dictation_command")
        if cmd:
            subprocess.Popen(cmd, shell=True)

    def scroll_button(up: bool) -> None:
        lines = notch if up else -notch
        herdr.scroll(-lines if invert else lines)

    actions = {
        "tab_next": lambda: herdr.tab_step(+1),
        "tab_prev": lambda: herdr.tab_step(-1),
        "workspace_next": lambda: herdr.workspace_step(+1),
        "workspace_prev": lambda: herdr.workspace_step(-1),
        "pane_left": lambda: herdr.pane_focus("left"),
        "pane_right": lambda: herdr.pane_focus("right"),
        "pane_up": lambda: herdr.pane_focus("up"),
        "pane_down": lambda: herdr.pane_focus("down"),
        "pane_zoom": herdr.pane_zoom_toggle,
        "scroll_up": lambda: scroll_button(True),
        "scroll_down": lambda: scroll_button(False),
        "enter": lambda: herdr.send_keys("enter"),
        "escape": lambda: herdr.send_keys("esc"),
        "dictation": dictation,
        "voice": lambda: None,   # handled specially in the loop
        "noop": lambda: None,
    }

    def dispatch(action: str) -> None:
        fn = actions.get(action)
        if fn is None:
            print(f"  ! unknown action: {action!r}", file=sys.stderr)
            return
        try:
            fn()
        except herdr.HerdrError as exc:
            print(f"  ! {action}: {exc}", file=sys.stderr)

    return dispatch


# ---- controller setup ------------------------------------------------------

def open_controller():
    pygame.init()
    pygame.joystick.init()
    if pygame.joystick.get_count() == 0:
        print("No controller detected. Plugged in / paired? Another process "
              "holding it? Only one program can own a controller at a time.",
              file=sys.stderr)
        sys.exit(1)
    js = pygame.joystick.Joystick(0)
    js.init()
    print(f"Controller: {js.get_name()}  "
          f"(buttons={js.get_numbuttons()} axes={js.get_numaxes()} hats={js.get_numhats()})")
    return js


def list_controllers() -> None:
    pygame.init()
    pygame.joystick.init()
    n = pygame.joystick.get_count()
    if n == 0:
        print("No controllers detected.")
        return
    for i in range(n):
        js = pygame.joystick.Joystick(i)
        js.init()
        print(f"[{i}] {js.get_name()}  "
              f"buttons={js.get_numbuttons()} axes={js.get_numaxes()} hats={js.get_numhats()}")


# ---- calibrate / discover --------------------------------------------------

def _await_input(js):
    """Block until a control fires; return ('button'|'axis'|'hat', i) or None
    if the user pressed Enter (POSIX) to skip."""
    import select
    pygame.event.clear()
    while True:
        for ev in pygame.event.get():
            if ev.type == pygame.JOYBUTTONDOWN:
                return ("button", ev.button)
            if ev.type == pygame.JOYAXISMOTION and abs(ev.value) > 0.7:
                return ("axis", ev.axis)
            if ev.type == pygame.JOYHATMOTION and ev.value != (0, 0):
                return ("hat", ev.hat)
        if os.name != "nt" and select.select([sys.stdin], [], [], 0)[0]:
            sys.stdin.readline()
            return None
        time.sleep(0.01)


def run_calibrate() -> None:
    js = open_controller()
    cfg = load_config()
    profile = cfg.get("profile", {}) or {}
    buttons: dict[str, int] = {}
    axes = {n: int(i) for n, i in profile.get("axes", {}).items()
            if str(n) != "_comment"}
    print("\nCALIBRATE — press each control when prompted.")
    print("Press Enter (in this terminal) to skip a control. Ctrl-C to stop.\n")
    try:
        for name in CALIBRATION_ORDER:
            print(f"  press [{name}] (Enter=skip): ", end="", flush=True)
            res = _await_input(js)
            if res is None:
                print("skipped")
                continue
            if res[0] == "button":
                buttons[name] = res[1]
                print(f"button {res[1]}")
            elif res[0] == "axis":
                axes[name] = res[1]
                print(f"axis {res[1]}")
            else:
                print("hat — D-pad is a hat, handled automatically (skipping)")
    except KeyboardInterrupt:
        print("\n(stopped early)")
    profile["buttons"] = buttons
    profile["axes"] = axes
    cfg["profile"] = profile
    save_config(cfg)
    print(f"\nSaved profile to {CONFIG_PATH.name}. Set `bindings`, then --monitor.")


def run_discover() -> None:
    js = open_controller()  # noqa: F841  (kept open for the event stream)
    btn_name, axis_index = name_maps(load_config())
    axis_name = {i: n for n, i in axis_index.items()}
    print("\nDISCOVER MODE — press each control; Ctrl-C to quit.")
    print("Shows raw index and the name your profile assigns it.\n")
    events_seen = 0
    last_beat = time.monotonic()
    try:
        while not _STOP:
            for ev in pygame.event.get():
                if ev.type == pygame.JOYBUTTONDOWN:
                    events_seen += 1
                    print(f"button {ev.button}  DOWN   ({btn_name.get(ev.button, 'unnamed')})")
                elif ev.type == pygame.JOYHATMOTION and ev.value != (0, 0):
                    events_seen += 1
                    d = HAT_DIRECTIONS.get(tuple(ev.value), "?")
                    print(f"hat {ev.hat}  {ev.value}   (dpad_{d})")
                elif ev.type == pygame.JOYAXISMOTION and abs(ev.value) > 0.5:
                    events_seen += 1
                    print(f"axis {ev.axis}  {ev.value:+.2f}   ({axis_name.get(ev.axis, 'unnamed')})")
            now = time.monotonic()
            if now - last_beat >= 2.0:
                last_beat = now
                if events_seen == 0:
                    print("… listening, 0 events. If pressing does nothing, grant "
                          "Input Monitoring (see README).")
                else:
                    print(f"… listening ({events_seen} events so far)")
            time.sleep(0.01)
    except KeyboardInterrupt:
        print("\nbye")


# ---- main loop -------------------------------------------------------------

def run_loop(cfg: dict, perform: bool = True, label: str = "RUNNING") -> None:
    signal.signal(signal.SIGTERM, _handle_term)
    js = open_controller()
    dispatch = make_dispatch(cfg)
    bindings = cfg.get("bindings", {})
    settings = cfg.get("settings", {})
    btn_name, axis_index = name_maps(cfg)
    btn_index = {n: i for i, n in btn_name.items()}

    trigger_threshold = float(settings.get("trigger_threshold", 0.5))
    sc = settings.get("scroll", {})
    scroll_axis = axis_index.get(sc.get("axis", "right_y"))
    scroll_invert = bool(sc.get("invert", False))
    scroll_dead = float(sc.get("deadzone", 0.18))
    scroll_tick = int(sc.get("tick_ms", 30)) / 1000.0
    scroll_max = int(sc.get("max_lines", 6))

    # Left stick -> arrow keys (only if an "arrows" block is present).
    arrows_on = "arrows" in settings
    ar = settings.get("arrows", {})
    arrows_ax = axis_index.get(ar.get("axis_x", "left_x")) if arrows_on else None
    arrows_ay = axis_index.get(ar.get("axis_y", "left_y")) if arrows_on else None
    arrows_dead = float(ar.get("deadzone", 0.5))
    arrows_repeat = int(ar.get("repeat_ms", 150)) / 1000.0

    vc = settings.get("voice", {})
    voice_mode = vc.get("mode", "both")
    voice_tap_max = int(vc.get("tap_max_ms", 300)) / 1000.0
    voice_repeat = int(vc.get("repeat_ms", 90)) / 1000.0
    voice_char = vc.get("char", " ")
    voice_name = next((n for n, a in bindings.items() if a == "voice"), None)
    voice_idx = btn_index.get(voice_name) if voice_name else None

    # Triggers = bindings whose name is an axis (e.g. ZL/ZR), minus the scroll axis.
    triggers = {n: axis_index[n] for n, a in bindings.items()
                if n in axis_index and n != sc.get("axis", "right_y")}
    trigger_down = {n: False for n in triggers}

    # voice state
    v_pressing = False
    v_latched = False
    v_down_t = 0.0
    v_last_send = 0.0
    scroll_last = 0.0
    arrows_last_dir = None
    arrows_last_fire = 0.0
    events_seen = 0
    last_beat = time.monotonic()

    def act(action: str, src: str) -> None:
        if perform:
            print(f"{src} -> {action}  [performed]")
            dispatch(action)
        else:
            print(f"{src} -> {action}  [skipped: do-nothing]")

    mode = "driving herdr" if perform else "OBSERVING ONLY — not performing actions"
    print(f"\n{label} — {mode}. Ctrl-C to quit.\n")
    try:
        while not _STOP:
            now = time.monotonic()
            for ev in pygame.event.get():
                if ev.type == pygame.JOYBUTTONDOWN:
                    events_seen += 1
                    if ev.button == voice_idx:
                        v_pressing = True
                        v_down_t = now
                        continue
                    name = btn_name.get(ev.button, f"button{ev.button}")
                    action = bindings.get(name)
                    if action and action != "voice":
                        act(action, name)
                    elif not action:
                        print(f"{name} (button {ev.button})  (unbound)")
                elif ev.type == pygame.JOYBUTTONUP:
                    if ev.button == voice_idx and v_pressing:
                        v_pressing = False
                        held = now - v_down_t
                        if voice_mode == "hold":
                            v_latched = False
                        elif voice_mode == "toggle":
                            v_latched = not v_latched
                        else:  # both: quick tap toggles, long hold is momentary
                            v_latched = (not v_latched) if held <= voice_tap_max else False
                elif ev.type == pygame.JOYHATMOTION and ev.value != (0, 0):
                    events_seen += 1
                    d = HAT_DIRECTIONS.get(tuple(ev.value))
                    name = f"dpad_{d}" if d else None
                    action = bindings.get(name) if name else None
                    if action:
                        act(action, name)

            # Triggers (ZL/ZR): fire on rising edge past the threshold.
            for name, aidx in triggers.items():
                if aidx >= js.get_numaxes():
                    continue
                pressed = js.get_axis(aidx) > trigger_threshold
                if pressed and not trigger_down[name]:
                    act(bindings[name], name)
                trigger_down[name] = pressed

            # Analog scroll with acceleration (SGR wheel via the socket).
            if scroll_axis is not None and scroll_axis < js.get_numaxes():
                val = js.get_axis(scroll_axis)
                lines = scroll_lines(val, scroll_dead, scroll_max)
                if lines and now - scroll_last >= scroll_tick:
                    scroll_last = now
                    up = val < 0  # stick up = scroll up (older content)
                    delta = lines if up else -lines
                    if scroll_invert:
                        delta = -delta
                    if perform:
                        herdr.scroll(delta)
                    else:
                        print(f"scroll {'up' if delta > 0 else 'down'} {abs(delta)}  [skipped: do-nothing]")

            # Left stick -> arrow keys (4-way dominant axis, repeats while held).
            if arrows_ax is not None and arrows_ay is not None \
                    and max(arrows_ax, arrows_ay) < js.get_numaxes():
                vx = js.get_axis(arrows_ax)
                vy = js.get_axis(arrows_ay)
                if max(abs(vx), abs(vy)) < arrows_dead:
                    arrows_last_dir = None
                else:
                    if abs(vx) >= abs(vy):
                        direction = "right" if vx > 0 else "left"
                    else:
                        direction = "down" if vy > 0 else "up"  # SDL y+ = down
                    if direction != arrows_last_dir or now - arrows_last_fire >= arrows_repeat:
                        arrows_last_fire = now
                        arrows_last_dir = direction
                        if perform:
                            try:
                                herdr.send_keys(direction)
                            except herdr.HerdrError as exc:
                                print(f"  ! arrow {direction}: {exc}", file=sys.stderr)
                        else:
                            print(f"arrow {direction}  [skipped: do-nothing]")

            # Voice: while active, stream spaces to emulate held-space.
            v_active = v_latched or v_pressing
            if v_active and now - v_last_send >= voice_repeat:
                v_last_send = now
                if perform:
                    try:
                        herdr.send_text(voice_char)
                    except herdr.HerdrError as exc:
                        print(f"  ! voice: {exc}", file=sys.stderr)
                else:
                    print("voice -> send space  [skipped: do-nothing]")

            if now - last_beat >= 3.0:
                last_beat = now
                if events_seen == 0:
                    print("… listening, 0 events. If pressing does nothing, grant "
                          "Input Monitoring (see README).")
            time.sleep(0.008)
    except KeyboardInterrupt:
        print("\nbye")


# ---- background control ----------------------------------------------------

def _read_pid():
    try:
        return int(PIDFILE.read_text().strip())
    except (FileNotFoundError, ValueError):
        return None


def _alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except OSError:
        return False
    return True


def cmd_status() -> None:
    pid = _read_pid()
    if pid and _alive(pid):
        print(f"running (pid {pid}); logs -> {LOGFILE}")
    else:
        if PIDFILE.exists():
            PIDFILE.unlink()
        print("not running")


def cmd_stop() -> None:
    pid = _read_pid()
    if not pid or not _alive(pid):
        print("not running")
        if PIDFILE.exists():
            PIDFILE.unlink()
        return
    os.kill(pid, signal.SIGTERM)
    for _ in range(50):
        if not _alive(pid):
            break
        time.sleep(0.1)
    if PIDFILE.exists():
        PIDFILE.unlink()
    print(f"stopped (pid {pid})")


def start_background(passthrough: list[str]) -> None:
    pid = _read_pid()
    if pid and _alive(pid):
        print(f"already running (pid {pid}). Use --stop first.", file=sys.stderr)
        sys.exit(1)
    args = [sys.executable, str(Path(__file__).resolve()), *passthrough]
    log = open(LOGFILE, "a")
    proc = subprocess.Popen(
        args,
        stdout=log,
        stderr=subprocess.STDOUT,
        stdin=subprocess.DEVNULL,
        start_new_session=(os.name != "nt"),
    )
    PIDFILE.write_text(str(proc.pid))
    print(f"started in background (pid {proc.pid}); logs -> {LOGFILE}\n"
          f"stop with: python {Path(__file__).name} --stop")


# ---- entrypoint ------------------------------------------------------------

def main() -> None:
    ap = argparse.ArgumentParser(description="Drive herdr from a game controller.")
    ap.add_argument("--list", action="store_true", help="list detected controllers")
    ap.add_argument("--calibrate", action="store_true",
                    help="press each control to (re)build the name->index profile")
    ap.add_argument("--discover", action="store_true",
                    help="print raw input events + the name each maps to")
    ap.add_argument("--monitor", action="store_true",
                    help="watch inputs and show the herdr action each maps to")
    ap.add_argument("--perform", action="store_true",
                    help="with --monitor, actually dispatch actions (default: do nothing)")
    ap.add_argument("--dry-run", action="store_true",
                    help="normal run, but log mapped actions without dispatching")
    ap.add_argument("--bg", action="store_true", help="start the run loop detached")
    ap.add_argument("--stop", action="store_true", help="stop the background daemon")
    ap.add_argument("--status", action="store_true", help="report background daemon status")
    args = ap.parse_args()

    if args.status:
        cmd_status()
    elif args.stop:
        cmd_stop()
    elif args.bg:
        passthrough = []
        if args.monitor:
            passthrough.append("--monitor")
        if args.perform:
            passthrough.append("--perform")
        if args.dry_run:
            passthrough.append("--dry-run")
        start_background(passthrough)
    elif args.list:
        list_controllers()
    elif args.calibrate:
        run_calibrate()
    elif args.discover:
        run_discover()
    elif args.monitor:
        run_loop(load_config(), perform=args.perform, label="MONITOR")
    else:
        run_loop(load_config(), perform=not args.dry_run, label="RUNNING")


if __name__ == "__main__":
    main()
