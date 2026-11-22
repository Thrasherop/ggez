<#
.SYNOPSIS
  Print the configuration packets exchanged with a SteelSeries device in a capture.

.DESCRIPTION
  Finds SteelSeries devices (vendor 1038) in the capture by their descriptors,
  then prints every packet carrying data to or from them as:
    time  direction  endpoint  hex bytes
  Mouse-movement reports are dropped unless -All is given.

.EXAMPLE
  .\tools\decode.ps1 captures\remap-button6-to-f13.pcapng
#>
param(
    [Parameter(Mandatory)][string]$File,
    [switch]$All
)
$ErrorActionPreference = 'Stop'

$tshark = "$env:ProgramFiles\Wireshark\tshark.exe"
if (-not (Test-Path $tshark)) { throw "tshark not found at $tshark - install Wireshark" }

# Pass 1: which bus/device addresses belong to SteelSeries?
$devices = & $tshark -r $File -Y 'usb.idVendor == 0x1038' -T fields -e usb.bus_id -e usb.device_address -e usb.idProduct |
    Sort-Object -Unique
if (-not $devices) { throw "No SteelSeries device descriptors in $File (was the device plugged in during capture?)" }

$clauses = foreach ($d in $devices) {
    $bus, $addr, $productId = $d -split "`t"
    Write-Host "# SteelSeries device: bus $bus address $addr product $productId"
    "(usb.bus_id == $bus && usb.device_address == $addr)"
}
$filter = '(' + ($clauses -join ' || ') + ')'
# Interface 3 is the configuration interface; its traffic is HID SET_REPORT
# control transfers and interrupt reports. Movement reports are short and
# frequent, so by default keep only packets on the control endpoint or with
# vendor-sized payloads.
if (-not $All) { $filter += ' && (usb.data_len >= 2)' }

& $tshark -r $File -Y $filter -T fields `
    -e frame.time_relative -e usb.endpoint_address -e usb.transfer_type `
    -e usb.data_fragment -e usbhid.data -e usb.capdata |
    ForEach-Object {
        $t, $ep, $type, $ctrl, $hid, $cap = $_ -split "`t"
        $data = @($ctrl, $hid, $cap) | Where-Object { $_ } | Select-Object -First 1
        if (-not $data) { return }
        $epNum = [Convert]::ToInt32($ep, 16)
        $dir = if ($epNum -band 0x80) { 'IN ' } else { 'OUT' }
        '{0,10:F4}  {1}  ep{2:x2}  {3}' -f [double]$t, $dir, $epNum, $data
    }
