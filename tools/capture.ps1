<#
.SYNOPSIS
  Record USB traffic to a file while you change settings in SteelSeries GG.

.DESCRIPTION
  Captures every USBPcap root hub for -Seconds seconds (or until Ctrl+C) into
  captures\<Name>.pcapng. Run from an elevated PowerShell: USBPcap needs
  administrator rights. Decode the result with tools\decode.ps1.

.EXAMPLE
  .\tools\capture.ps1 -Name remap-button6-to-f13 -Seconds 60
#>
param(
    [Parameter(Mandatory)][string]$Name,
    [int]$Seconds = 60
)
$ErrorActionPreference = 'Stop'

$tshark = "$env:ProgramFiles\Wireshark\tshark.exe"
if (-not (Test-Path $tshark)) { throw "tshark not found at $tshark - install Wireshark" }

$outDir = Join-Path (Split-Path $PSScriptRoot -Parent) 'captures'
New-Item -ItemType Directory -Force $outDir | Out-Null
$out = Join-Path $outDir "$Name.pcapng"

# One capture interface per USB root hub; capture them all so it does not
# matter which port the mouse or dongle is on.
$hubs = & $tshark -D | Where-Object { $_ -match 'USBPcap\d+' } | ForEach-Object { $Matches[0] } | Sort-Object -Unique
if (-not $hubs) { throw "No USBPcap interfaces found - install USBPcap, then reboot or replug" }

$ifArgs = $hubs | ForEach-Object { '-i'; "\\.\$_" }
Write-Host "Capturing $($hubs -join ', ') for $Seconds s -> $out"
Write-Host "Make your change in GG now."
& $tshark @ifArgs -a "duration:$Seconds" -w $out -q
Write-Host "Saved $out"
