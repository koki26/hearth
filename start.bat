@echo off
rem Starts the Hearth server. Pass --noGUI for a console-only server.
setlocal
cd /d "%~dp0"

if /i "%~1"=="--noGUI" (set TARGET=hearth-server) else (set TARGET=hearth-server-gui)
set EXE=target\release\%TARGET%.exe

if not exist "%EXE%" (
    echo Building %TARGET% ...
    cargo build --release -p %TARGET% || goto :failed
)

"%EXE%" deploy\hearth.toml
if errorlevel 1 goto :failed
exit /b 0

:failed
echo.
echo The server exited with an error.
pause
exit /b 1
