#!/usr/bin/env python3
"""
HiveMind demo: visualizes roads from city.json. Spawn cars by clicking Spawn Car, then start lot, then end lot.
Reset removes all active cars. Run from project root. Server must be running (cargo run in hive_mind_server).
"""

import colorsys
import json
import re
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path

try:
    import pygame
except ImportError:
    print("Install pygame: pip install pygame")
    sys.exit(1)

# Paths
ROOT = Path(__file__).resolve().parent
CITY_JSON = ROOT / "city.json"
CAR_SCRIPT = ROOT / "car" / "car.py"
SERVER_URL = "http://127.0.0.1:8080"

# Viz config
POLL_INTERVAL_MS = 500
WINDOW_SIZE = (900, 600)
MARGIN = 60
MARGIN_LEFT = 60
ROAD_COLOR = (80, 80, 80)
ROAD_WIDTH = 10
LOT_COLOR = (60, 120, 220)   # blue
LOT_RADIUS = 30
CAR_RADIUS = 2
# Golden-ratio hue step so each car gets a distinct color (no duplicates even with 100+ cars)
_GOLDEN = 0.618033988749895


def _car_color_for_index(index: int) -> tuple:
    """Return a unique RGB color (0-255) for car index; maximally distinct hues."""
    h = (index * _GOLDEN) % 1.0
    r, g, b = colorsys.hsv_to_rgb(h, 0.85, 1.0)
    return (int(r * 255), int(g * 255), int(b * 255))
BG_COLOR = (30, 35, 45)
TEXT_COLOR = (200, 200, 200)
GRID_COLOR = (50, 55, 65)
# Top-left buttons
SPAWN_BTN_X, SPAWN_BTN_Y, SPAWN_BTN_W, SPAWN_BTN_H = 12, 12, 120, 32


def load_city():
    """Load city from city.json."""
    with open(CITY_JSON) as f:
        return json.load(f)


def world_bounds(city):
    all_pts = []
    for seg in city.get("segments", []):
        all_pts.extend(seg.get("pts", []))
    for lot in city.get("parking_lots", {}).values():
        all_pts.append(lot.get("center", [0, 0]))
        all_pts.append(lot.get("entrance", lot.get("exit", [0, 0])))
    if not all_pts:
        return -100, 100, -100, 100
    xs = [p[0] for p in all_pts]
    ys = [p[1] for p in all_pts]
    pad = 150
    return min(xs) - pad, max(xs) + pad, min(ys) - pad, max(ys) + pad


def to_screen(x, y, x_min, x_max, y_min, y_max, w, h):
    if x_max == x_min:
        sx = w // 2
    else:
        sx = MARGIN_LEFT + (x - x_min) / (x_max - x_min) * (w - MARGIN_LEFT - MARGIN)
    if y_max == y_min:
        sy = h // 2
    else:
        sy = h - MARGIN - (y - y_min) / (y_max - y_min) * (h - 2 * MARGIN)
    return int(sx), int(sy)


def screen_to_lot(sx, sy, city, x_min, x_max, y_min, y_max, w, h):
    """Return lot_id if (sx, sy) is inside a parking lot, else None."""
    lots = city.get("parking_lots", {})
    if not lots:
        return None
    for lot_id, lot in lots.items():
        cx, cy = lot.get("center", [0, 0])
        size = lot.get("size", [40, 40])
        if isinstance(size, (list, tuple)) and len(size) >= 2:
            hw, hh = float(size[0]) / 2, float(size[1]) / 2
            scx, scy = to_screen(cx, cy, x_min, x_max, y_min, y_max, w, h)
            scale_x = (w - MARGIN_LEFT - MARGIN) / (x_max - x_min) if x_max != x_min else 1
            scale_y = (h - 2 * MARGIN) / (y_max - y_min) if y_max != y_min else 1
            sw, sh = hw * 2 * scale_x, hh * 2 * scale_y
            rect = pygame.Rect(scx - sw / 2, scy - sh / 2, sw, sh)
            if rect.collidepoint(sx, sy):
                return lot_id
        else:
            scx, scy = to_screen(cx, cy, x_min, x_max, y_min, y_max, w, h)
            if (sx - scx) ** 2 + (sy - scy) ** 2 <= LOT_RADIUS ** 2:
                return lot_id
    return None


def fetch_car_positions():
    try:
        req = urllib.request.Request(f"{SERVER_URL}/car-positions")
        with urllib.request.urlopen(req, timeout=2) as r:
            text = r.read().decode()
            cars = []
            for line in text.strip().split("\n"):
                if ":" in line and "x=" in line and "y=" in line:
                    license_part, rest = line.split(":", 1)
                    license_part = license_part.strip()
                    m = re.search(r"x=([-\d.]+)\s+y=([-\d.]+)", rest)
                    if m:
                        cars.append((license_part, float(m.group(1)), float(m.group(2))))
            return cars
    except Exception as e:
        return (None, str(e))


def wait_for_server(timeout=5):
    start = time.monotonic()
    while time.monotonic() - start < timeout:
        try:
            urllib.request.urlopen(f"{SERVER_URL}/health", timeout=1)
            return True
        except Exception:
            time.sleep(0.3)
    return False


def kill_processes_on_ports(ports):
    if sys.platform != "win32":
        return
    for port in ports:
        try:
            out = subprocess.run(
                ["netstat", "-ano"],
                capture_output=True,
                text=True,
                timeout=5,
                creationflags=subprocess.CREATE_NO_WINDOW if getattr(subprocess, "CREATE_NO_WINDOW", None) else 0,
            )
            for line in (out.stdout or "").splitlines():
                if f":{port}" in line and "LISTENING" in line:
                    parts = line.split()
                    if parts:
                        pid = parts[-1]
                        if pid.isdigit():
                            subprocess.run(
                                ["taskkill", "/F", "/PID", pid],
                                capture_output=True,
                                timeout=5,
                                creationflags=subprocess.CREATE_NO_WINDOW if getattr(subprocess, "CREATE_NO_WINDOW", None) else 0,
                            )
                            break
        except Exception:
            pass
    time.sleep(1.0)


def remove_all_cars(procs):
    """Tell server to clear all cars, then kill all car processes."""
    try:
        req = urllib.request.Request(
            f"{SERVER_URL}/remove-all-cars",
            data=b"",
            method="POST",
            headers={"Content-Type": "application/x-www-form-urlencoded"},
        )
        urllib.request.urlopen(req, timeout=3)
    except Exception as e:
        print(f"Remove-all failed: {e}", flush=True)
    ports = [port for _lic, port, _ in procs]
    kill_processes_on_ports(ports)
    for _lic, _port, p in procs:
        try:
            p.terminate()
        except Exception:
            pass
    procs.clear()
    print("Reset: all cars removed.", flush=True)


def spawn_car(license_id, port, from_lot, to_lot):
    """Spawn one car with given license, port, from_lot -> to_lot. Returns Popen."""
    kill_processes_on_ports([port])
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0x08000000) if sys.platform == "win32" else 0
    cmd = [sys.executable, str(CAR_SCRIPT), license_id, str(port), from_lot, to_lot]
    # Don't suppress stderr so "Registration failed" from the car is visible
    p = subprocess.Popen(
        cmd,
        cwd=ROOT,
        creationflags=flags,
        stdout=subprocess.DEVNULL,
        stderr=None,  # show in console so user sees e.g. "Registration failed: HTTP Error 400"
    )
    time.sleep(1.0)  # give car process a moment to bind port (reduced from 2.0)
    return p


def _spawn_car_in_background(license_id, port, from_lot, to_lot, procs):
    """Run in a thread: spawn car, wait for registration, append to procs if success."""
    try:
        p = spawn_car(license_id, port, from_lot, to_lot)
        # Poll for registration instead of one long sleep (max ~3s)
        for _ in range(6):
            time.sleep(0.5)
            result = fetch_car_positions()
            if isinstance(result, list) and any(lic == license_id for lic, _x, _y in result):
                procs.append((license_id, port, p))
                print(f"Spawned {license_id}: {from_lot} -> {to_lot}", flush=True)
                return
        try:
            p.terminate()
        except Exception:
            pass
        print(f"Registration failed for {license_id} ({from_lot} -> {to_lot}). No path? Restart server after city.json changes.", flush=True)
    except Exception as e:
        print(f"Spawn failed for {license_id}: {e}", flush=True)


def run_viz(procs, spawn_state, next_car_index):
    """procs: list of (license, port, Popen). spawn_state: [mode, start_lot]. next_car_index: [int] so each spawn gets a unique license/port even when overlapping."""
    w, h = WINDOW_SIZE
    pygame.init()
    screen = pygame.display.set_mode(WINDOW_SIZE, pygame.RESIZABLE)
    pygame.display.set_caption("HiveMind - Spawn cars by picking start and end lots")
    font = pygame.font.Font(None, 28)
    car_label_font = pygame.font.Font(None, 19)  # 2/3 of 28 for smaller car text
    clock = pygame.time.Clock()
    last_poll = 0
    cars = []
    error_msg = None
    car_colors = {}  # license_id -> color; keeps each car's color stable when new cars spawn

    def get_reset_rect():
        return pygame.Rect(w - 110, h - 38, 90, 26)

    running = True
    while running:
        city = load_city()
        x_min, x_max, y_min, y_max = world_bounds(city)
        spawn_mode, start_lot = spawn_state[0], spawn_state[1]
        lots = city.get("parking_lots", {})
        if spawn_mode is None:
            spawn_btn_label = "Spawn Car"
        elif spawn_mode == "start_lot":
            spawn_btn_label = "Click a starting parking lot"
        else:
            spawn_btn_label = f"Pick ending lot (from {start_lot})"
        spawn_btn_w = max(SPAWN_BTN_W, font.size(spawn_btn_label)[0] + 16)
        spawn_rect = pygame.Rect(SPAWN_BTN_X, SPAWN_BTN_Y, spawn_btn_w, SPAWN_BTN_H)

        for event in pygame.event.get():
            if event.type == pygame.QUIT:
                running = False
            elif event.type == pygame.VIDEORESIZE:
                w, h = event.w, event.h
                screen = pygame.display.set_mode((w, h), pygame.RESIZABLE)
            elif event.type == pygame.MOUSEBUTTONDOWN and event.button == 1:
                if get_reset_rect().collidepoint(event.pos):
                    remove_all_cars(procs)
                    continue
                if spawn_rect.collidepoint(event.pos):
                    if spawn_mode is None:
                        spawn_state[0] = "start_lot"
                        spawn_state[1] = None
                    else:
                        spawn_state[0] = None
                        spawn_state[1] = None
                    continue
                # Click on map: treat as lot pick if in spawn mode
                if spawn_mode == "start_lot" and lots:
                    lot_id = screen_to_lot(event.pos[0], event.pos[1], city, x_min, x_max, y_min, y_max, w, h)
                    if lot_id:
                        spawn_state[0] = "end_lot"
                        spawn_state[1] = lot_id
                elif spawn_mode == "end_lot" and lots:
                    lot_id = screen_to_lot(event.pos[0], event.pos[1], city, x_min, x_max, y_min, y_max, w, h)
                    if lot_id and lot_id != start_lot:
                        # Spawn in background so the UI stays responsive (no freeze)
                        n = next_car_index[0]
                        next_car_index[0] += 1
                        license_id = f"CAR{n:03d}"
                        port = 9000 + n
                        spawn_state[0] = None
                        spawn_state[1] = None
                        t = threading.Thread(
                            target=_spawn_car_in_background,
                            args=(license_id, port, start_lot, lot_id, procs),
                            daemon=True,
                        )
                        t.start()

        now = pygame.time.get_ticks()
        if now - last_poll > POLL_INTERVAL_MS:
            last_poll = now
            result = fetch_car_positions()
            if isinstance(result, tuple) and len(result) == 2 and result[0] is None:
                cars = []
                error_msg = result[1]
            else:
                cars = result if isinstance(result, list) else []
                error_msg = None

        screen.fill(BG_COLOR)

        for gx in range(int(x_min), int(x_max) + 1, 100):
            sx, _ = to_screen(gx, 0, x_min, x_max, y_min, y_max, w, h)
            if 0 <= sx <= w:
                pygame.draw.line(screen, GRID_COLOR, (sx, 0), (sx, h), 1)
        for gy in range(int(y_min), int(y_max) + 1, 100):
            _, sy = to_screen(0, gy, x_min, x_max, y_min, y_max, w, h)
            if 0 <= sy <= h:
                pygame.draw.line(screen, GRID_COLOR, (0, sy), (w, sy), 1)

        for seg in city.get("segments", []):
            pts = seg.get("pts", [])
            for i in range(len(pts) - 1):
                p1 = to_screen(pts[i][0], pts[i][1], x_min, x_max, y_min, y_max, w, h)
                p2 = to_screen(pts[i + 1][0], pts[i + 1][1], x_min, x_max, y_min, y_max, w, h)
                pygame.draw.line(screen, ROAD_COLOR, p1, p2, ROAD_WIDTH)

        for lot_id, lot in lots.items():
            cx, cy = lot.get("center", [0, 0])
            size = lot.get("size", [40, 40])
            if isinstance(size, (list, tuple)) and len(size) >= 2:
                hw, hh = float(size[0]) / 2, float(size[1]) / 2
                scx, scy = to_screen(cx, cy, x_min, x_max, y_min, y_max, w, h)
                scale_x = (w - MARGIN_LEFT - MARGIN) / (x_max - x_min) if x_max != x_min else 1
                scale_y = (h - 2 * MARGIN) / (y_max - y_min) if y_max != y_min else 1
                sw, sh = hw * 2 * scale_x, hh * 2 * scale_y
                rect = pygame.Rect(scx - sw / 2, scy - sh / 2, sw, sh)
                pygame.draw.rect(screen, LOT_COLOR, rect, 3)
                label = font.render(lot_id, True, LOT_COLOR)
                screen.blit(label, (scx - label.get_width() // 2, scy - 10))
            else:
                sx, sy = to_screen(cx, cy, x_min, x_max, y_min, y_max, w, h)
                pygame.draw.circle(screen, LOT_COLOR, (sx, sy), LOT_RADIUS, 3)
                label = font.render(lot_id, True, LOT_COLOR)
                screen.blit(label, (sx - label.get_width() // 2, sy - 10))

        for (license_id, cx, cy) in cars:
            sx, sy = to_screen(cx, cy, x_min, x_max, y_min, y_max, w, h)
            if license_id not in car_colors:
                car_colors[license_id] = _car_color_for_index(len(car_colors))
            color = car_colors[license_id]
            pygame.draw.circle(screen, color, (sx, sy), CAR_RADIUS)
            outline = 1 if CAR_RADIUS <= 5 else 2
            pygame.draw.circle(screen, (255, 255, 255), (sx, sy), CAR_RADIUS, outline)
            label = car_label_font.render(license_id, True, color)
            screen.blit(label, (sx - label.get_width() // 2, sy - CAR_RADIUS - 12))

        # Top-left: Spawn Car button or prompt
        pygame.draw.rect(screen, (70, 100, 160), spawn_rect)
        pygame.draw.rect(screen, (120, 150, 220), spawn_rect, 2)
        lbl = font.render(spawn_btn_label, True, (255, 255, 255))
        screen.blit(lbl, (spawn_rect.x + (spawn_rect.w - lbl.get_width()) // 2, spawn_rect.y + 4))

        status = f"Cars: {len(cars)}  |  {SERVER_URL}"
        if error_msg:
            status = f"Server error: {error_msg}"
        elif len(cars) == 0 and len(procs) > 0:
            status = f"Cars: 0 (server has no cars; restart server after city.json changes?)  |  {SERVER_URL}"
        text = font.render(status, True, TEXT_COLOR)
        screen.blit(text, (10, h - 32))

        reset_rect = get_reset_rect()
        pygame.draw.rect(screen, (70, 100, 160), reset_rect)
        pygame.draw.rect(screen, (120, 150, 220), reset_rect, 2)
        reset_label = font.render("Reset", True, (255, 255, 255))
        screen.blit(reset_label, (reset_rect.x + (reset_rect.w - reset_label.get_width()) // 2, reset_rect.y + 4))

        pygame.display.flip()
        clock.tick(30)

    pygame.quit()


def main():
    print("HiveMind demo - Spawn Car (top left), then click start lot, then end lot. Reset removes all cars.")
    print("Checking server...")
    if not wait_for_server():
        print("ERROR: Server not running. Start it first:")
        print("  cd hive_mind_server")
        print("  cargo run")
        sys.exit(1)
    print("Server OK.")
    procs = []  # list of (license, port, Popen)
    spawn_state = [None, None]  # [mode, start_lot]
    next_car_index = [1]  # unique license/port per spawn (incremented when spawn starts)
    try:
        run_viz(procs, spawn_state, next_car_index)
    finally:
        remove_all_cars(procs)


if __name__ == "__main__":
    main()
