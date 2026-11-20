# Builds dist\DiskInfo-windows-<arch>.zip: DiskInfo.exe with the bundled smartctl.exe.
# Usage (PowerShell, on Windows with Rust):
#   packaging\build-windows.ps1 -Target x86_64-pc-windows-msvc -Arch x64 -Smartctl target\smartctl.exe
#   packaging\build-windows.ps1 -Target aarch64-pc-windows-msvc -Arch arm64 -Smartctl target\smartctl.exe
# smartctl.exe comes from packaging/build-smartctl.sh windows-x86_64 (built with mingw-w64);
# Windows on ARM runs that x64 build through its built-in emulation.
param(
    [Parameter(Mandatory)] [string] $Target,
    [Parameter(Mandatory)] [string] $Arch,
    [Parameter(Mandatory)] [string] $Smartctl
)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

rustup target add $Target
cargo build --release --target $Target
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$stage = "target\DiskInfo-windows-$Arch"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Path "$stage\licenses" | Out-Null
Copy-Item "target\$Target\release\diskinfo.exe" "$stage\DiskInfo.exe"
Copy-Item $Smartctl "$stage\smartctl.exe"
Copy-Item "LICENSE" "$stage\licenses\LICENSE-DiskInfo.txt"
Copy-Item "LICENSE-CrystalDiskInfo.txt" "$stage\licenses\"
Copy-Item "target\smartmontools-license\COPYING" "$stage\licenses\COPYING-smartmontools.txt"
Set-Content -Path "$stage\LEEME - README.txt" -Encoding UTF8 -Value @"
DiskInfo
========
ES: Abre DiskInfo.exe. Para ver la salud de los discos pulsa "Reiniciar como administrador".
    Deja smartctl.exe en la misma carpeta que DiskInfo.exe.
EN: Open DiskInfo.exe. To see disk health, press "Restart as administrator".
    Keep smartctl.exe in the same folder as DiskInfo.exe.

smartctl.exe: smartmontools 7.5 (GPL-2.0-or-later), unmodified.
Source: https://github.com/smartmontools/smartmontools/releases/tag/RELEASE_7_5
"@

New-Item -ItemType Directory -Force -Path dist | Out-Null
$zip = "dist\DiskInfo-windows-$Arch.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path "$stage\*" -DestinationPath $zip
Write-Host "Built $zip"
