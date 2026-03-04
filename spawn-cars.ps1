# Spawn 2 cars: one A->B, one B->A
# Usage: .\spawn-cars.ps1
# Prereqs: server running (cargo run in hive_mind_server), Python 3

$ErrorActionPreference = "Stop"

$cars = @(
    @{ License = "CAR001"; Port = 9001; From = "A"; To = "B" }
    @{ License = "CAR002"; Port = 9002; From = "B"; To = "A" }
)

foreach ($c in $cars) {
    Write-Host "Spawning $($c.License): $($c.From) -> $($c.To) on port $($c.Port)"
    Start-Process python -ArgumentList "car/car.py", $c.License, $c.Port, $c.From, $c.To
    Start-Sleep -Seconds 2
}

Write-Host "`n2 cars spawned. Query positions: Invoke-WebRequest http://localhost:8080/car-positions"
