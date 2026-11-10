# 0단계 GUI 시험 도구: 시험 앱 창 찾기, 화면 캡처, 마우스·키보드 입력 (SendInput)
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class S0 {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint f, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(POINT p);
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, System.Text.StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
}
"@
[S0]::SetProcessDPIAware() | Out-Null

$script:Out = if ($env:TBS_S0_OUT) { $env:TBS_S0_OUT } else { Join-Path $PSScriptRoot "..\out" }

function Get-S0Window { if (-not $script:Hwnd) { $j = S0-Cmd "hwnd" | ConvertFrom-Json; $script:Hwnd = [IntPtr][long]($j.result.hwnd -replace "[^0-9]", "") }; $script:Hwnd }
function Get-S0Rect { $r = New-Object S0+RECT; [S0]::GetWindowRect((Get-S0Window), [ref]$r) | Out-Null; $r }

function Save-Shot([string]$name, $rect = $null) {
  if (-not $rect) { $rect = Get-S0Rect }
  $w = $rect.R - $rect.L; $h = $rect.B - $rect.T
  $bmp = New-Object System.Drawing.Bitmap $w, $h
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($rect.L, $rect.T, 0, 0, (New-Object System.Drawing.Size $w, $h))
  $p = Join-Path $script:Out "$name.png"; $bmp.Save($p); $g.Dispose(); $bmp.Dispose(); $p
}

function Move-Mouse([int]$x, [int]$y) { [S0]::SetCursorPos($x, $y) | Out-Null }
function Mouse-Down { [S0]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero) }
function Mouse-Up { [S0]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero) }
function Click([int]$x, [int]$y) { Move-Mouse $x $y; Start-Sleep -Milliseconds 80; Mouse-Down; Start-Sleep -Milliseconds 60; Mouse-Up }
function Drag([int]$x1, [int]$y1, [int]$x2, [int]$y2, [int]$steps = 20) {
  Move-Mouse $x1 $y1; Start-Sleep -Milliseconds 150; Mouse-Down; Start-Sleep -Milliseconds 150
  for ($i = 1; $i -le $steps; $i++) { Move-Mouse ([int]($x1 + ($x2 - $x1) * $i / $steps)) ([int]($y1 + ($y2 - $y1) * $i / $steps)); Start-Sleep -Milliseconds 20 }
  Start-Sleep -Milliseconds 150; Mouse-Up; Start-Sleep -Milliseconds 300
}

# 키 조합 보내기: Send-Combo @(0x11) 0x54  → Ctrl+T
function Send-Combo([byte[]]$mods, [byte]$key) {
  foreach ($m in $mods) { [S0]::keybd_event($m, 0, 0, [UIntPtr]::Zero) }
  [S0]::keybd_event($key, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 40
  [S0]::keybd_event($key, 0, 2, [UIntPtr]::Zero)
  [array]::Reverse($mods); foreach ($m in $mods) { [S0]::keybd_event($m, 0, 2, [UIntPtr]::Zero) }
  Start-Sleep -Milliseconds 200
}

function Class-At([int]$x, [int]$y) {
  $p = New-Object S0+POINT; $p.X = $x; $p.Y = $y
  $h = [S0]::WindowFromPoint($p); $sb = New-Object System.Text.StringBuilder 256
  [S0]::GetClassNameW($h, $sb, 256) | Out-Null; $sb.ToString()
}

# 앱의 시험 명령 통로: out/cmd.txt에 쓰고 out/cmd-done.txt를 기다린다
function S0-Cmd([string]$cmd) {
  $done = Join-Path $script:Out "cmd-done.txt"
  Remove-Item $done -ErrorAction SilentlyContinue
  [IO.File]::WriteAllText((Join-Path $script:Out "cmd.txt"), $cmd)
  for ($i = 0; $i -lt 150; $i++) { if (Test-Path $done) { Start-Sleep -Milliseconds 50; return Get-Content $done -Raw }; Start-Sleep -Milliseconds 100 }
  "timeout"
}
