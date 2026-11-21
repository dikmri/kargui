# 再変換を、Windows 標準の入力欄（lib/ime-ref.cs。この道具の中の別のスレッドで開く）で ime-check.ps1 の (6) と同じ操作で試し、kargui と比べる道具。
# 標準の入力欄で始まるなら kargui の答え方が足りない。標準の入力欄でも始まらないなら、IME の設定（変換キーの割り当て）の問題。
# 候補は確定せず Esc で取り消す（IME の学習で、利用者の候補の順番を変えないため）。
# 使い方: powershell -NoProfile -ExecutionPolicy Bypass -File tools/ime-reconvert-ref.ps1 [-Pos -1410,200]
#   作者が席を外している間に走らせるときは tools/away-run.ps1 から呼ぶ。人の入力があると止まる（終了コード 2）
param(
  [string]$Pos = '-1410,200',
  [string]$OutDir = "$PSScriptRoot\..\logs\ime-ref"
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$log = Join-Path $OutDir 'ref.log'
Remove-Item $log, (Join-Path $OutDir 'aborted.txt') -ErrorAction SilentlyContinue
. "$PSScriptRoot\lib\sendinput.ps1"

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Path "$PSScriptRoot\lib\ime-ref.cs" -ReferencedAssemblies System.Windows.Forms, System.Drawing
$xy = $Pos.Split(',')
$p = $null
if (-not [RefApp]::StartThread($log, [int]$xy[0], [int]$xy[1])) { throw '標準の入力欄の窓が開かない' }
$h = [RefApp]::Handle
Front
function LogCount([string]$pattern) { @(Select-String -Path $log -Pattern $pattern).Count }

Tap 0x16   # VK_IME_ON
Start-Sleep -Milliseconds 300
TypeRoma 'g'
Start-Sleep -Milliseconds 400
if ((LogCount 'event=ime.start') -eq 0) {
  # VK_IME_ON が効かない IME では、半角/全角キーで切り替える
  Tap 0x08; Tap 0x19; Start-Sleep -Milliseconds 300; TypeRoma 'g'; Start-Sleep -Milliseconds 400
}
if ((LogCount 'event=ime.start') -eq 0) { Abort 4 'IME がオンにならない' }
TypeRoma 'yuunyuuwokau'; Start-Sleep -Milliseconds 300
Tap 0x20; Start-Sleep -Milliseconds 500   # 変換
Tap 0x0D; Start-Sleep -Milliseconds 400   # 1 番目の候補で確定
Chord 0x10 0x24; Start-Sleep -Milliseconds 300   # Shift+Home で全部を選ぶ
$startsBefore = LogCount 'event=ime.start'
$linesBefore = (Get-Content $log -Encoding UTF8).Count
Tap 0x1C; Start-Sleep -Milliseconds 800   # 変換キー（VK_CONVERT）
Shot 'ref-reconvert'
$started = (LogCount 'event=ime.start') -gt $startsBefore
$afterConvert = @(Get-Content $log -Encoding UTF8 | Select-Object -Skip $linesBefore)
Tap 0x1B; Start-Sleep -Milliseconds 300   # Esc（再変換を取り消す）
Tap 0x1B; Start-Sleep -Milliseconds 300
Tap 0x1A   # VK_IME_OFF
[void][K]::PostMessageW($h, 0x0010, [IntPtr]0, [IntPtr]0)   # WM_CLOSE
Start-Sleep -Milliseconds 500
[void][K]::ForceForeground($script:prevForeground)

$out = @()
$out += "標準の入力欄（WinForms の TextBox）で、選んで変換キー: 再変換が始まった=$started"
$out += "  変換キーの後の記録:"
$afterConvert | Select-Object -First 12 | ForEach-Object { $out += "  $_" }
$out += "  閉じた時: " + (Get-Content $log -Encoding UTF8 | Where-Object { $_ -match 'event=close' } | Select-Object -Last 1)
$out | Set-Content -Path (Join-Path $OutDir 'summary.txt') -Encoding UTF8
$out
