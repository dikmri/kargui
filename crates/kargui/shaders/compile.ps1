# ui.hlsl を組み立てて ui_vs.cso と ui_ps.cso を作る。できた .cso はリポジトリに入れる。
# 使い方: powershell -ExecutionPolicy Bypass -File spike/shaders/compile.ps1
$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$fxc = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\fxc.exe' | Sort-Object FullName | Select-Object -Last 1
if (-not $fxc) { throw 'fxc.exe が見つかりません（Windows SDK が要ります）' }
& $fxc.FullName /nologo /O3 /T vs_4_0 /E vs_main /Fo "$here\ui_vs.cso" "$here\ui.hlsl"
if ($LASTEXITCODE -ne 0) { throw "vs の組み立てに失敗: $LASTEXITCODE" }
& $fxc.FullName /nologo /O3 /T ps_4_0 /E ps_main /Fo "$here\ui_ps.cso" "$here\ui.hlsl"
if ($LASTEXITCODE -ne 0) { throw "ps の組み立てに失敗: $LASTEXITCODE" }
Write-Output "ok: $($fxc.FullName)"
