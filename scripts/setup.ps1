# ES TimeTrack - one-shot setup for a fresh machine.
# Run from the project root:   .\scripts\setup.ps1
# Idempotent: safe to run again. Creates the venv, installs dependencies,
# and writes a .env with a real secret key if one doesn't exist yet.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
Write-Host "ES TimeTrack setup - $root" -ForegroundColor Cyan

# 1. Find a suitable Python (3.11+).
function Get-Python {
    foreach ($cmd in @("py -3.12", "py -3.11", "py -3", "python")) {
        $parts = $cmd.Split(" ")
        try {
            $v = & $parts[0] $parts[1..($parts.Length-1)] --version 2>$null
            if ($v -match "Python (\d+)\.(\d+)") {
                if ([int]$Matches[1] -gt 3 -or ([int]$Matches[1] -eq 3 -and [int]$Matches[2] -ge 11)) {
                    return $cmd
                }
            }
        } catch {}
    }
    return $null
}

$py = Get-Python
if (-not $py) {
    Write-Host "ERROR: Python 3.11+ not found." -ForegroundColor Red
    Write-Host "Install it from https://www.python.org/downloads/ (tick 'Add to PATH')," -ForegroundColor Yellow
    Write-Host "or run:  winget install --id Python.Python.3.12 --source winget" -ForegroundColor Yellow
    exit 1
}
Write-Host "Using Python: $py" -ForegroundColor Green

# 2. Create the virtual environment.
if (-not (Test-Path ".\.venv\Scripts\python.exe")) {
    Write-Host "Creating virtual environment (.venv)..."
    $parts = $py.Split(" ")
    & $parts[0] $parts[1..($parts.Length-1)] -m venv .venv
} else {
    Write-Host ".venv already exists - reusing."
}
$venvPy = ".\.venv\Scripts\python.exe"

# 3. Install dependencies.
Write-Host "Installing dependencies..."
& $venvPy -m pip install --upgrade pip --quiet
& $venvPy -m pip install -r requirements.txt

# 4. Create .env with a generated secret key if missing.
if (-not (Test-Path ".env")) {
    Write-Host "Creating .env with a generated secret key..."
    Copy-Item .env.example .env
    $key = & $venvPy -c "import secrets; print(secrets.token_hex(32))"
    (Get-Content .env) -replace 'TIMETRACK_SECRET_KEY=.*', "TIMETRACK_SECRET_KEY=$key" |
        Set-Content .env -Encoding utf8
    Write-Host "Wrote .env (edit it to set TIMETRACK_DB_PATH and TIMETRACK_BACKUP_PATH for the server)."
} else {
    Write-Host ".env already exists - leaving it untouched."
}

Write-Host ""
Write-Host "Setup complete." -ForegroundColor Green
Write-Host "Next steps:" -ForegroundColor Cyan
Write-Host "  1. Create the first manager account:" -ForegroundColor Cyan
Write-Host "       $venvPy -m app.cli create-manager --name `"Your Name`" --email you@es.co.za"
Write-Host "  2. Start the server:" -ForegroundColor Cyan
Write-Host "       .\scripts\run.ps1"
Write-Host "  3. Open http://localhost:8000"
