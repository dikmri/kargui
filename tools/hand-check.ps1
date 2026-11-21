# 公開前の作者の手の確かめで、アプリを 1 つ開く道具。アプリの記録と、読み上げソフトに届く出来事（UI オートメーション）の記録を
# logs/hand-check/ に残す（ナレーターで何が読まれたかを、後から記録で確かめるため）。窓を閉じると記録も止まる。
# 使い方: powershell -NoProfile -ExecutionPolicy Bypass -File tools/hand-check.ps1 -App taskbook|settings|reader|table
param(
  [ValidateSet('taskbook', 'settings', 'reader', 'table')][string]$App = 'taskbook',
  [int]$Seconds = 1800
)
$root = Resolve-Path "$PSScriptRoot\.."
$exe = Join-Path $root "target\release\$App-kargui.exe"
$dir = Join-Path $root 'logs\hand-check'
New-Item -ItemType Directory -Force -Path $dir | Out-Null
$log = Join-Path $dir "$App.log"
$uia = Join-Path $dir "$App-uia.log"
$extra = if ($App -eq 'taskbook') { '--items 100 ' } else { '' }
$p = Start-Process -FilePath $exe -ArgumentList "$extra--log `"$log`"" -PassThru
Start-Sleep -Milliseconds 800
Start-Process -FilePath powershell -WindowStyle Hidden -ArgumentList "-NoProfile -ExecutionPolicy Bypass -File `"$PSScriptRoot\uia-events.ps1`" -ProcessId $($p.Id) -Out `"$uia`" -Seconds $Seconds"
"開きました: $App"
"  アプリの記録: $log"
"  読み上げの出来事の記録: $uia"
