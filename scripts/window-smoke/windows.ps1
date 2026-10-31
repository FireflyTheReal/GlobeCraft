# Launches GlobeCraft, then drives its native title bar through the Windows API:
# screenshot, maximize, minimize, restore, close. Writes PNGs and report.txt to $Out.
param([string]$Exe, [string]$Out)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $Out | Out-Null
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class W {
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int c);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern int GetWindowLong(IntPtr h, int i);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  public struct RECT { public int L, T, R, B; }
}
"@
$report = New-Object System.Collections.Generic.List[string]
function Shot($name) {
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
  $bmp.Save((Join-Path $Out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  $g.Dispose(); $bmp.Dispose()
}
function State($h, $label) {
  $r = New-Object W+RECT; [W]::GetWindowRect($h, [ref]$r) | Out-Null
  $line = "$label`: zoomed=$([W]::IsZoomed($h)) iconic=$([W]::IsIconic($h)) rect=$($r.L),$($r.T),$($r.R),$($r.B)"
  $report.Add($line); Write-Host $line
}
$p = Start-Process -FilePath $Exe -PassThru
$h = [IntPtr]::Zero
for ($i = 0; $i -lt 120 -and $h -eq [IntPtr]::Zero; $i++) {
  Start-Sleep -Milliseconds 500; $p.Refresh(); $h = $p.MainWindowHandle
}
if ($h -eq [IntPtr]::Zero) { throw 'no window appeared' }
Start-Sleep -Seconds 15
$t = New-Object System.Text.StringBuilder 256; [W]::GetWindowText($h, $t, 256) | Out-Null
$style = [W]::GetWindowLong($h, -16)
# WS_CAPTION 0x00C00000, WS_SYSMENU 0x00080000, WS_MINIMIZEBOX 0x00020000, WS_MAXIMIZEBOX 0x00010000
$report.Add("title: $t")
$report.Add(("style: 0x{0:X8} caption={1} sysmenu={2} minbox={3} maxbox={4}" -f $style,
  [bool]($style -band 0x00C00000), [bool]($style -band 0x00080000),
  [bool]($style -band 0x00020000), [bool]($style -band 0x00010000)))
[W]::SetForegroundWindow($h) | Out-Null
State $h 'launched'; Shot '01-launched'
[W]::ShowWindow($h, 3) | Out-Null; Start-Sleep -Seconds 3; State $h 'maximized'; Shot '02-maximized'
[W]::ShowWindow($h, 6) | Out-Null; Start-Sleep -Seconds 2; State $h 'minimized'; Shot '03-minimized'
[W]::ShowWindow($h, 9) | Out-Null; Start-Sleep -Seconds 3; State $h 'restored'; Shot '04-restored'
[W]::ShowWindow($h, 9) | Out-Null; Start-Sleep -Seconds 2; State $h 'restored-again'; Shot '05-unmaximized'
$closed = Get-Date
[W]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null   # WM_CLOSE, as the X button
if ($p.WaitForExit(30000)) {
  $report.Add(("closed: exit {0} after {1:N1} s" -f $p.ExitCode, ((Get-Date) - $closed).TotalSeconds))
} else { $report.Add('closed: still running after 30 s'); $p.Kill() }
$report | Set-Content (Join-Path $Out 'report.txt')
