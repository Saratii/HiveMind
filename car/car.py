#!/usr/bin/env python3
"""
HiveMind car emulator.
- GET /position, GET /state, POST /command
- State: at_center -> to_entrance -> waiting_entrance -> to_road -> on_road -> to_dest_center -> parked
- Stops within 5 units of entrance/road point/lot center.
"""

import argparse
import math
import threading
import time
from http.server import HTTPServer, BaseHTTPRequestHandler
from urllib.parse import parse_qs

# Car state
x = 0.0
y = 0.0
dir_x = 1.0
dir_y = 0.0
speed = 0.0
license = "CAR001"
run = True
goal_x = None
goal_y = None
_state = "at_center"

MAP_X_MIN, MAP_X_MAX = -1500, 1500
MAP_Y_MIN, MAP_Y_MAX = -1500, 1500
ARRIVAL_DIST = 5.0
LOT_SPEED = 50.0


def clear_goal():
    global goal_x, goal_y
    goal_x = None
    goal_y = None


def set_goal(gx: float, gy: float):
    global goal_x, goal_y, dir_x, dir_y, speed
    goal_x = gx
    goal_y = gy
    dx = gx - x
    dy = gy - y
    dist = math.hypot(dx, dy)
    if dist > 1e-9:
        dir_x = dx / dist
        dir_y = dy / dist
    speed = LOT_SPEED


def sim_loop():
    global x, y, _state, speed, goal_x, goal_y
    last = time.monotonic()
    while run:
        time.sleep(0.05)
        now = time.monotonic()
        dt = now - last
        last = now
        if speed > 0.001:
            x += dir_x * speed * dt
            y += dir_y * speed * dt
        if goal_x is not None and goal_y is not None:
            dist = math.hypot(x - goal_x, y - goal_y)
            # Stop when within range, or when we've overshot (passed the goal along our direction)
            dx_to_goal = goal_x - x
            dy_to_goal = goal_y - y
            overshot = (dx_to_goal * dir_x + dy_to_goal * dir_y) <= 0  # moving away from goal
            if dist <= ARRIVAL_DIST or (overshot and _state == "to_dest_center"):
                if _state == "to_entrance":
                    _state = "waiting_entrance"
                    speed = 0.0
                elif _state == "to_road":
                    _state = "on_road"
                    speed = 0.0
                elif _state == "to_dest_center":
                    _state = "parked"
                    speed = 0.0
                clear_goal()


class CarHandler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def do_GET(self):
        if self.path == "/position":
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(f"x={x:.6f}&y={y:.6f}".encode())
        elif self.path == "/state":
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(f"state={_state}&x={x:.6f}&y={y:.6f}".encode())
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self):
        global x, y, dir_x, dir_y, speed, _state, goal_x, goal_y
        if self.path == "/command":
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length).decode() if length else ""
            params = parse_qs(body)

            def get(k, default=""):
                return params.get(k, [default])[0]

            cmd = get("type", "set_route")
            if cmd == "stop":
                speed = 0.0
                clear_goal()
            elif cmd == "drive_to_entrance":
                try:
                    ex = float(get("entrance_x"))
                    ey = float(get("entrance_y"))
                    _state = "to_entrance"
                    set_goal(ex, ey)
                except ValueError:
                    pass
            elif cmd == "enter_roadway":
                try:
                    rx = float(get("road_x"))
                    ry = float(get("road_y"))
                    _state = "to_road"
                    set_goal(rx, ry)
                except ValueError:
                    pass
            elif cmd == "go_to_lot_center":
                try:
                    cx = float(get("center_x"))
                    cy = float(get("center_y"))
                    _state = "to_dest_center"
                    set_goal(cx, cy)
                except ValueError:
                    pass
            elif cmd == "set_route":
                try:
                    speed = float(get("speed", "0"))
                    dx = float(get("direction_x", "1"))
                    dy = float(get("direction_y", "0"))
                    mag = math.hypot(dx, dy)
                    if mag > 1e-9:
                        dir_x = dx / mag
                        dir_y = dy / mag
                    clear_goal()
                    _state = "on_road"
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
    body = f"license={license}&url={car_url}&from={from_lot}&to={to_lot}".encode()
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
    global x, y, dir_x, dir_y, license, run, _state, goal_x, goal_y
    parser = argparse.ArgumentParser(description="HiveMind car emulator")
    parser.add_argument("license", nargs="?", default="CAR001")
    parser.add_argument("port", nargs="?", type=int, default=9001)
    parser.add_argument("from_lot", nargs="?", default="A")
    parser.add_argument("to_lot", nargs="?", default="B")
    parser.add_argument("--server", default="http://127.0.0.1:8080")
    parser.add_argument("--no-register", action="store_true")
    args = parser.parse_args()

    license = args.license
    goal_x = None
    goal_y = None
    # Match city.json lot centers (A–I on spurs only; J removed)
    lot_centers = {
        "A": (-1100.0, 600.0), "B": (-1100.0, 800.0), "C": (-400.0, -50.0), "D": (-800.0, 50.0),
        "E": (-300.0, -650.0), "F": (750.0, 1300.0), "G": (900.0, -450.0), "H": (900.0, -650.0),
        "I": (-300.0, 450.0),
    }
    x, y = lot_centers.get(args.from_lot, (0.0, 0.0))
    dir_x, dir_y = 1.0, 0.0
    _state = "at_center"

    sim = threading.Thread(target=sim_loop, daemon=True)
    sim.start()

    server = HTTPServer(("0.0.0.0", args.port), CarHandler)
    server_thread = threading.Thread(target=server.serve_forever, daemon=False)
    server_thread.start()
    time.sleep(1.2)

    if not args.no_register:
        car_url = f"http://127.0.0.1:{args.port}"
        register(args.server, car_url, args.from_lot, args.to_lot)

    print(f"[{license}] Listening on port {args.port}, {args.from_lot} -> {args.to_lot}")
    try:
        server_thread.join()
    except KeyboardInterrupt:
        run = False
        server.shutdown()


if __name__ == "__main__":
    main()
