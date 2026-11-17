# One-time setup for Tiny Time Track Lite on a new computer.
# Right-click this file -> "Run with PowerShell".
# Creates a private Python environment in .\.venv and installs the app's libraries.
# Needs Python 3.11 or newer (https://www.python.org/downloads/).

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
$ok = $false

function Find-Python {
    # Try the Windows "py" launcher first, then "python" on PATH. Each candidate must
    # really run and be 3.11+ (the Microsoft Store "python" stub fails this test).
    $candidates = @(@("py", "-3"), @("python"))
    foreach ($candidate in $candidates) {
        $exe = $candidate[0]
        $extra = @($candidate | Select-Object -Skip 1)
        if (-not (Get-Command $exe -ErrorAction SilentlyContinue)) { continue }
        try {
            $answer = & $exe @extra -c "import sys; print('OK' if sys.version_info >= (3, 11) else 'OLD', '%d.%d' % sys.version_info[:2])" 2>$null
        } catch { continue }
        if ($answer -match '^OK (\S+)') { return @{ Exe = $exe; Args = $extra; Version = $Matches[1] } }
        if ($answer -match '^OLD (\S+)') { Write-Host "Found Python $($Matches[1]) via '$exe', but 3.11 or newer is needed." -ForegroundColor Yellow }
    }
    return $null
}

try {
    Write-Host "Tiny Time Track Lite - setup" -ForegroundColor Cyan
    $python = Find-Python
    if (-not $python) {
        throw "Python 3.11 or newer wasn't found. Install it from https://www.python.org/downloads/ (tick 'Add python.exe to PATH'), then run this setup again."
    }
    Write-Host "Using Python $($python.Version)."

    if (-not (Test-Path ".venv\Scripts\python.exe")) {
        Write-Host "Creating the app's Python environment (.venv)..."
        & $python.Exe @($python.Args) -m venv .venv
        if ($LASTEXITCODE -ne 0) { throw "Couldn't create the Python environment." }
    }
    Write-Host "Installing libraries (this can take a minute)..."
    & .\.venv\Scripts\python.exe -m pip install --quiet --disable-pip-version-check -r requirements.txt
    if ($LASTEXITCODE -ne 0) { throw "Installing the libraries failed - check your internet connection and try again." }

    & .\.venv\Scripts\python.exe -c "import ttlite.web" 2>$null
    if ($LASTEXITCODE -ne 0) { throw "The app didn't load after installing. Try deleting the .venv folder and running setup again." }

    Write-Host ""
    Write-Host "Done! Double-click 'Start Tiny Time Track Lite.bat' to start the app." -ForegroundColor Green
    $ok = $true
}
catch {
    Write-Host ""
    Write-Host "Setup didn't finish: $($_.Exception.Message)" -ForegroundColor Red
}
finally {
    Write-Host ""
    Read-Host "Press Enter to close this window"
}
if (-not $ok) { exit 1 }
