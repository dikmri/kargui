# アプリを前面に出さずにサブモニターで開き、表示できた（記録の ready）ら窓を撮って閉じる道具。
# 作者が席にいる時に使ってよい（前面を取らない）。記録は -Log、写真は -Out。
# 使い方: powershell -NoProfile -ExecutionPolicy Bypass -File tools/app-shot.ps1 -Exe <exe> -Out <png> [-Log <log>] [-Args "..."] [-WaitMs 600]
param(
  [Parameter(Mandatory)][string]$Exe,
  [Parameter(Mandatory)][string]$Out,
  [string]$Log = "$PSScriptRoot\..\logs\app-shot.log",
  [string]$Args = '',
  [string]$Pos = '-1410,200',
  [int]$WaitMs = 600
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class AS {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
}
"@
[void][AS]::SetProcessDpiAwarenessContext([IntPtr](-4))
Remove-Item $Log -ErrorAction SilentlyContinue
$fg = [AS]::GetForegroundWindow()
$p = Start-Process -FilePath $Exe -ArgumentList "--pos $Pos --no-focus --log `"$Log`" $Args" -PassThru
$deadline = (Get-Date).AddSeconds(15)
while (-not ((Test-Path $Log) -and (Select-String -Path $Log -Pattern 'event=ready' -Quiet))) {
  if ((Get-Date) -gt $deadline) { Stop-Process -Id $p.Id -ErrorAction SilentlyContinue; throw '表示が終わらない' }
  Start-Sleep -Milliseconds 100
}
Start-Sleep -Milliseconds $WaitMs
$p.Refresh()
$h = $p.MainWindowHandle
$r = New-Object AS+RECT; [void][AS]::DwmGetWindowAttribute($h, 9, [ref]$r, 16)
$w = $r.R - $r.L; $hh = $r.B - $r.T
$b = New-Object System.Drawing.Bitmap $w, $hh
$g = [System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object System.Drawing.Size $w, $hh)); $g.Dispose()
$b.Save($Out, [System.Drawing.Imaging.ImageFormat]::Png)
$mem = [math]::Round($p.PrivateMemorySize64 / 1MB, 1)
[void][AS]::PostMessageW($h, 0x10, [IntPtr]0, [IntPtr]0)
[void]$p.WaitForExit(5000)
$ready = (Select-String -Path $Log -Pattern 'event=ready' | Select-Object -First 1).Line
"shot=$Out size=${w}x$hh private_mb=$mem foreground_kept=$([AS]::GetForegroundWindow() -eq $fg)"
"ready: $ready"
