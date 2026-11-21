// 比べるための Windows 標準の入力欄（WinForms の TextBox = Win32 の EDIT）を 1 つだけ置いた窓。
// IME とのやり取り（WM_IME_STARTCOMPOSITION・WM_IME_REQUEST など）と、文字の変化を記録に書く。
// ime-ref-app.ps1（手で開く）と ime-reconvert-ref.ps1（道具が別のスレッドで開いて操作する）が使う。
using System;
using System.Drawing;
using System.IO;
using System.Threading;
using System.Windows.Forms;

public class LogBox : TextBox {
  public string LogPath;
  static readonly string[] Imr = { "", "COMPOSITIONWINDOW", "CANDIDATEWINDOW", "COMPOSITIONFONT", "RECONVERTSTRING", "CONFIRMRECONVERTSTRING", "QUERYCHARPOSITION", "DOCUMENTFEED" };
  public void W(string s) { File.AppendAllText(LogPath, DateTime.UtcNow.ToString("o") + " " + s + Environment.NewLine); }
  protected override void WndProc(ref Message m) {
    string name = null;
    long w = m.WParam.ToInt64();
    switch (m.Msg) {
      case 0x010D: name = "event=ime.start"; break;
      case 0x010E: name = "event=ime.end"; break;
      case 0x010F: name = "event=ime.composition lp=0x" + m.LParam.ToInt64().ToString("x"); break;
      case 0x0288: name = "event=ime.request imr=" + (w >= 0 && w < Imr.Length ? Imr[w] : w.ToString()) + " buffer=" + (m.LParam != IntPtr.Zero); break;
      case 0x0100: name = "event=key vk=0x" + w.ToString("x"); break;
    }
    base.WndProc(ref m);
    if (name != null) W(name + (m.Msg == 0x0288 ? " result=" + m.Result.ToInt64() : ""));
  }
  protected override void OnTextChanged(EventArgs e) { base.OnTextChanged(e); W("event=text text=" + Text); }
}

public static class RefApp {
  public const string Title = "kargui-ime-ref";
  public static IntPtr Handle;

  public static Form Make(string log, int x, int y) {
    var f = new Form();
    f.Text = Title;
    f.StartPosition = FormStartPosition.Manual;
    f.Location = new Point(x, y);
    f.Size = new Size(520, 160);
    var b = new LogBox();
    b.LogPath = log;
    b.Width = 460;
    b.Location = new Point(20, 40);
    b.Font = new Font("Yu Gothic UI", 12);
    f.Controls.Add(b);
    f.Shown += (s, e) => { b.Focus(); b.W("event=ready"); };
    f.FormClosing += (s, e) => b.W("event=close text=" + b.Text);
    return f;
  }

  // 別のスレッドで窓を開き、出るまで待つ（呼んだ側はそのままキーを送れる）
  public static bool StartThread(string log, int x, int y) {
    var ready = new ManualResetEvent(false);
    var t = new Thread(() => {
      Application.EnableVisualStyles();
      var f = Make(log, x, y);
      f.Shown += (s, e) => { Handle = f.Handle; ready.Set(); };
      Application.Run(f);
    });
    t.SetApartmentState(ApartmentState.STA);
    t.IsBackground = true;
    t.Start();
    return ready.WaitOne(15000);
  }
}
