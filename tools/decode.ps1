<#
.SYNOPSIS
  Print the packets exchanged with SteelSeries devices in a capture.

.DESCRIPTION
  Takes the -Name given to capture.ps1 (or explicit .pcap paths), finds
  SteelSeries devices (vendor 1038) by their descriptors, and prints every
  data-carrying packet to or from them as:
    time  direction  endpoint  hex bytes
  Mouse movement and other ordinary input reports are dropped unless -All
  is given; by default only reports of 32 bytes or more are shown, which is
  the size of configuration traffic.

.EXAMPLE
  .\tools\decode.ps1 remap-button6-to-f13
#>
param(
    [Parameter(Mandatory)][string]$Name,
    [switch]$All
)
$ErrorActionPreference = 'Stop'

$tshark = "$env:ProgramFiles\Wireshark\tshark.exe"
if (-not (Test-Path $tshark)) { throw "tshark not found at $tshark - install Wireshark" }

$files = if (Test-Path $Name) { Get-Item $Name } else {
    Get-ChildItem (Join-Path (Split-Path $PSScriptRoot -Parent) 'captures') -Filter "$Name-*.pcap"
}
if (-not $files) { throw "No capture files found for '$Name'" }

foreach ($file in $files) {
    # Pass 1: which device addresses belong to SteelSeries?
    $devices = & $tshark -r $file.FullName -Y 'usb.idVendor == 0x1038' -T fields -e usb.device_address -e usb.idProduct 2>$null |
        Sort-Object -Unique
    if (-not $devices) { continue }

    $clauses = foreach ($d in $devices) {
        $addr, $productId = $d -split "`t"
        "# $($file.Name): SteelSeries product $productId at address $addr"
        "usb.device_address == $addr"
    }
    $clauses | Where-Object { $_ -like '#*' } | Write-Output
    $filter = '(' + (($clauses | Where-Object { $_ -notlike '#*' }) -join ' || ') + ')'
    if (-not $All) { $filter += ' && usb.data_len >= 32' }

    & $tshark -r $file.FullName -Y $filter -T fields `
        -e frame.time_relative -e usb.endpoint_address -e usb.data_fragment -e usbhid.data -e usb.capdata 2>$null |
        ForEach-Object {
            $t, $ep, $ctrl, $hid, $cap = $_ -split "`t"
            $data = @($ctrl, $hid, $cap) | Where-Object { $_ } | Select-Object -First 1
            if (-not $data) { return }
            $epNum = [Convert]::ToInt32($ep, 16)
            $dir = if ($epNum -band 0x80) { 'IN ' } else { 'OUT' }
            # Trailing zero padding carries no information.
            $data = $data -replace '(00)+$', ''
            '{0,10:F4}  {1}  ep{2:x2}  {3}' -f [double]$t, $dir, $epNum, $data
        }
}
