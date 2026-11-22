<#
.SYNOPSIS
  Runs one administrator-only research action without a Windows prompt.

.DESCRIPTION
  USB capture and stopping SteelSeries GG both need administrator rights.
  Registering this script as a scheduled task that runs with highest
  privileges lets an unattended session do those two things. It performs
  exactly one action, described by captures\elevated-request.json:

    { "action": "capture", "name": "some-name", "seconds": 60 }
    { "action": "stop-gg" }
    { "action": "start-gg" }
    { "action": "remove-task" }   (deletes the scheduled task itself)

  and writes what happened to captures\elevated-result.txt.

  SECURITY: while the task exists, anything running as you can edit this
  script and have it run as administrator. Register it only for a research
  session and remove it afterwards.

  Register (from an administrator terminal):
    schtasks /create /tn ggez-elevated /rl highest /sc once /st 00:00 /f /tr "powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File E:\claude\other_software\ggez\tools\elevated.ps1"
  Remove:
    schtasks /delete /tn ggez-elevated /f
#>
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dir = Join-Path $root 'captures'
New-Item -ItemType Directory -Force $dir | Out-Null
$result = Join-Path $dir 'elevated-result.txt'

try {
    $request = Get-Content (Join-Path $dir 'elevated-request.json') -Raw | ConvertFrom-Json
    switch ($request.action) {
        'capture' {
            $name = [string]$request.name
            $seconds = [int]$request.seconds
            if ($name -notmatch '^[A-Za-z0-9_-]{1,60}$') { throw "bad capture name '$name'" }
            if ($seconds -lt 1 -or $seconds -gt 900) { throw "seconds must be 1-900" }
            if ($request.restartGG) {
                # Record what GG sends a mouse when it starts: stop it now and
                # start it again a few seconds into the capture.
                Get-Process | Where-Object { $_.Name -like 'SteelSeries*' } | Stop-Process -Force
                Start-Sleep -Seconds 2
                Get-Process | Where-Object { $_.Name -like 'SteelSeries*' } | Stop-Process -Force
                Start-Process powershell -WindowStyle Hidden -ArgumentList @(
                    '-NoProfile', '-Command',
                    "Start-Sleep 8; Start-Process '$env:ProgramFiles\SteelSeries\GG\SteelSeriesGG.exe' -ArgumentList '-dataPath=\`"C:\ProgramData\SteelSeries\GG\`"','-dbEnv=production'")
            }
            & (Join-Path $PSScriptRoot 'capture.ps1') -Name $name -Seconds $seconds
            "ok capture $name" | Set-Content $result
        }
        'stop-gg' {
            Get-Process | Where-Object { $_.Name -like 'SteelSeries*' } | Stop-Process -Force
            Start-Sleep -Seconds 2
            # The launcher restarts its children; sweep once more.
            Get-Process | Where-Object { $_.Name -like 'SteelSeries*' } | Stop-Process -Force
            "ok stop-gg" | Set-Content $result
        }
        'start-gg' {
            # The same arguments GG's own Start menu shortcut uses; without
            # them it exits at once.
            Start-Process "$env:ProgramFiles\SteelSeries\GG\SteelSeriesGG.exe" -ArgumentList @(
                '-dataPath="C:\ProgramData\SteelSeries\GG"', '-dbEnv=production')
            "ok start-gg" | Set-Content $result
        }
        'remove-task' {
            schtasks /delete /tn ggez-elevated /f | Out-Null
            "ok remove-task" | Set-Content $result
        }
        default { throw "unknown action '$($request.action)'" }
    }
} catch {
    "error $($_.Exception.Message)" | Set-Content $result
}
