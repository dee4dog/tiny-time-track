# Start the ES TimeTrack server (development / manual run).
# For unattended operation on the office server, install it as a Windows
# service instead - see the README "Running as a Windows service" section.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

if (-not (Test-Path ".\.venv\Scripts\python.exe")) {
    Write-Host "No virtual environment found. Run .\scripts\setup.ps1 first." -ForegroundColor Red
    exit 1
}

Write-Host "Starting ES TimeTrack on http://localhost:8000 (Ctrl+C to stop)..." -ForegroundColor Cyan
& ".\.venv\Scripts\python.exe" -m app
