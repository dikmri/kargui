# 比べるための Windows 標準の入力欄を、手で開く（中身は lib/ime-ref.cs）。
# IME とのやり取りと文字の変化を記録に書く。道具からは ime-reconvert-ref.ps1 が別のスレッドで開く。
# 使い方: powershell -NoProfile -ExecutionPolicy Bypass -File tools/ime-ref-app.ps1 -Log <記録> [-Pos -1410,200]
param(
  [Parameter(Mandatory)][string]$Log,
  [string]$Pos = '-1410,200'
)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -Path "$PSScriptRoot\lib\ime-ref.cs" -ReferencedAssemblies System.Windows.Forms, System.Drawing
$xy = $Pos.Split(',')
[Windows.Forms.Application]::EnableVisualStyles()
[Windows.Forms.Application]::Run([RefApp]::Make($Log, [int]$xy[0], [int]$xy[1]))
