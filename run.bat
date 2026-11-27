@echo off
setlocal
cd /d "%~dp0"

where cargo >nul 2>nul
if errorlevel 1 (
  echo cargo is not installed. Install Rust 1.100 or newer first.
  pause
  exit /b 1
)

where trunk >nul 2>nul
if errorlevel 1 (
  echo trunk is not installed. Install it with: cargo install trunk
  pause
  exit /b 1
)

echo Building and serving the frontend on port 1420...
start "Barry trunk" /b trunk serve --port 1420

echo Waiting for the frontend to answer...
powershell -NoProfile -Command "$deadline = (Get-Date).AddMinutes(8); while ((Get-Date) -lt $deadline) { try { $client = New-Object Net.Sockets.TcpClient; $client.Connect('127.0.0.1', 1420); $client.Close(); exit 0 } catch { Start-Sleep -Milliseconds 500 } }; exit 1"
if errorlevel 1 (
  echo The frontend server did not start on port 1420.
  taskkill /F /T /IM trunk.exe >nul 2>nul
  pause
  exit /b 1
)

echo Building the desktop app...
cargo build -p bh-app
if errorlevel 1 (
  echo The desktop app could not be built.
  taskkill /F /T /IM trunk.exe >nul 2>nul
  pause
  exit /b 1
)

tasklist /FI "IMAGENAME eq barry.exe" 2>nul | find /I "barry.exe" >nul
if not errorlevel 1 (
  echo Barry Harness is already running. Close the open window, then run this again.
  taskkill /F /T /IM trunk.exe >nul 2>nul
  pause
  exit /b 1
)

echo Binary: target\debug\barry.exe
for %%F in (target\debug\barry.exe) do echo Built:  %%~tF
echo Starting Barry Harness. Closing this window closes the app and the server.
target\debug\barry.exe

taskkill /F /T /IM trunk.exe >nul 2>nul
endlocal
