@echo off
setlocal
cd /d "%~dp0"

where trunk >nul 2>nul
if errorlevel 1 (
  echo trunk is not installed. Install it with: cargo install trunk
  pause
  exit /b 1
)

echo Building the frontend and opening it in your browser.
echo Chat, sign-in and saved settings need the desktop app: run run.bat for those.
trunk serve --release --port 8090 --open

endlocal
