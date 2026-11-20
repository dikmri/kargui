# 窓の中の部品の木を、Windows の UI オートメーション（スクリーンリーダーが読む仕組み）で書き出す。
# 部品ごとに 種類・名前・値・チェックの状態・焦点 を 1 行に書き、最後に数をまとめる。
# 使い方: powershell -NoProfile -ExecutionPolicy Bypass -File tools/uia-dump.ps1 -ProcessId 1234 -Out runtime/uia.txt
param(
  [Parameter(Mandatory = $true)][int]$ProcessId,
  [Parameter(Mandatory = $true)][string]$Out,
  [int]$MaxDepth = 12
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$p = Get-Process -Id $ProcessId
if ($p.MainWindowHandle -eq [IntPtr]::Zero) { throw '窓が見つからない' }
$root = [System.Windows.Automation.AutomationElement]::FromHandle($p.MainWindowHandle)
$walker = [System.Windows.Automation.TreeWalker]::ControlViewWalker
$script:lines = New-Object System.Collections.Generic.List[string]
$script:count = @{}
$script:unnamed = 0
$interactive = @('Button', 'CheckBox', 'Edit', 'ListItem', 'RadioButton', 'ComboBox', 'Hyperlink', 'MenuItem', 'TabItem')

function Dump($el, $depth) {
  $c = $el.Current
  $type = $c.ControlType.ProgrammaticName -replace '^ControlType\.', ''
  $line = ('  ' * $depth) + $type + ' "' + $c.Name + '"'
  try {
    $vp = $el.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    $line += ' value="' + $vp.Current.Value + '"'
  } catch {}
  try {
    $tp = $el.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    $line += ' toggle=' + $tp.Current.ToggleState
  } catch {}
  if ($c.HasKeyboardFocus) { $line += ' [焦点]' }
  if ($c.IsKeyboardFocusable) { $line += ' [Tab]' }
  $script:lines.Add($line)
  if (-not $script:count.ContainsKey($type)) { $script:count[$type] = 0 }
  $script:count[$type]++
  if ($interactive -contains $type -and [string]::IsNullOrWhiteSpace($c.Name)) { $script:unnamed++ }
  if ($depth -ge $MaxDepth) { return }
  $child = $walker.GetFirstChild($el)
  while ($null -ne $child) {
    Dump $child ($depth + 1)
    $child = $walker.GetNextSibling($child)
  }
}

$t = [System.Diagnostics.Stopwatch]::StartNew()
Dump $root 0
$t.Stop()
$summary = ($script:count.GetEnumerator() | Sort-Object Name | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join ' '
$script:lines.Add('')
$script:lines.Add("合計 $($script:lines.Count - 1) 個 / 名前の無い操作部品 $($script:unnamed) 個 / $([int]$t.ElapsedMilliseconds) ms")
$script:lines.Add($summary)
$dir = Split-Path -Parent $Out
if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
[System.IO.File]::WriteAllLines($Out, $script:lines, (New-Object System.Text.UTF8Encoding $false))
Write-Output "ok $($script:lines.Count) lines unnamed=$($script:unnamed)"
