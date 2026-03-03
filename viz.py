#!/usr/bin/env python3
"""
HiveMind Pygame visualization.
- Loads city.json (roads, parking lots)
- Polls /car-positions from the server
- Draws roads, lots, and cars; updates every 1 second
"""

import json
import re
import sys
import urllib.request

try:
    import pygame
except ImportError:
    print("Install pygame: pip install pygame")
    sys.exit(1)

# Config
CITY_JSON = "city.json"
SERVER_URL = "http://127.0.0.1:8080"
POLL_INTERVAL_MS = 1000
WINDOW_SIZE = (900, 600)
MARGIN = 60
ROAD_COLOR = (80, 80, 80)
ROAD_WIDTH = 8
LOT_COLOR = (100, 140, 180)
LOT_RADIUS = 25
CAR_COLORS = [(220, 60, 60), (60, 180, 80), (60, 120, 220)]
CAR_RADIUS = 12
BG_COLOR = (30, 35, 45)
TEXT_COLOR = (200, 200, 200)
GRID_COLOR = (50, 55, 65)


def load_city(path: str):
    with open(path) as f:
        return json.load(f)


def world_bounds(city):
    all_pts = []
    for seg in city.get("segments", []):
        all_pts.extend(seg.get("pts", []))
    for lot_id, lot in city.get("parking_lots", {}).items():
        all_pts.append(lot.get("center", [0, 0]))
        all_pts.append(lot.get("exit", [0, 0]))
    if not all_pts:
        return -100, 100, -100, 100
    xs = [p[0] for p in all_pts]
    ys = [p[1] for p in all_pts]
    pad = 80
    return min(xs) - pad, max(xs) + pad, min(ys) - pad, max(ys) + pad


def to_screen(x, y, x_min, x_max, y_min, y_max, w, h):
    if x_max == x_min:
        sx = w // 2
    else:
        sx = MARGIN + (x - x_min) / (x_max - x_min) * (w - 2 * MARGIN)
    if y_max == y_min:
        sy = h // 2
    else:
        # flip y: world y up = screen y down
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


def main():
    city = load_city(CITY_JSON)
    x_min, x_max, y_min, y_max = world_bounds(city)
    w, h = WINDOW_SIZE

    pygame.init()
    screen = pygame.display.set_mode(WINDOW_SIZE, pygame.RESIZABLE)
    pygame.display.set_caption("HiveMind - Car Visualization")
    font = pygame.font.Font(None, 24)
    clock = pygame.time.Clock()
    last_poll = 0
    cars = []
    error_msg = None

    running = True
    while running:
        for event in pygame.event.get():
            if event.type == pygame.QUIT:
                running = False
            elif event.type == pygame.VIDEORESIZE:
                w, h = event.w, event.h
                screen = pygame.display.set_mode((w, h), pygame.RESIZABLE)

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

        # Grid
        for gx in range(int(x_min), int(x_max) + 1, 50):
            sx, _ = to_screen(gx, 0, x_min, x_max, y_min, y_max, w, h)
            if 0 <= sx <= w:
                pygame.draw.line(screen, GRID_COLOR, (sx, 0), (sx, h), 1)
        for gy in range(int(y_min), int(y_max) + 1, 50):
            _, sy = to_screen(0, gy, x_min, x_max, y_min, y_max, w, h)
            if 0 <= sy <= h:
                pygame.draw.line(screen, GRID_COLOR, (0, sy), (w, sy), 1)

        # Roads
        for seg in city.get("segments", []):
            pts = seg.get("pts", [])
            for i in range(len(pts) - 1):
                p1 = to_screen(pts[i][0], pts[i][1], x_min, x_max, y_min, y_max, w, h)
                p2 = to_screen(pts[i + 1][0], pts[i + 1][1], x_min, x_max, y_min, y_max, w, h)
                pygame.draw.line(screen, ROAD_COLOR, p1, p2, ROAD_WIDTH)

        # Parking lots
        for lot_id, lot in city.get("parking_lots", {}).items():
            cx, cy = lot.get("center", [0, 0])
            sx, sy = to_screen(cx, cy, x_min, x_max, y_min, y_max, w, h)
            pygame.draw.circle(screen, LOT_COLOR, (sx, sy), LOT_RADIUS, 2)
            label = font.render(lot_id, True, LOT_COLOR)
            screen.blit(label, (sx - label.get_width() // 2, sy - 8))

        # Cars
        for i, (license_id, cx, cy) in enumerate(cars):
            sx, sy = to_screen(cx, cy, x_min, x_max, y_min, y_max, w, h)
            color = CAR_COLORS[i % len(CAR_COLORS)]
            pygame.draw.circle(screen, color, (sx, sy), CAR_RADIUS)
            pygame.draw.circle(screen, (255, 255, 255), (sx, sy), CAR_RADIUS, 2)
            label = font.render(license_id, True, color)
            screen.blit(label, (sx - label.get_width() // 2, sy - CAR_RADIUS - 14))

        # Status
        status = f"Cars: {len(cars)}  |  Poll: {SERVER_URL}"
        if error_msg:
            status = f"Server error: {error_msg}"
        text = font.render(status, True, TEXT_COLOR)
        screen.blit(text, (10, h - 28))

        pygame.display.flip()
        clock.tick(30)

    pygame.quit()


if __name__ == "__main__":
    main()
