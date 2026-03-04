#!/usr/bin/env python3
"""
HiveMind demo: visualizes roads from city.json, spawns cars, shows them driving.
Run from project root. Server must already be running (cargo run in hive_mind_server).
"""

import json
import re
import subprocess
import sys
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
ROAD_COLOR = (80, 80, 80)
ROAD_WIDTH = 10
LOT_COLOR = (100, 140, 180)
LOT_RADIUS = 30
CAR_COLORS = [(220, 60, 60), (60, 180, 80)]
CAR_RADIUS = 14
BG_COLOR = (30, 35, 45)
TEXT_COLOR = (200, 200, 200)
GRID_COLOR = (50, 55, 65)


def load_city():
    with open(CITY_JSON) as f:
        return json.load(f)


def world_bounds(city):
    all_pts = []
    for seg in city.get("segments", []):
        all_pts.extend(seg.get("pts", []))
    for lot in city.get("parking_lots", {}).values():
        all_pts.append(lot.get("center", [0, 0]))
        all_pts.append(lot.get("exit", [0, 0]))
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
        sx = MARGIN + (x - x_min) / (x_max - x_min) * (w - 2 * MARGIN)
    if y_max == y_min:
        sy = h // 2
    else:
        sy = h - MARGIN - (y - y_min) / (y_max - y_min) * (h - 2 * MARGIN)
    return int(sx), int(sy)


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
    """Kill any process listening on the given ports (Windows: netstat + taskkill)."""
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

def reset_cars_to_start():
    """Ask server to reset all cars to start and restart their routes."""
    req = urllib.request.Request(
        f"{SERVER_URL}/reset-car",
        data=b"",
        method="POST",
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    try:
        urllib.request.urlopen(req, timeout=3)
        print("Reset: cars restarted at start.", flush=True)
    except Exception as e:
        print(f"Reset failed: {e}", flush=True)


def spawn_cars():
    """Spawn 2 cars in background: one A->B, one B->A."""
    kill_processes_on_ports([9001, 9002])
    cars = [
        ("CAR001", 9001, "A", "B"),
        ("CAR002", 9002, "B", "A"),
    ]
    procs = []
    flags = getattr(subprocess, "CREATE_NO_WINDOW", 0x08000000) if sys.platform == "win32" else 0
    for license_id, port, from_lot, to_lot in cars:
        cmd = [sys.executable, str(CAR_SCRIPT), license_id, str(port), from_lot, to_lot]
        p = subprocess.Popen(
            cmd,
            cwd=ROOT,
            creationflags=flags,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        procs.append(p)
        time.sleep(2.0)
    return procs


def run_viz(procs):
    city = load_city()
    x_min, x_max, y_min, y_max = world_bounds(city)
    w, h = WINDOW_SIZE

    pygame.init()
    screen = pygame.display.set_mode(WINDOW_SIZE, pygame.RESIZABLE)
    pygame.display.set_caption("HiveMind - Road Demo")
    font = pygame.font.Font(None, 28)
    clock = pygame.time.Clock()
    last_poll = 0
    cars = []
    error_msg = None

    def get_reset_rect():
        return pygame.Rect(w - 110, h - 38, 90, 26)

    running = True
    while running:
        for event in pygame.event.get():
            if event.type == pygame.QUIT:
                running = False
            elif event.type == pygame.VIDEORESIZE:
                w, h = event.w, event.h
                screen = pygame.display.set_mode((w, h), pygame.RESIZABLE)
            elif event.type == pygame.MOUSEBUTTONDOWN and event.button == 1:
                if get_reset_rect().collidepoint(event.pos):
                    reset_cars_to_start()

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
                for lic, cx, cy in cars:
                    print(f"  {lic}: x={cx:.1f}  y={cy:.1f}", flush=True)

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

        for lot_id, lot in city.get("parking_lots", {}).items():
            cx, cy = lot.get("center", [0, 0])
            sx, sy = to_screen(cx, cy, x_min, x_max, y_min, y_max, w, h)
            pygame.draw.circle(screen, LOT_COLOR, (sx, sy), LOT_RADIUS, 3)
            label = font.render(lot_id, True, LOT_COLOR)
            screen.blit(label, (sx - label.get_width() // 2, sy - 10))

        for i, (license_id, cx, cy) in enumerate(cars):
            sx, sy = to_screen(cx, cy, x_min, x_max, y_min, y_max, w, h)
            color = CAR_COLORS[i % len(CAR_COLORS)]
            pygame.draw.circle(screen, color, (sx, sy), CAR_RADIUS)
            pygame.draw.circle(screen, (255, 255, 255), (sx, sy), CAR_RADIUS, 2)
            label = font.render(license_id, True, color)
            screen.blit(label, (sx - label.get_width() // 2, sy - CAR_RADIUS - 18))

        status = f"Cars: {len(cars)}  |  Roads from city.json  |  {SERVER_URL}"
        if error_msg:
            status = f"Server error: {error_msg}"
        text = font.render(status, True, TEXT_COLOR)
        screen.blit(text, (10, h - 32))

        # Reset button
        reset_rect = get_reset_rect()
        pygame.draw.rect(screen, (70, 100, 160), reset_rect)
        pygame.draw.rect(screen, (120, 150, 220), reset_rect, 2)
        reset_label = font.render("Reset", True, (255, 255, 255))
        screen.blit(reset_label, (reset_rect.x + (reset_rect.w - reset_label.get_width()) // 2, reset_rect.y + 4))

        pygame.display.flip()
        clock.tick(30)

    pygame.quit()


def main():
    print("HiveMind demo")
    print("Checking server...")
    if not wait_for_server():
        print("ERROR: Server not running. Start it first:")
        print("  cd hive_mind_server")
        print("  cargo run")
        sys.exit(1)
    print("Server OK. Spawning 2 cars (CAR001 A->B, CAR002 B->A)...")

    procs = spawn_cars()
    time.sleep(2.5)
    print("Car spawned. Starting visualization. Coordinate updates below:\n")

    time.sleep(1)

    try:
        run_viz(procs)
    finally:
        for p in procs:
            try:
                p.terminate()
            except Exception:
                pass


if __name__ == "__main__":
    main()
