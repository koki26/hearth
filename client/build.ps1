# Builds the GDExtension for the Godot project. With -Export it also exports
# the game to dist/Hearth.exe and the world builder to dist/HearthStudio.exe.
# With -Test it runs the world format checks headless.
param(
	[switch]$Release,
	[switch]$Export,
	[switch]$Test,
	[switch]$InstallTemplates,
	[string]$Godot = $(if ($env:GODOT) { $env:GODOT } else { "godot" })
)
# Native tools write progress to stderr, which "Stop" would treat as fatal.
$ErrorActionPreference = "Continue"
$root = Resolve-Path "$PSScriptRoot/.."
# Fail before anything else: a missing Godot used to get as far as deleting dist/.
if (-not (Get-Command $Godot -ErrorAction SilentlyContinue)) {
	throw "Godot not found ('$Godot'). Put it on PATH, set GODOT to the *_console.exe, or pass -Godot <path>."
}

# libopus is built from source with cmake, which is not on PATH after a default
# install, and which otherwise picks a Visual Studio generator it cannot find.
if (-not (Get-Command cmake -ErrorAction SilentlyContinue) -and (Test-Path "$env:ProgramFiles/CMake/bin/cmake.exe")) {
	$env:PATH = "$env:ProgramFiles/CMake/bin;$env:PATH"
}
if (-not $env:CMAKE_GENERATOR -and (Test-Path "$env:ProgramFiles/Microsoft Visual Studio/2022")) {
	$env:CMAKE_GENERATOR = "Visual Studio 17 2022"
}
$build = if ($Release -or $Export) { "release" } else { "debug" }

cargo build -p hearth-client @(if ($build -eq "release") { "--release" })
if ($LASTEXITCODE -ne 0) { throw "cargo build failed." }
New-Item -ItemType Directory -Force "$PSScriptRoot/godot/bin" | Out-Null
Copy-Item "$root/target/$build/hearth_client.dll" "$PSScriptRoot/godot/bin/" -Force
Write-Host "Copied the $build extension to client/godot/bin."

# --import so any new class_name is registered, then regenerate the 2D theme
# from the tokens in ui.gd, so it can never drift from them (docs/UI_STYLE.md).
& $Godot --headless --path "$PSScriptRoot/godot" --import
& $Godot --headless --path "$PSScriptRoot/godot" --script res://tools/make_theme.gd

if ($Test) {
	# Every tests/*_test.gd, so adding a suite needs no change here.
	# Not $test: PowerShell variables are case-insensitive, so that is the
	# -Test switch above, and assigning a file to it silently skipped the whole
	# loop -- which is how this stopped running the tests without saying so.
	foreach ($suite in (Get-ChildItem "$PSScriptRoot/godot/tests" -Filter "*_test.gd" | Sort-Object Name)) {
		& $Godot --headless --path "$PSScriptRoot/godot" --script "res://tests/$($suite.Name)"
		if ($LASTEXITCODE -ne 0) { throw "$($suite.Name) failed." }
		Write-Host "$($suite.Name) passed."
	}
}
if (-not $Export) { return }

# Templates are named "4.7.2.stable" and released as tag "4.7.2-stable".
# shortcut: stable releases only, rerun with a -Godot pointing at a stable build for rc/beta.
$version = (& $Godot --version | Select-Object -First 1).Trim()
$templates = (($version -split "\.")[0..3]) -join "."
$dir = "$env:APPDATA/Godot/export_templates/$templates"
if (-not (Test-Path $dir)) {
	if (-not $InstallTemplates) {
		throw "Export templates for $templates are missing. Install them in Godot (Editor > Manage Export Templates), or rerun with -InstallTemplates to download them (about 1 GB)."
	}
	$tag = $templates -replace "\.stable$", "-stable"
	# A .tpz is a zip, but Expand-Archive only accepts the .zip extension.
	$zip = "$env:TEMP/godot-templates-$templates.zip"
	$unpacked = "$env:TEMP/godot-templates-$templates"
	if (-not (Test-Path $zip)) {
		Write-Host "Downloading export templates for $templates (about 1 GB) ..."
		Invoke-WebRequest "https://github.com/godotengine/godot/releases/download/$tag/Godot_v${tag}_export_templates.tpz" -OutFile $zip -ErrorAction Stop
	}
	Write-Host "Unpacking export templates ..."
	Expand-Archive $zip $unpacked -Force
	New-Item -ItemType Directory -Force $dir | Out-Null
	Move-Item "$unpacked/templates/*" $dir -Force
	Remove-Item $zip, $unpacked -Recurse -Force
}

New-Item -ItemType Directory -Force "$root/dist" | Out-Null
# Delete the old exes first: otherwise a failed export leaves yesterday's build
# in place and the Test-Path checks below happily pass, which means playing
# against a current server with a client that speaks an older protocol.
Remove-Item "$root/dist/Hearth.exe", "$root/dist/HearthStudio.exe" -Force -ErrorAction SilentlyContinue
# Exporting two 110 MB exes back to back intermittently fails to rename the
# second one, leaving a .tmp behind and no exe, because Windows is usually
# still scanning the first. One retry is enough, and is far cheaper than
# leaving people to work out why half their build is missing.
function Export-Preset($preset, $exe) {
	foreach ($attempt in 1, 2) {
		& $Godot --headless --path "$PSScriptRoot/godot" --export-release $preset
		if (Test-Path $exe) { return }
		Write-Host "$preset did not produce $exe; retrying once."
		Remove-Item ([IO.Path]::ChangeExtension($exe, ".tmp")) -Force -ErrorAction SilentlyContinue
		Start-Sleep -Seconds 3
	}
	throw "$preset export failed, see the Godot output above."
}

# Two exes from one project: the game, and the world builder, which is the
# same pack with the "studio" feature tag (docs/decisions.md D26).
Export-Preset "Windows Desktop" "$root/dist/Hearth.exe"
Export-Preset "Windows Studio" "$root/dist/HearthStudio.exe"
# Godot's own copy of the extension fails while the editor holds it open, so
# copy it over the top rather than make people close the editor.
Copy-Item "$PSScriptRoot/godot/bin/hearth_client.dll" "$root/dist/" -Force
Write-Host "Exported dist/Hearth.exe and dist/HearthStudio.exe."
