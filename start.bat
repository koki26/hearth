@echo off
rem Starts a Flame: one Hearth game server. Pass --noGUI for a console-only one.
setlocal
cd /d "%~dp0"

rem The server hands out worlds from data\worlds. Pack the one that ships with
rem the client, so a brand new server already has a hub to spawn people into.
rem Packed every start, not just the first: a stale hub.hworld would otherwise
rem be handed out for ever after the world was edited. It takes milliseconds.
rem Every world the client ships with, not just the hub, so a brand new server
rem already has somewhere to send people.
if not exist "data\worlds" mkdir "data\worlds"
for /d %%W in ("client\godot\worlds\*") do (
    if exist "%%~fW\world.json" (
        powershell -NoProfile -Command "Compress-Archive -Path '%%~fW/*' -DestinationPath \"$env:TEMP/hearth-%%~nxW.zip\" -Force; Move-Item -Force \"$env:TEMP/hearth-%%~nxW.zip\" 'data/worlds/%%~nxW.hworld'"
        echo Packed %%~nxW into data\worlds\%%~nxW.hworld
    )
)

if /i "%~1"=="--noGUI" (set TARGET=flame) else (set TARGET=flame-gui)
set EXE=target\release\%TARGET%.exe

rem Always let cargo look: a leftover exe from an older protocol cannot talk to
rem a current client, and the build is a no-op when nothing has changed.
where cargo >nul 2>&1 || goto :nocargo
echo Building %TARGET% ...
cargo build --release -p hearth-%TARGET% || goto :failed
goto :run

:nocargo
if not exist "%EXE%" (
    echo %EXE% is missing and cargo is not installed.
    goto :failed
)

:run

"%EXE%" deploy\flame.toml
if errorlevel 1 goto :failed
exit /b 0

:failed
echo.
echo The Flame went out with an error.
pause
exit /b 1
