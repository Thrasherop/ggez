<#
.SYNOPSIS
  Record USB traffic to a file while you change settings in SteelSeries GG.

.DESCRIPTION
  Captures every USB root hub with USBPcap for -Seconds seconds, then keeps
  only the traffic of SteelSeries devices in captures\<Name>-USBPcap<N>.pcap
  (one file per hub that has one). USBPcap needs administrator rights, so
  the script relaunches itself elevated (one Windows prompt) when needed.
  Decode the result with tools\decode.ps1.

  The raw recording covers every device on the hub, keyboards too. It is
  deleted once filtered, but avoid typing anything private while it runs.

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

# Record to the system drive first. Writing straight to a USB disk on a
# captured hub records the capture's own writes, growing without bound.
$rawDir = Join-Path $env:SystemRoot "Temp\ggez-capture-$PID"
New-Item -ItemType Directory -Force $rawDir | Out-Null
Get-ChildItem $outDir -Filter "$Name-*.pcap" | Remove-Item

$jobs = foreach ($hub in $hubs) {
    $tag = $hub -replace '.*\\', ''
    $raw = Join-Path $rawDir "$tag.pcap"
    [pscustomobject]@{
        Raw  = $raw
        Out  = Join-Path $outDir "$Name-$tag.pcap"
        Proc = Start-Process $usbpcap -PassThru -WindowStyle Hidden -ArgumentList @(
            '-d', $hub, '-o', "`"$raw`"", '-A', '--inject-descriptors')
    }
}
Start-Sleep -Seconds $Seconds
$jobs.Proc | Stop-Process -Force
Start-Sleep -Milliseconds 500

# Keep only SteelSeries devices: smaller files, and no keyboard input on disk.
$tshark = "$env:ProgramFiles\Wireshark\tshark.exe"
foreach ($job in $jobs) {
    if (-not (Test-Path $job.Raw)) { continue }
    $addrs = & $tshark -r $job.Raw -Y 'usb.idVendor == 0x1038' -T fields -e usb.device_address 2>$null |
        Sort-Object -Unique
    if ($addrs) {
        $filter = ($addrs | ForEach-Object { "usb.device_address == $_" }) -join ' || '
        & $tshark -r $job.Raw -Y $filter -w $job.Out -F pcap 2>$null
    }
}
Remove-Item $rawDir -Recurse -Force
