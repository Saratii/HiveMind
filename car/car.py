#!/usr/bin/env python3
"""
HiveMind car emulator.
- HTTP server: GET /position, POST /command
- Updates position from speed and direction (dead reckoning)
- Registers with server on startup
"""

import argparse
import math
import threading
import time
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import parse_qs, urlparse

# Car state
x = 0.0
y = 0.0
dir_x = 1.0
dir_y = 0.0
speed = 0.0
license = "CAR001"
run = True

# Reject position updates outside map (never warp car off roads)
MAP_X_MIN, MAP_X_MAX = -500, 1500
MAP_Y_MIN, MAP_Y_MAX = -600, 600


def sim_loop():
    """Update position every 50ms based on speed and direction."""
    global x, y
    last = time.monotonic()
    while run:
        time.sleep(0.05)
        now = time.monotonic()
        dt = now - last
        last = now
        if speed > 0.001:
            x += dir_x * speed * dt
            y += dir_y * speed * dt


class CarHandler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass  # quiet

    def do_GET(self):
        if self.path == "/position":
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(f"x={x:.6f}&y={y:.6f}".encode())
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self):
        global x, y, dir_x, dir_y, speed
        if self.path == "/command":
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length).decode() if length else ""
            params = parse_qs(body)

            def get(k, default=""):
                return params.get(k, [default])[0]

            cmd = get("type", "set_route")
            if cmd == "stop":
                speed = 0.0
            elif cmd == "set_route":
                try:
                    speed = float(get("speed", "0"))
                    dx = float(get("direction_x", "1"))
                    dy = float(get("direction_y", "0"))
                    mag = math.hypot(dx, dy)
                    if mag > 1e-9:
                        dir_x = dx / mag
                        dir_y = dy / mag
                    if "pos_x" in params and "pos_y" in params:
                        px = float(get("pos_x", str(x)))
                        py = float(get("pos_y", str(y)))
                        if MAP_X_MIN <= px <= MAP_X_MAX and MAP_Y_MIN <= py <= MAP_Y_MAX:
                            x, y = px, py
                except ValueError:
                    pass

            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(b"ok")
        else:
            self.send_response(404)
            self.end_headers()


def register(server_url: str, car_url: str, from_lot: str, to_lot: str):
    import urllib.request

    body = (
        f"license={license}&url={car_url}&from={from_lot}&to={to_lot}"
    ).encode()
    req = urllib.request.Request(
        f"{server_url}/register-car",
        data=body,
        method="POST",
        headers={"Content-Type": "application/x-www-form-urlencoded"},
    )
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            print(f"[{license}] Registered: {from_lot} -> {to_lot}")
    except Exception as e:
        print(f"[{license}] Registration failed: {e}")


def main():
    global x, y, dir_x, dir_y, license, run
    parser = argparse.ArgumentParser(description="HiveMind car emulator")
    parser.add_argument("license", nargs="?", default="CAR001")
    parser.add_argument("port", nargs="?", type=int, default=9001)
    parser.add_argument("from_lot", nargs="?", default="A", help="spawn parking lot A|B|C")
    parser.add_argument("to_lot", nargs="?", default="B", help="destination parking lot A|B|C")
    parser.add_argument("--server", default="http://127.0.0.1:8080")
    parser.add_argument("--no-register", action="store_true")
    args = parser.parse_args()

    license = args.license
    port = args.port

    # Lot centers (match city.json)
    lot_centers = {
        "A": (900, -500),
        "B": (-300, 500),
        "C": (200, -220),
    }
    x, y = lot_centers.get(args.from_lot, (0, 0))
    # Keep speed=0 until server sends first command (avoids wrong initial direction)
    dir_x, dir_y = 1.0, 0.0

    sim = threading.Thread(target=sim_loop, daemon=True)
    sim.start()

    # Start HTTP server FIRST so we're listening before server sends commands
    server = HTTPServer(("0.0.0.0", port), CarHandler)
    server_thread = threading.Thread(target=server.serve_forever, daemon=False)
    server_thread.start()
    time.sleep(1.2)  # ensure server is listening before server sends commands

    if not args.no_register:
        car_url = f"http://127.0.0.1:{port}"
        register(args.server, car_url, args.from_lot, args.to_lot)

    print(f"[{license}] Listening on port {port}, {args.from_lot} -> {args.to_lot}")
    try:
        server_thread.join()
    except KeyboardInterrupt:
        run = False
        server.shutdown()


if __name__ == "__main__":
    main()
