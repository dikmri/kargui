# 読み上げソフトに届く UI オートメーションの出来事を記録する（ナレーターが何を読んだかを、後から記録で確かめるため）。
# 焦点の移動・名前や値やチェックの状態の変化・部品の木の変化を、時刻つきで 1 行ずつ書く。
# 使い方: powershell -NoProfile -ExecutionPolicy Bypass -File tools/uia-events.ps1 -ProcessId 1234 -Out logs/uia-events.log -Seconds 120
#   -Seconds を過ぎるか、窓が閉じたら止まる。ナレーターを使いながら走らせると、読まれた部品の出来事が残る。
param(
  [Parameter(Mandatory = $true)][int]$ProcessId,
  [Parameter(Mandatory = $true)][string]$Out,
  [int]$Seconds = 120
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type -ReferencedAssemblies UIAutomationClient, UIAutomationTypes, WindowsBase -TypeDefinition @"
using System;
using System.IO;
using System.Windows.Automation;
public static class UiaEvents {
  static StreamWriter w;
  static readonly object gate = new object();
  static int pid;
  public static int Count;
  static string Describe(AutomationElement e) {
    try {
      var c = e.Current;
      string type = c.ControlType.ProgrammaticName.Replace("ControlType.", "");
      string extra = "";
      object p;
      if (e.TryGetCurrentPattern(ValuePattern.Pattern, out p)) extra += " value=\"" + ((ValuePattern)p).Current.Value + "\"";
      if (e.TryGetCurrentPattern(TogglePattern.Pattern, out p)) extra += " toggle=" + ((TogglePattern)p).Current.ToggleState;
      return "role=" + type + " name=\"" + c.Name + "\"" + extra;
    } catch (Exception ex) { return "role=? error=" + ex.GetType().Name; }
  }
  static void Log(string ev, AutomationElement e, string extra) {
    string d = e == null ? "" : Describe(e);
    lock (gate) {
      Count++;
      w.WriteLine(DateTime.UtcNow.ToString("yyyy-MM-ddTHH:mm:ss.fffZ") + " event=uia." + ev + " " + d + (extra.Length > 0 ? " " + extra : ""));
      w.Flush();
    }
  }
  public static void Start(int processId, IntPtr hwnd, string path) {
    pid = processId;
    w = new StreamWriter(path, true, new System.Text.UTF8Encoding(false));
    var win = AutomationElement.FromHandle(hwnd);
    Log("start", win, "pid=" + pid);
    Automation.AddAutomationFocusChangedEventHandler((s, a) => {
      var e = s as AutomationElement;
      try { if (e == null || e.Current.ProcessId != pid) return; } catch { return; }
      Log("focus", e, "");
    });
    Automation.AddAutomationPropertyChangedEventHandler(win, TreeScope.Subtree, (s, a) => {
      Log("property", s as AutomationElement, "prop=" + a.Property.ProgrammaticName.Replace("AutomationElementIdentifiers.", "").Replace("PatternIdentifiers.", "") + " new=\"" + a.NewValue + "\"");
    }, AutomationElement.NameProperty, ValuePattern.ValueProperty, TogglePattern.ToggleStateProperty, SelectionItemPattern.IsSelectedProperty);
    Automation.AddStructureChangedEventHandler(win, TreeScope.Subtree, (s, a) => {
      Log("structure", s as AutomationElement, "change=" + a.StructureChangeType);
    });
  }
  public static void Stop() {
    Automation.RemoveAllEventHandlers();
    lock (gate) { w.WriteLine(DateTime.UtcNow.ToString("yyyy-MM-ddTHH:mm:ss.fffZ") + " event=uia.stop count=" + Count); w.Close(); }
  }
}
"@
$p = Get-Process -Id $ProcessId
if ($p.MainWindowHandle -eq [IntPtr]::Zero) { throw '窓が見つからない' }
[UiaEvents]::Start($ProcessId, $p.MainWindowHandle, (Resolve-Path -LiteralPath (Split-Path -Parent $Out)).Path + '\' + (Split-Path -Leaf $Out))
$end = (Get-Date).AddSeconds($Seconds)
while ((Get-Date) -lt $end -and -not $p.HasExited) { Start-Sleep -Milliseconds 200; $p.Refresh() }
[UiaEvents]::Stop()
"uia-events: $([UiaEvents]::Count) 件 → $Out"
