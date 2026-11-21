# 前面の窓へキー入力をまねて送る道具の共通の部品（ime-check.ps1・ime-reconvert-ref.ps1・away-run.ps1 が読み込む）。
# 読み込む側は $h（送り先の窓）・$p（送り先のプロセス）・$OutDir を決めてから Guard などを使う。
# 人がキーやマウスに触ったら、その時点で止める（終了コード 2）。送り先が前面でなくなったら止める（終了コード 3）。
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices; using System.Text;
public static class K {
  [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public KEYBDINPUT ki; public ulong pad; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
  [DllImport("user32.dll")] public static extern uint SendInput(uint n, INPUT[] inputs, int size);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr pid);
  [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a, uint b, bool attach);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] static extern bool BringWindowToTop(IntPtr h);
  [DllImport("shell32.dll")] static extern int SHQueryUserNotificationState(out int state);
  [DllImport("user32.dll")] static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
  [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr h);
  [DllImport("user32.dll")] static extern bool SystemParametersInfoW(uint action, uint p1, out uint value, uint winini);
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr l);
  // 矢印・Home・End・PgUp・PgDn・Insert・Delete は拡張キーの印を付けて送る。
  // 印が無いとテンキーのキーとして扱われ、NumLock が入っていると Shift を押していても離れた状態で届く
  public static void Key(ushort vk, bool up) {
    var i = new INPUT[1];
    bool ext = (vk >= 0x21 && vk <= 0x28) || vk == 0x2D || vk == 0x2E;
    i[0].type = 1; i[0].ki.wVk = vk; i[0].ki.dwFlags = (up ? 2u : 0u) | (ext ? 1u : 0u);
    SendInput(1, i, Marshal.SizeOf(typeof(INPUT)));
  }
  // 窓を前面に出す。この PC は前面の切り替えの待ち時間が最大（ほかのアプリは前面を取れない）なので、
  // 取れなければ今の前面の窓の入力に一時的につないでから出す
  public static bool ForceForeground(IntPtr h) {
    if (SetForegroundWindow(h) && GetForegroundWindow() == h) return true;
    var fg = GetForegroundWindow();
    uint a = GetCurrentThreadId(), b = fg == IntPtr.Zero ? 0 : GetWindowThreadProcessId(fg, IntPtr.Zero);
    bool att = b != 0 && b != a && AttachThreadInput(a, b, true);
    BringWindowToTop(h);
    SetForegroundWindow(h);
    if (att) AttachThreadInput(a, b, false);
    return GetForegroundWindow() == h;
  }
  // 全画面のアプリ（動画・ゲーム・発表）が前面にあるか。あれば席にいるとみなす
  public static int NotificationState() { int v; return SHQueryUserNotificationState(out v) == 0 ? v : 0; }
  // 画面がロックされていない（入力を受ける画面が開ける）
  public static bool Unlocked() { var d = OpenInputDesktop(0, false, 0x0001); if (d == IntPtr.Zero) return false; CloseDesktop(d); return true; }
  // 入力が無い時間がこれを超えると、どのプロセスでも窓を前面に出せる（SPI_GETFOREGROUNDLOCKTIMEOUT。記録に書くだけ）
  public static uint ForegroundLockMs() { uint v; SystemParametersInfoW(0x2000, 0, out v, 0); return v; }
  public static List<string> Visible() {
    var list = new List<string>();
    EnumWindows((h, l) => {
      if (IsWindowVisible(h)) {
        RECT r; GetWindowRect(h, out r);
        var sb = new StringBuilder(256); GetClassNameW(h, sb, 256);
        list.Add(h.ToInt64() + "|" + sb + "|" + r.L + "," + r.T + "," + r.R + "," + r.B);
      }
      return true;
    }, IntPtr.Zero);
    return list;
  }
}
// 人の入力を見張る。キーとマウスの入力を低いレベルのフックで受け、まねて送った入力（印付き）と、
// 1 秒で 8 px に満たないマウスの揺れ（動いていない機器が出し続ける入力）は数えない。
// キーの中身は記録しない（時刻と数だけ）
public static class Presence {
  delegate IntPtr HookProc(int code, IntPtr w, IntPtr l);
  [StructLayout(LayoutKind.Sequential)] struct POINT { public int X, Y; }
  [StructLayout(LayoutKind.Sequential)] struct MSLL { public POINT pt; public uint data; public uint flags; public uint time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct KBLL { public uint vk; public uint scan; public uint flags; public uint time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] struct MSG { public IntPtr h; public uint m; public IntPtr w; public IntPtr l; public uint t; public POINT p; }
  [DllImport("user32.dll")] static extern IntPtr SetWindowsHookExW(int id, HookProc f, IntPtr mod, uint thread);
  [DllImport("user32.dll")] static extern IntPtr CallNextHookEx(IntPtr h, int code, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] static extern int GetMessageW(out MSG m, IntPtr h, uint a, uint b);
  [DllImport("kernel32.dll")] static extern IntPtr GetModuleHandleW(IntPtr name);
  static HookProc kb, ms;
  static POINT anchor; static uint anchorAt;
  public static uint LastReal = (uint)Environment.TickCount;
  public static int RealKeys, InjectedKeys, RealMouse, InjectedMouse, Jitter;
  // 人のマウスの入力をメッセージの種類ごとに数える（0x200 からの差。0 = 動き、1〜6 = ボタン、0x0a = ホイール）
  public static int[] ByMsg = new int[0x10];
  public static void Start() {
    var t = new System.Threading.Thread(() => {
      kb = (code, w, l) => {
        if (code >= 0) {
          var k = (KBLL)Marshal.PtrToStructure(l, typeof(KBLL));
          if ((k.flags & 0x10) != 0) InjectedKeys++; else { RealKeys++; LastReal = (uint)Environment.TickCount; }
        }
        return CallNextHookEx(IntPtr.Zero, code, w, l);
      };
      ms = (code, w, l) => {
        if (code >= 0) {
          var m = (MSLL)Marshal.PtrToStructure(l, typeof(MSLL));
          uint now = (uint)Environment.TickCount;
          if ((m.flags & 0x01) == 0 && (int)w >= 0x200 && (int)w < 0x210) ByMsg[(int)w - 0x200]++;
          if ((m.flags & 0x01) != 0) InjectedMouse++;
          else if ((int)w != 0x0200) { RealMouse++; LastReal = now; }   // ボタンとホイールは必ず人
          else {
            if (unchecked(now - anchorAt) > 1000) { anchor = m.pt; anchorAt = now; }
            if (Math.Abs(m.pt.X - anchor.X) + Math.Abs(m.pt.Y - anchor.Y) > 8) { RealMouse++; LastReal = now; anchor = m.pt; anchorAt = now; }
            else Jitter++;
          }
        }
        return CallNextHookEx(IntPtr.Zero, code, w, l);
      };
      SetWindowsHookExW(13, kb, GetModuleHandleW(IntPtr.Zero), 0);   // WH_KEYBOARD_LL
      SetWindowsHookExW(14, ms, GetModuleHandleW(IntPtr.Zero), 0);   // WH_MOUSE_LL
      MSG msg;
      while (GetMessageW(out msg, IntPtr.Zero, 0, 0) > 0) { }
    });
    t.IsBackground = true;
    t.Start();
  }
  // 最後の人の入力から何ミリ秒たったか
  public static uint IdleMs() { return unchecked((uint)Environment.TickCount - LastReal); }
}
"@
[void][K]::SetProcessDpiAwarenessContext([IntPtr](-4))
[Presence]::Start()
$script:prevForeground = [K]::GetForegroundWindow()

# 止める。送り先を閉じ、理由を aborted.txt に書いて、終了コードで知らせる
function Abort([int]$code, [string]$why) {
  if ($p) { Stop-Process -Id $p.Id -ErrorAction SilentlyContinue }
  if ($OutDir) { "$(Get-Date -Format o) $why" | Set-Content -Path (Join-Path $OutDir 'aborted.txt') -Encoding UTF8 }
  [void][K]::ForceForeground($script:prevForeground)
  Write-Output $why
  exit $code
}
function Guard {
  if ([Presence]::RealKeys + [Presence]::RealMouse -gt 0) { Abort 2 '人の入力があったので止めた（席に戻ったとみなす）' }
  if ([K]::GetForegroundWindow() -ne $h) { Abort 3 '送り先が前面ではないので止めた（ほかの窓に文字を打たないため）' }
}
function Tap([int]$vk) { Guard; [K]::Key([uint16]$vk, $false); Start-Sleep -Milliseconds 25; [K]::Key([uint16]$vk, $true); Start-Sleep -Milliseconds 45 }
# 修飾キー（Shift 0x10・Ctrl 0x11）を押したまま vk を押す
function Chord([int]$mod, [int]$vk) { Guard; [K]::Key([uint16]$mod, $false); Tap $vk; [K]::Key([uint16]$mod, $true); Start-Sleep -Milliseconds 45 }
function TypeRoma([string]$s) { foreach ($c in $s.ToUpper().ToCharArray()) { Tap ([int]$c) } }
function MakeLParam([int]$x, [int]$y) { [IntPtr](([int64]($y -band 0xffff) -shl 16) -bor ($x -band 0xffff)) }
function Click([int]$x, [int]$y) { [void][K]::PostMessageW($h, 0x201, [IntPtr]1, (MakeLParam $x $y)); Start-Sleep -Milliseconds 60; [void][K]::PostMessageW($h, 0x202, [IntPtr]0, (MakeLParam $x $y)); Start-Sleep -Milliseconds 300 }
function Shot([string]$name) {
  $r = New-Object K+RECT; [void][K]::GetWindowRect($h, [ref]$r)
  $w = $r.R - $r.L; $hh = $r.B - $r.T + 260
  $b = New-Object System.Drawing.Bitmap $w, $hh
  $g = [System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen($r.L, $r.T, 0, 0, (New-Object System.Drawing.Size $w, $hh)); $g.Dispose()
  $b.Save((Join-Path $OutDir "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
}
# 送り先を前面に出す。出せなければ止める
function Front {
  [void][K]::ForceForeground($h)
  Start-Sleep -Milliseconds 300
  if ([K]::GetForegroundWindow() -ne $h) { Abort 3 '送り先を前面に出せなかった（人が操作中か、前面の切り替えが止められている）' }
}
