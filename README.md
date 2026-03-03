# HiveMind

Simple car simulation: cars register with the server, get paths between 3 parking lots, traverse roads, and report x,y coordinates.

## Quick Start

### 1. Start the server

```powershell
cd hive_mind_server
cargo run
```

Server runs on http://0.0.0.0:8080 and loads `../city.json`.

### 2. Spawn 3 cars

In a new terminal:

```powershell
.\spawn-cars.ps1
```

This starts:
- CAR001: lot A → B (port 9001)
- CAR002: lot B → C (port 9002)
- CAR003: lot C → A (port 9003)

### 3. Query positions

```powershell
.\query-positions.ps1
```

Polls `/car-positions` every 2 seconds and displays x,y for each car.

Or manually:

```powershell
Invoke-WebRequest http://localhost:8080/car-positions
```

## Spawn a single car

```powershell
python car/car.py CAR001 9001 A B
#           license port  from to
```

## Check lot coordinates

```powershell
.\check-lots.ps1
```

Fetches `/parking-lots` and displays center and exit coords for A, B, C.

## Endpoints

| Endpoint       | Method | Description                    |
|----------------|--------|--------------------------------|
| /register-car  | POST   | license, url, from, to         |
| /car-positions | GET    | Live poll of all cars' x,y     |
| /parking-lots  | GET    | Lot centers and exits          |
| /health        | GET    | Health check                   |

## Visualization

```powershell
pip install pygame
python viz.py
```

Opens a Pygame window showing roads, parking lots (A, B, C), and car positions. Polls the server every second. Resizable window.

## city.json

- `segments`: road line segments `[[x1,y1], [x2,y2], ...]`
- `parking_lots`: A, B, C with `center` (spawn) and `exit` (road connection)
