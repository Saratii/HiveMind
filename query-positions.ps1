# Poll and display car positions every 2 seconds
# Usage: .\query-positions.ps1

while ($true) {
    Clear-Host
    Write-Host "=== HiveMind Car Positions $(Get-Date -Format 'HH:mm:ss') ===" -ForegroundColor Cyan
    try {
        $r = Invoke-WebRequest -Uri "http://localhost:8080/car-positions" -UseBasicParsing -TimeoutSec 2
        $r.Content | ForEach-Object { $_ -split "`n" } | ForEach-Object { Write-Host $_ }
    } catch {
        Write-Host "Server not reachable. Is it running?" -ForegroundColor Yellow
    }
    Start-Sleep -Seconds 2
}
