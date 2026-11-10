<#
  spec 7장 메모리 측정: 한 브라우저에 속한 모든 프로세스의 메모리 합계를 적는다.

  쓰는 법 (PowerShell):
    .\tools\measure-memory.ps1                  # TB's Browser (tbs-browser.exe와 그 아래 WebView2 프로세스)
    .\tools\measure-memory.ps1 -Name msedge     # Edge (다른 Edge 창은 닫고 잰다)
    .\tools\measure-memory.ps1 -Name msedge -CommandLine "edge-measure"
                                                # 명령줄에 그 글자가 들어간 Edge 인스턴스만 (임시 프로필로 띄운 측정용 Edge)

  "개인 작업 집합"은 작업 관리자의 "메모리" 열과 같은 값이다.
#>
param([string]$Name = "tbs-browser", [string]$CommandLine = "")

$exe = "$Name.exe"
$all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CommandLine
$byParent = $all | Group-Object ParentProcessId -AsHashTable -AsString
# 맨 위 프로세스: 이름이 같고, 부모는 이름이 다른 것
$roots = $all | Where-Object { $_.Name -ieq $exe } | Where-Object { $p = $_.ParentProcessId; -not ($all | Where-Object { $_.ProcessId -eq $p -and $_.Name -ieq $exe }) }
if ($CommandLine) { $roots = $roots | Where-Object { $_.CommandLine -and $_.CommandLine.Contains($CommandLine) } }
if (-not $roots) { Write-Host "$exe 이(가) 실행 중이 아닙니다. 브라우저를 켠 뒤 다시 실행해 주세요."; exit 1 }

$ids = New-Object System.Collections.Generic.List[int]
$queue = New-Object System.Collections.Generic.Queue[int]
foreach ($r in $roots) { $queue.Enqueue([int]$r.ProcessId) }
while ($queue.Count) {
  $id = $queue.Dequeue()
  if ($ids.Contains($id)) { continue }
  $ids.Add($id)
  if ($byParent -and $byParent.ContainsKey("$id")) { foreach ($c in $byParent["$id"]) { $queue.Enqueue([int]$c.ProcessId) } }
}

$perf = @{}
Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | ForEach-Object { $perf[[int]$_.IDProcess] = [int64]$_.WorkingSetPrivate }

$rows = foreach ($id in $ids) {
  $p = Get-Process -Id $id -ErrorAction SilentlyContinue
  if (-not $p) { continue }
  [pscustomobject]@{
    PID = $id
    이름 = $p.ProcessName
    개인작업집합MB = [math]::Round(($perf[$id]) / 1MB, 1)
    커밋MB = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
  }
}
$rows | Sort-Object 개인작업집합MB -Descending | Format-Table -AutoSize
$ws = ($rows | Measure-Object 개인작업집합MB -Sum).Sum
$commit = ($rows | Measure-Object 커밋MB -Sum).Sum
Write-Host ("{0}  프로세스 {1}개  개인 작업 집합 합계 {2:N0} MB  커밋 합계 {3:N0} MB  ({4})" -f $exe, $rows.Count, $ws, $commit, (Get-Date -Format "yyyy-MM-dd HH:mm"))
