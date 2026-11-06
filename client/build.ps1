# Builds the GDExtension and copies it where the Godot project expects it.
param([switch]$Release)
$ErrorActionPreference = "Stop"
$profileArgs = if ($Release) { @("--release") } else { @() }
cargo build -p hearth-client @profileArgs
$dir = if ($Release) { "release" } else { "debug" }
New-Item -ItemType Directory -Force "$PSScriptRoot/godot/bin" | Out-Null
Copy-Item "$PSScriptRoot/../target/$dir/hearth_client.dll" "$PSScriptRoot/godot/bin/" -Force
Write-Host "copied $dir build to client/godot/bin"
