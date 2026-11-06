# Builds the GDExtension for the Godot project. With -Export it also exports
# the game to dist/Hearth.exe.
param(
	[switch]$Release,
	[switch]$Export,
	[switch]$InstallTemplates,
	[string]$Godot = $(if ($env:GODOT) { $env:GODOT } else { "godot" })
)
$ErrorActionPreference = "Stop"
$root = Resolve-Path "$PSScriptRoot/.."
$build = if ($Release -or $Export) { "release" } else { "debug" }

cargo build -p hearth-client @(if ($build -eq "release") { "--release" })
New-Item -ItemType Directory -Force "$PSScriptRoot/godot/bin" | Out-Null
Copy-Item "$root/target/$build/hearth_client.dll" "$PSScriptRoot/godot/bin/" -Force
Write-Host "Copied the $build extension to client/godot/bin."
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
		Invoke-WebRequest "https://github.com/godotengine/godot/releases/download/$tag/Godot_v${tag}_export_templates.tpz" -OutFile $zip
	}
	Write-Host "Unpacking export templates ..."
	Expand-Archive $zip $unpacked -Force
	New-Item -ItemType Directory -Force $dir | Out-Null
	Move-Item "$unpacked/templates/*" $dir -Force
	Remove-Item $zip, $unpacked -Recurse -Force
}

New-Item -ItemType Directory -Force "$root/dist" | Out-Null
& $Godot --headless --path "$PSScriptRoot/godot" --export-release "Windows Desktop"
Write-Host "Exported dist/Hearth.exe."
