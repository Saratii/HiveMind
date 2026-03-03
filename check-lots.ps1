# Fetch and display parking lot coordinates from the server
# Usage: .\check-lots.ps1
# Prereq: server running (cargo run in hive_mind_server)

try {
    $r = Invoke-WebRequest -Uri "http://localhost:8080/parking-lots" -UseBasicParsing
    Write-Host "=== Parking Lot Coordinates ===" -ForegroundColor Cyan
    $r.Content | ForEach-Object { $_ -split "`n" } | ForEach-Object { Write-Host $_ }
} catch {
    Write-Host "Server not reachable. Start it with: cd hive_mind_server; cargo run" -ForegroundColor Yellow
}
