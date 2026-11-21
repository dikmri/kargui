# 日本語入力の 5 項目（計画 2.3）を、実際の IME で確かめる道具。
# タスク帳を開いて前面に出し、キー入力をまねて IME で打ち、アプリの記録と画面の写真で確かめる。
#   (1) 変換を確定する Enter では追加されない（次の Enter で 1 回だけ追加される）
#   (2) 変換中は値が変わったと知らせない（絞り込みの欄で、変換中は絞り込みが走らない）
#   (3) 変換中の文字が入力欄の中に出る（記録の ime.preedit と写真）
#   (4) 候補の窓が文字の位置に出る（IME に答えた文字の位置と、新しく出た窓の位置を比べる）
#   (5) 変換の途中で焦点を移しても、変換中の文字が消えない（元の入力欄に確定される）
#   (6) 再変換: 確定した文字を選んで変換キーを押すと、変換し直せる（変換キーは道具が送るので、キーボードに無くてよい）
#   あわせて、変換中の文字の文節（区切りと変換中の文節）が IME から届いているかを数える
#   候補はいつも 1 番目で確定する（IME の学習で、利用者の候補の順番を変えないため）
# 注意: キー入力は前面の窓に届く。走らせている間（約 30 秒）はキーボードとマウスに触らない。
#   タスク帳が前面でなくなったら、その時点で止める（ほかの窓に文字を打たないため）。
# 使い方: powershell -NoProfile -ExecutionPolicy Bypass -File tools/ime-check.ps1 [-Pos -1410,200]
#   作者が席を外している間に走らせるときは tools/away-run.ps1 から呼ぶ。人の入力があると止まる（終了コード 2）
param(
  [string]$Pos = '-1410,200',
  [string]$Exe = "$PSScriptRoot\..\target\release\taskbook-kargui.exe",
  [string]$OutDir = "$PSScriptRoot\..\logs\ime-check"
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path $OutDir).Path
$log = Join-Path $OutDir 'taskbook.log'
Remove-Item $log -ErrorAction SilentlyContinue
Remove-Item (Join-Path $OutDir 'aborted.txt') -ErrorAction SilentlyContinue
. "$PSScriptRoot\lib\sendinput.ps1"

$p = Start-Process -FilePath $Exe -ArgumentList "--items 3 --pos $Pos --log `"$log`"" -PassThru
$deadline = (Get-Date).AddSeconds(10)
while (-not ((Test-Path $log) -and (Select-String -Path $log -Pattern 'event=ready' -Quiet))) {
  if ((Get-Date) -gt $deadline) { throw 'タスク帳の準備が終わらない' }
  Start-Sleep -Milliseconds 100
}
$h = $p.MainWindowHandle
for ($i = 0; $i -lt 20 -and $h -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 100; $p.Refresh(); $h = $p.MainWindowHandle }
Front

function LogCount([string]$pattern) { @(Select-String -Path $log -Pattern $pattern).Count }

# 新しいタスクの入力欄（窓の中の 400,68）を押して、IME をオンにする
Click 400 68
Tap 0x16   # VK_IME_ON
Start-Sleep -Milliseconds 300
TypeRoma 'g'
Start-Sleep -Milliseconds 400
if ((LogCount 'event=ime.start') -eq 0) {
  # VK_IME_ON が効かない IME では、半角/全角キーで切り替える
  Tap 0x08; Tap 0x19; Start-Sleep -Milliseconds 300; TypeRoma 'g'; Start-Sleep -Milliseconds 400
}
if ((LogCount 'event=ime.start') -eq 0) { Stop-Process -Id $p.Id; throw 'IME がオンにならない' }
TypeRoma 'yuunyuuwokau'
Start-Sleep -Milliseconds 400
Shot '1-composing'
$before = [K]::Visible()
Tap 0x20; Start-Sleep -Milliseconds 500   # 変換
Tap 0x20; Start-Sleep -Milliseconds 700   # 候補の窓
Shot '2-candidates'
$after = [K]::Visible()
$new = @($after | Where-Object { $before -notcontains $_ })
# 1 番目の候補に戻してから確定する（2 番目を確定すると、IME がそれを覚えて次から先に出す）
Tap 0x26; Start-Sleep -Milliseconds 300   # ↑
Tap 0x0D; Start-Sleep -Milliseconds 400   # 確定
$addsAfterCommit = LogCount 'event=add '
Tap 0x0D; Start-Sleep -Milliseconds 400   # 追加
$addsAfterSubmit = LogCount 'event=add '
# (6) 再変換: 「牛乳を買う」を確定し、Ctrl+A で全部を選んで変換キーを押す。
#   変換し直した 1 番目の候補を確定して追加し、選んでいた文字と置き換わったか（二重にならないか）を見る
TypeRoma 'gyuunyuuwokau'; Start-Sleep -Milliseconds 300
Tap 0x20; Start-Sleep -Milliseconds 500   # 変換
Tap 0x0D; Start-Sleep -Milliseconds 400   # 確定（まだ追加しない）
$text6 = if ((Get-Content $log -Encoding UTF8 | Where-Object { $_ -match 'event=ime.commit args="text=' } | Select-Object -Last 1) -match 'text=(.*?)"') { $Matches[1] } else { '' }
Chord 0x11 0x41   # Ctrl+A で全部を選ぶ
Start-Sleep -Milliseconds 300
$selected = LogCount 'key=A mods=Mods \{ shift: false, ctrl: true'
$startsBefore = LogCount 'event=ime.start'
$linesBefore = (Get-Content $log -Encoding UTF8).Count
Tap 0x1C; Start-Sleep -Milliseconds 800   # 変換キー（VK_CONVERT）
Shot '4-reconvert'
$reconvStarted = (LogCount 'event=ime.start') -gt $startsBefore
$afterConvert = @(Get-Content $log -Encoding UTF8 | Select-Object -Skip $linesBefore)
Tap 0x0D; Start-Sleep -Milliseconds 400   # 1 番目の候補で確定
Shot '5-after-reconvert'
Tap 0x0D; Start-Sleep -Milliseconds 400   # 追加
$added6 = if ((Get-Content $log -Encoding UTF8 | Select-Object -Skip $linesBefore | Where-Object { $_ -match 'event=add args="text=[^"]' } | Select-Object -First 1) -match 'text=(.*?)"') { $Matches[1] } else { '' }
# 欄を空にする（Ctrl+A と Delete）
Chord 0x11 0x41; Tap 0x2E
Start-Sleep -Milliseconds 300
# (5) 変換の途中で絞り込みの欄（130,112）を押す
TypeRoma 'shiryou'; Start-Sleep -Milliseconds 400
Click 130 112
Start-Sleep -Milliseconds 400
Shot '3-after-focus-move'
# (2) 絞り込みの欄で変換する。変換中は絞り込みが走らない
$filtersBefore = LogCount 'event=filter '
TypeRoma 'shiryou'; Start-Sleep -Milliseconds 400
$filtersComposing = LogCount 'event=filter '
Tap 0x20; Start-Sleep -Milliseconds 400; Tap 0x0D; Start-Sleep -Milliseconds 500
$filtersCommitted = LogCount 'event=filter '
Tap 0x1A   # VK_IME_OFF
[void][K]::PostMessageW($h, 0x0010, [IntPtr]0, [IntPtr]0)   # WM_CLOSE
[void]$p.WaitForExit(5000)
[void][K]::ForceForeground($script:prevForeground)

# 結果をまとめる
$lines = Get-Content $log -Encoding UTF8
$commit = @($lines | Where-Object { $_ -match 'event=ime.commit args="text=' } | Select-Object -First 1)
$add = @($lines | Where-Object { $_ -match 'event=add args="text=' } | Select-Object -First 1)
# 名前を $pos にしない（引数の $Pos と同じ変数になり、文字列 1 つにまとめられてしまう）
$charPos = @($lines | Where-Object { $_ -match 'imr=QUERYCHARPOSITION' -and $_ -match 'answered=true' })
$preedit = @($lines | Where-Object { $_ -match 'event=ime.preedit' })
$focusCommit = @($lines | Where-Object { $_ -match 'event=ime.commit_on_focus' })
$reconv = @($lines | Where-Object { $_ -match 'imr=RECONVERTSTRING' })
$confirm = @($lines | Where-Object { $_ -match 'imr=CONFIRMRECONVERTSTRING' })
$split = @($preedit | Where-Object { $_ -match 'clauses=\[0, \d+, ' })
$targeted = @($preedit | Where-Object { $_ -match 'target=Some' })
$out = @()
$out += "(1) 確定の Enter で追加されない: 確定の後の追加 $addsAfterCommit 回、次の Enter の後 $addsAfterSubmit 回 → " + $(if ($addsAfterCommit -eq 0 -and $addsAfterSubmit -eq 1) { '合格' } else { '不合格' })
$out += "    確定: $commit"
$out += "    追加: $add"
$out += "(2) 変換中は絞り込みが走らない: 変換前 $filtersBefore 回 → 変換中 $filtersComposing 回 → 確定後 $filtersCommitted 回 → " + $(if ($filtersComposing -eq $filtersBefore -and $filtersCommitted -gt $filtersComposing) { '合格' } else { '不合格' })
$out += "(3) 変換中の文字: ime.preedit $($preedit.Count) 件（写真 1-composing.png で下線を見る） → " + $(if ($preedit.Count -gt 0) { '記録は合格' } else { '不合格' })
$out += "(4) 文字の位置の答え $($charPos.Count) 件。最後: $($charPos | Select-Object -Last 1)"
$out += "    候補の窓で新しく出た窓: " + $(if ($new.Count) { $new -join ' / ' } else { 'なし' }) + "（写真 2-candidates.png）"
$out += "(5) 焦点を移した時の確定: " + $(if ($focusCommit.Count) { "$($focusCommit[0]) → 合格" } else { 'ime.commit_on_focus が無い → 不合格' })
# 選んだ文字と置き換わっていれば、追加した文字は確定した文字と同じくらいの長さ（二重なら倍近くになる）
$notDoubled = $added6.Length -gt 0 -and $added6.Length -le $text6.Length + 2
$out += "(6) 再変換: Ctrl+A で選んだ=$($selected -gt 0)、変換が始まった=$reconvStarted、選ぶ前「$text6」→ 追加「$added6」 → " + $(if ($selected -gt 0 -and $reconvStarted -and $notDoubled) { '合格' } else { '不合格' })
$out += "    変換キーの後の IME の問い合わせ（前後の文字・再変換・範囲の確定）:"
$afterConvert | Where-Object { $_ -match 'imr=(DOCUMENTFEED|RECONVERTSTRING|CONFIRMRECONVERTSTRING)' -or $_ -match 'event=ime.start' } | Select-Object -First 8 | ForEach-Object { $out += "    $_" }
$out += "    全体で RECONVERTSTRING $($reconv.Count) 件、CONFIRMRECONVERTSTRING $($confirm.Count) 件"
$out += "文節: 2 つ以上に分かれて届いた ime.preedit $($split.Count) 件、変換中の文節があった $($targeted.Count) 件（写真 2-candidates.png で太い下線を見る）"
$out | Set-Content -Path (Join-Path $OutDir 'summary.txt') -Encoding UTF8
$out
