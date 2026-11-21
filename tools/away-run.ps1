# 作者が席を外している間だけ、窓を前面に出してキー入力をまねる確かめの道具を走らせる。
# - 人がキーとマウスに触れていない時間が IdleSeconds を超え、画面がロックされておらず、
#   全画面のゲームや発表のモードが前面にない時に、Scripts を順に走らせる。
#   人の入力は低いレベルのフックで見る（まねて送った入力と、1 秒で 8 px に満たないマウスの揺れは数えない。
#   この PC はマウスが止まっていても揺れの入力が出続けるので、Windows の「最後の入力の時刻」は使えない）
# - 走っている途中で人の入力があると、その道具は止まる（終了コード 2）。また席を外すのを待って、やり直す
# - 状態の変わり目を logs/away-run/status.log に書く
# 使い方（後ろで走らせる）: powershell -NoProfile -ExecutionPolicy Bypass -File tools/away-run.ps1 [-IdleSeconds 600] [-MaxHours 12]
param(
  [string[]]$Scripts = @("$PSScriptRoot\ime-check.ps1", "$PSScriptRoot\ime-reconvert-ref.ps1"),
  [int]$IdleSeconds = 600,
  [double]$MaxHours = 12,
  [int]$Retries = 5,
  [string]$Status = "$PSScriptRoot\..\logs\away-run\status.log"
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path (Split-Path $Status) | Out-Null
. "$PSScriptRoot\lib\sendinput.ps1"
function Say([string]$s) {
  $line = "$(Get-Date -Format o) $s"
  Add-Content -Path $Status -Value $line -Encoding UTF8
  $line
}
$needMs = $IdleSeconds * 1000
$deadline = (Get-Date).AddHours($MaxHours)
Say "start scripts=$($Scripts.Count) need_idle_ms=$needMs foreground_lock_ms=$([K]::ForegroundLockMs())"
$i = 0
$tries = 0
$state = ''
while ($i -lt $Scripts.Count) {
  if ((Get-Date) -gt $deadline) { Say 'timeout'; exit 1 }
  # 通知の状態: 3 = Direct3D の全画面（ゲーム）、4 = 発表のモード。この 2 つの時は席にいるとみなす。
  # 2（全画面の窓）は使わない。この PC では画面いっぱいに広げた Claude の窓でも 2 になる
  $ns = [K]::NotificationState()
  $now = if (-not [K]::Unlocked()) { 'locked' } elseif (@(3, 4) -contains $ns) { "fullscreen($ns)" } elseif ([Presence]::IdleMs() -lt $needMs) { 'present' } else { 'away' }
  if ($now -ne $state) { Say "state=$now"; $state = $now }
  if ($now -ne 'away') { Start-Sleep -Seconds 15; continue }
  $s = $Scripts[$i]
  Say "run $(Split-Path $s -Leaf) idle_ms=$([Presence]::IdleMs()) jitter=$([Presence]::Jitter)"
  & powershell -NoProfile -ExecutionPolicy Bypass -File $s | Out-Null
  $code = $LASTEXITCODE
  Say "exit $(Split-Path $s -Leaf) code=$code"
  $state = ''
  if (($code -eq 2 -or $code -eq 3) -and $tries -lt $Retries) { $tries++; continue }   # 人が戻った・前面に出せなかった → また待つ
  $i++
  $tries = 0
}
Say 'done'
