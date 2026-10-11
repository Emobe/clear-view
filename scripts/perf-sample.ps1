<#
.SYNOPSIS
  Samples CPU and GPU use of a running clear-view once a second (roadmap 5.5a).

.DESCRIPTION
  Start clear-view first, put it in the state to measure (magnifier off, fullscreen or docked,
  zoom, resolution), then run this. It does not start or change the app.

  CPU is the process's TotalProcessorTime over the wall clock, as a percentage of ONE core, so
  it can pass 100 (the same unit as the app's own `[perf]` line).

  GPU comes from the Windows "GPU Engine" performance counters (the ones Task Manager shows),
  summed over this process's engines per type, as a percentage of each engine: Gpu3D, GpuCopy
  and GpuOther (everything else, for example video). Our process creates two D3D devices (the
  capture copy and wgpu), and both count here. The desktop compositor's own GPU use is not part
  of it.

  Output is one row per second, then mean, 95th percentile and maximum per column. The first
  second is dropped: the counters have no previous sample to compare with.

.PARAMETER Seconds
  How long to sample. Default 15.

.PARAMETER ProcessName
  The process to sample, without .exe. Default clear-view.

.PARAMETER Move
  Moves the mouse pointer in a circle (radius 300 px, a couple of turns a second) for the whole
  run, so "mouse moving" is the same each time. It takes the mouse from you while it
  runs. Leave it off for "mouse still" and keep your hands off the mouse.

.PARAMETER Label
  Free text copied into every row, for example "docked-top-25 moving 1440p".

.PARAMETER Csv
  Also write the rows to this CSV file.

.EXAMPLE
  .\scripts\perf-sample.ps1 -Seconds 20 -Label "fullscreen still 1440p"
  .\scripts\perf-sample.ps1 -Seconds 20 -Move -Label "fullscreen moving 1440p" -Csv fs-moving.csv
#>
param(
    [int]$Seconds = 15,
    [string]$ProcessName = 'clear-view',
    [switch]$Move,
    [string]$Label = '',
    [string]$Csv = ''
)

$ErrorActionPreference = 'Stop'

if ($Seconds -lt 3) { throw 'Use at least 3 seconds: the first sample is dropped.' }

$proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $proc) { throw "No process named '$ProcessName' is running. Start clear-view first." }
$procId = $proc.Id

# The GPU counter set is missing on old Windows and with some drivers; say so instead of
# sampling zeros. Counter names are localised, so this assumes an English Windows.
$set = Get-Counter -ListSet 'GPU Engine' -ErrorAction SilentlyContinue
if (-not $set) { throw "The 'GPU Engine' performance counter set is not available on this machine." }

$mover = $null
if ($Move) {
    $mover = Start-Job -ArgumentList ($Seconds + 2) -ScriptBlock {
        param($duration)
        Add-Type -Namespace Win32 -Name Cursor -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
[DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
public struct POINT { public int X; public int Y; }
'@
        $p = New-Object Win32.Cursor+POINT
        [void][Win32.Cursor]::GetCursorPos([ref]$p)
        $cx = $p.X; $cy = $p.Y; $r = 300.0; $angle = 0.0
        $clock = [Diagnostics.Stopwatch]::StartNew()
        while ($clock.Elapsed.TotalSeconds -lt $duration) {
            $angle += 0.15   # a couple of turns a second at the 10 ms step below
            [void][Win32.Cursor]::SetCursorPos([int]($cx + $r * [math]::Cos($angle)), [int]($cy + $r * [math]::Sin($angle)))
            Start-Sleep -Milliseconds 10
        }
        [void][Win32.Cursor]::SetCursorPos($cx, $cy)
    }
}

$rows = New-Object System.Collections.Generic.List[object]
$clock = [Diagnostics.Stopwatch]::StartNew()
$lastCpu = $proc.TotalProcessorTime.TotalSeconds
$lastAt = 0.0
$n = 0

try {
    Get-Counter -Counter '\GPU Engine(*)\Utilization Percentage' -SampleInterval 1 -MaxSamples $Seconds |
        ForEach-Object {
            $now = $clock.Elapsed.TotalSeconds
            $proc.Refresh()
            if ($proc.HasExited) { throw "$ProcessName exited during the run." }
            $cpuNow = $proc.TotalProcessorTime.TotalSeconds
            $cpu = ($cpuNow - $lastCpu) / [math]::Max($now - $lastAt, 0.001) * 100.0
            $lastCpu = $cpuNow; $lastAt = $now

            $g3d = 0.0; $gcopy = 0.0; $gother = 0.0
            foreach ($s in $_.CounterSamples) {
                if ($s.InstanceName -notlike "pid_${procId}_*") { continue }
                if ($s.InstanceName -match 'engtype_(.+)$') { $type = $Matches[1] } else { $type = '' }
                switch -Regex ($type) {
                    '^3D$'   { $g3d += $s.CookedValue }
                    '^Copy$' { $gcopy += $s.CookedValue }
                    default  { $gother += $s.CookedValue }
                }
            }

            $n++
            if ($n -eq 1) { return }   # no previous sample to compare with
            $rows.Add([pscustomobject][ordered]@{
                Label   = $Label
                Second  = $n - 1
                CpuPct  = [math]::Round($cpu, 1)
                Gpu3D   = [math]::Round($g3d, 1)
                GpuCopy = [math]::Round($gcopy, 1)
                GpuOther = [math]::Round($gother, 1)
            })
        }
}
finally {
    if ($mover) { Stop-Job $mover -ErrorAction SilentlyContinue; Remove-Job $mover -Force -ErrorAction SilentlyContinue }
}

if ($rows.Count -eq 0) { throw 'No samples were taken.' }

$rows | Format-Table -AutoSize

function Summarise($name) {
    $v = @($rows | ForEach-Object { [double]$_.$name } | Sort-Object)
    $p95 = $v[[math]::Min($v.Count - 1, [int][math]::Ceiling(0.95 * $v.Count) - 1)]
    [pscustomobject][ordered]@{
        Column = $name
        Mean   = [math]::Round(($v | Measure-Object -Average).Average, 1)
        P95    = $p95
        Max    = $v[-1]
    }
}

Write-Host ("Summary for '{0}' (pid {1}, {2} samples; CPU is % of one core):" -f $Label, $procId, $rows.Count)
'CpuPct', 'Gpu3D', 'GpuCopy', 'GpuOther' | ForEach-Object { Summarise $_ } | Format-Table -AutoSize

if ($Csv) {
    $rows | Export-Csv -Path $Csv -NoTypeInformation
    Write-Host "Rows written to $Csv"
}
