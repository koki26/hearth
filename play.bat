@echo off
rem Starts the Hearth client. If SteamVR is already running it is used, so
rem launching Hearth never pulls the headset out of it. --steamvr insists on
rem SteamVR even when it is not running yet.
setlocal EnableDelayedExpansion
cd /d "%~dp0"

if not exist "dist\Hearth.exe" (
	echo dist\Hearth.exe is missing. Build it first:
	echo     powershell -ExecutionPolicy Bypass -File client\build.ps1 -Export
	pause
	exit /b 1
)

set "ARGS=%*"
set "STEAMVR="
if /i "%~1"=="--steamvr" (
	set "ARGS=!ARGS:--steamvr=!"
	set "STEAMVR=1"
)
tasklist /fi "imagename eq vrmonitor.exe" 2>nul | findstr /i vrmonitor >nul && set "STEAMVR=1"

if defined STEAMVR (
	for /f "tokens=2,*" %%a in ('reg query "HKCU\Software\Valve\Steam" /v SteamPath 2^>nul ^| findstr /i SteamPath') do set "STEAM=%%b"
	set "RUNTIME=!STEAM:/=\!\steamapps\common\SteamVR\steamxr_win64.json"
	if not exist "!RUNTIME!" set "RUNTIME=C:\Program Files (x86)\Steam\steamapps\common\SteamVR\steamxr_win64.json"
	if exist "!RUNTIME!" (
		set "XR_RUNTIME_JSON=!RUNTIME!"
		echo Using SteamVR's OpenXR runtime.
	) else (
		echo SteamVR not found, using the system OpenXR runtime.
	)
)

dist\Hearth.exe !ARGS!
