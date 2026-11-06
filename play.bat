@echo off
rem Starts the Hearth client. Pass --steamvr to force SteamVR's OpenXR runtime
rem when something else (Virtual Desktop, Oculus) holds the slot.
setlocal EnableDelayedExpansion
cd /d "%~dp0"

if not exist "dist\Hearth.exe" (
	echo dist\Hearth.exe is missing. Build it first:
	echo     powershell -ExecutionPolicy Bypass -File client\build.ps1 -Export
	pause
	exit /b 1
)

set "ARGS=%*"
if /i "%~1"=="--steamvr" (
	set "ARGS=!ARGS:--steamvr=!"
	for /f "tokens=2,*" %%a in ('reg query "HKCU\Software\Valve\Steam" /v SteamPath 2^>nul ^| findstr /i SteamPath') do set "STEAM=%%b"
	set "RUNTIME=!STEAM:/=\!\steamapps\common\SteamVR\steamxr_win64.json"
	if not exist "!RUNTIME!" set "RUNTIME=C:\Program Files (x86)\Steam\steamapps\common\SteamVR\steamxr_win64.json"
	if not exist "!RUNTIME!" (
		echo Could not find SteamVR. Install it from Steam, or start without --steamvr.
		pause
		exit /b 1
	)
	set "XR_RUNTIME_JSON=!RUNTIME!"
	echo Using SteamVR: !RUNTIME!
)

dist\Hearth.exe !ARGS!
