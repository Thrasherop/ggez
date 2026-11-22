<#
.SYNOPSIS
  Record USB traffic to a file while you change settings in SteelSeries GG.

.DESCRIPTION
  Captures every USB root hub with USBPcap for -Seconds seconds and writes
  captures\<Name>-USBPcap<N>.pcap, one file per hub. USBPcap needs
  administrator rights, so the script relaunches itself elevated (one
  Windows prompt) when needed. Decode the result with tools\decode.ps1.

  The capture includes every device on the hub, keyboards too: do not type
  anything private while it runs.

.EXAMPLE
  .\tools\capture.ps1 -Name remap-button6-to-f13 -Seconds 60
#>
param(
    [Parameter(Mandatory)][string]$Name,
    [int]$Seconds = 60
)
$ErrorActionPreference = 'Stop'

$usbpcap = "$env:ProgramFiles\USBPcap\USBPcapCMD.exe"
if (-not (Test-Path $usbpcap)) { throw "USBPcapCMD not found at $usbpcap - install USBPcap and reboot" }

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Start-Process powershell -Verb RunAs -Wait -WindowStyle Minimized -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"",
        '-Name', "`"$Name`"", '-Seconds', $Seconds)
    return
}

$outDir = Join-Path (Split-Path $PSScriptRoot -Parent) 'captures'
New-Item -ItemType Directory -Force $outDir | Out-Null

# One control device per root hub; capture them all so it does not matter
# which port the mouse or dongle is on.
$hubs = & $usbpcap --extcap-interfaces | ForEach-Object { if ($_ -match 'value=(\\\\\.\\USBPcap\d+)') { $Matches[1] } }
if (-not $hubs) { throw "USBPcap reports no root hubs - reboot after installing it" }

$procs = foreach ($hub in $hubs) {
    $out = Join-Path $outDir ("{0}-{1}.pcap" -f $Name, ($hub -replace '.*\\', ''))
    Remove-Item $out -ErrorAction SilentlyContinue
    Start-Process $usbpcap -PassThru -WindowStyle Hidden -ArgumentList @(
        '-d', $hub, '-o', "`"$out`"", '-A', '--inject-descriptors')
}
Start-Sleep -Seconds $Seconds
$procs | Stop-Process -Force
