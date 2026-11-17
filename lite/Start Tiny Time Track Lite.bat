@echo off
rem Double-click to start Tiny Time Track Lite. It opens in your web browser.
rem Keep the black window open while you work; close it to stop the app.
setlocal
cd /d "%~dp0"
title Tiny Time Track Lite

set "PY=%~dp0.venv\Scripts\python.exe"
if not exist "%PY%" set "PY=%~dp0..\.venv\Scripts\python.exe"
if not exist "%PY%" (
  echo Tiny Time Track Lite is not set up yet.
  echo Right-click setup.ps1 and choose "Run with PowerShell" first.
  pause
  exit /b 1
)

"%PY%" -m ttlite %*
if errorlevel 1 pause
