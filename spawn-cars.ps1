# Spawn 3 cars: one from each parking lot to the next
# Usage: .\spawn-cars.ps1
# Prereqs: server running (cargo run in hive_mind_server), Python 3

$ErrorActionPreference = "Stop"

$cars = @(
    @{ License = "CAR001"; Port = 9001; From = "A"; To = "B" }
    @{ License = "CAR002"; Port = 9002; From = "B"; To = "C" }
    @{ License = "CAR003"; Port = 9003; From = "C"; To = "A" }
)

foreach ($c in $cars) {
    Write-Host "Spawning $($c.License): $($c.From) -> $($c.To) on port $($c.Port)"
    Start-Process python -ArgumentList "car/car.py", $c.License, $c.Port, $c.From, $c.To
    Start-Sleep -Seconds 2
}

Write-Host "`n3 cars spawned. Query positions: Invoke-WebRequest http://localhost:8080/car-positions"
