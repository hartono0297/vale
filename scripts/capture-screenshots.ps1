<#
.SYNOPSIS
  Regenerates the README screenshots in assets/screenshots from the debug
  build's demo modes.

.DESCRIPTION
  Each shot starts `vale.exe` in a demo mode (`--demo`,
  `--demo-robot`, `--demo-cli`, `--demo-devices`, `--demo-editor`,
  `--settings`) with
  DI_LANG=en, waits for its window, and copies it from the screen twice: once
  over a black backdrop and once over a white one. The two copies give each
  pixel's alpha, so the PNG keeps the island's rounded shape (and the
  editors' rounded corners) on a transparent background, cropped to the
  window. Nothing is clicked; the demo process is killed after each shot.

  Run from the repository root in Windows PowerShell after `cargo build`.
  Stop a running island first (Stop-Process -Name vale); it would
  sit over the demo windows. The backdrop briefly covers the window being
  captured, so leave the mouse and keyboard alone while it runs.

  The calendar and reminder scenes are built around the time now (see
  docs/memory/demo-data-time-of-day.md): look at the PNGs before committing.

.EXAMPLE
  cargo build; .\scripts\capture-screenshots.ps1
  .\scripts\capture-screenshots.ps1 -Only calendar,cli
#>
param(
    [string]$Exe = 'target\debug\vale.exe',
    [string]$Out = 'assets\screenshots',
    # Names of the shots to take (see $shots below); all when empty.
    [string[]]$Only = @(),
    # Window mode for the settings window and the editors.
    [ValidateSet('dark', 'light')]
    [string]$Window = 'dark'
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -ReferencedAssemblies System.Drawing -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;

public static class Shot {
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc f, IntPtr p);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr h, int i);
    [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);
    delegate bool EnumProc(IntPtr h, IntPtr p);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }

    const int GWL_EXSTYLE = -20;
    const int WS_EX_LAYERED = 0x80000;

    /// Visible top-level windows of a process, largest first.
    public static IntPtr[] Windows(int pid) {
        var found = new List<IntPtr>();
        EnumWindows((h, p) => {
            uint owner;
            GetWindowThreadProcessId(h, out owner);
            if (owner == (uint)pid && IsWindowVisible(h)) found.Add(h);
            return true;
        }, IntPtr.Zero);
        found.Sort((a, b) => Area(Bounds(b)) - Area(Bounds(a)));
        return found.ToArray();
    }

    static int Area(Rectangle r) { return r.Width * r.Height; }

    public static bool Layered(IntPtr h) {
        return (GetWindowLong(h, GWL_EXSTYLE) & WS_EX_LAYERED) != 0;
    }

    /// The window as drawn: DWM's frame bounds (no shadow), or the plain
    /// rect for a layered window.
    public static Rectangle Bounds(IntPtr h) {
        RECT r;
        if (Layered(h) || DwmGetWindowAttribute(h, 9, out r, 16) != 0) GetWindowRect(h, out r);
        return Rectangle.FromLTRB(r.L, r.T, r.R, r.B);
    }

    public static void Raise(IntPtr h) {
        // HWND_TOPMOST, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE
        SetWindowPos(h, new IntPtr(-1), 0, 0, 0, 0, 0x13);
    }

    public static Bitmap Grab(Rectangle r) {
        var bmp = new Bitmap(r.Width, r.Height, PixelFormat.Format32bppArgb);
        using (var g = Graphics.FromImage(bmp)) g.CopyFromScreen(r.Location, Point.Empty, r.Size);
        return bmp;
    }

    static int[] Pixels(Bitmap b) {
        var data = b.LockBits(new Rectangle(0, 0, b.Width, b.Height), ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
        var px = new int[b.Width * b.Height];
        Marshal.Copy(data.Scan0, px, 0, px.Length);
        b.UnlockBits(data);
        return px;
    }

    /// Alpha from the same pixels over black and over white. With `corners`
    /// > 0 only that many pixels at each corner may be transparent (an opaque
    /// window: a blinking caret between the two copies must not punch a hole).
    /// Cropped to what is not fully transparent.
    public static Bitmap Matte(Bitmap black, Bitmap white, int corners) {
        int w = black.Width, h = black.Height;
        var k = Pixels(black);
        var wh = Pixels(white);
        var outPx = new int[w * h];
        int minX = w, minY = h, maxX = -1, maxY = -1;
        for (int y = 0; y < h; y++) {
            for (int x = 0; x < w; x++) {
                int i = y * w + x;
                int a;
                bool edge = corners <= 0
                    || ((x < corners || x >= w - corners) && (y < corners || y >= h - corners));
                int kr = (k[i] >> 16) & 255, kg = (k[i] >> 8) & 255, kb = k[i] & 255;
                if (edge) {
                    int wr = (wh[i] >> 16) & 255, wg = (wh[i] >> 8) & 255, wb = wh[i] & 255;
                    int diff = Math.Max(wr - kr, Math.Max(wg - kg, wb - kb));
                    a = 255 - Math.Max(0, Math.Min(255, diff));
                } else {
                    a = 255;
                }
                if (a <= 2) { outPx[i] = 0; continue; }
                int r = Math.Min(255, kr * 255 / a), g = Math.Min(255, kg * 255 / a), b = Math.Min(255, kb * 255 / a);
                outPx[i] = (a << 24) | (r << 16) | (g << 8) | b;
                if (x < minX) minX = x;
                if (x > maxX) maxX = x;
                if (y < minY) minY = y;
                if (y > maxY) maxY = y;
            }
        }
        if (maxX < 0) throw new Exception("nothing drawn");
        var crop = Rectangle.FromLTRB(minX, minY, maxX + 1, maxY + 1);
        var result = new Bitmap(crop.Width, crop.Height, PixelFormat.Format32bppArgb);
        var data = result.LockBits(new Rectangle(0, 0, crop.Width, crop.Height), ImageLockMode.WriteOnly, PixelFormat.Format32bppArgb);
        for (int y = 0; y < crop.Height; y++) {
            Marshal.Copy(outPx, (crop.Y + y) * w + crop.X, data.Scan0 + y * data.Stride, crop.Width);
        }
        result.UnlockBits(data);
        return result;
    }
}
'@

[void][Shot]::SetProcessDPIAware()
if (-not (Test-Path $Exe)) { throw "No ${Exe}: run cargo build first." }
if (Get-Process vale -ErrorAction SilentlyContinue | Where-Object { $_.Path -ne (Resolve-Path $Exe).Path }) {
    Write-Warning 'Another vale is running and may cover the demo windows.'
}
New-Item -ItemType Directory -Force $Out | Out-Null

# name, arguments, extra environment, settle time (ms), wheel notches (settings).
$shots = @(
    @{ name = 'calendar'; args = '--demo'; env = @{ DI_DEMO_STEP = 'cal-month' } }
    @{ name = 'calendar-day'; args = '--demo'; env = @{ DI_DEMO_STEP = 'cal-day' } }
    @{ name = 'notch'; args = '--demo'; env = @{ DI_DEMO_STEP = 'notch:cal-week' } }
    @{ name = 'pomodoro'; args = '--demo'; env = @{ DI_DEMO_STEP = 'pomodoro' } }
    @{ name = 'reminders'; args = '--demo'; env = @{ DI_DEMO_STEP = 'reminders-5' } }
    @{ name = 'notes'; args = '--demo'; env = @{ DI_DEMO_STEP = 'notes-4' } }
    @{ name = 'music'; args = '--demo'; env = @{ DI_DEMO_STEP = 'media' } }
    @{ name = 'music-volume'; args = '--demo'; env = @{ DI_DEMO_STEP = 'music-volume' } }
    @{ name = 'notch-volume'; args = '--demo'; env = @{ DI_DEMO_STEP = 'notch:media-volume' } }
    @{ name = 'music-lyrics'; args = '--demo'; env = @{ DI_DEMO_STEP = 'media-lyrics-on' } }
    @{ name = 'mixer'; args = '--demo'; env = @{ DI_DEMO_STEP = 'mixer' } }
    @{ name = 'shelf'; args = '--demo'; env = @{ DI_DEMO_STEP = 'shelf' } }
    @{ name = 'shelf-empty'; args = '--demo'; env = @{ DI_DEMO_STEP = 'shelf-empty' } }
    @{ name = 'shelf-drop'; args = '--demo'; env = @{ DI_DEMO_STEP = 'shelf-drop' } }
    @{ name = 'shelf-drop-files'; args = '--demo'; env = @{ DI_DEMO_STEP = 'shelf-drop-files' } }
    @{ name = 'search'; args = '--demo'; env = @{ DI_DEMO_STEP = 'search' } }
    # Files and folders among the apps (made-up names, drawn icons).
    @{ name = 'search-files'; args = '--demo'; env = @{ DI_DEMO_STEP = 'search-files' } }
    @{ name = 'shortcuts-custom'; args = '--demo'; env = @{ DI_DEMO_STEP = 'shortcuts-custom' } }
    @{ name = 'shortcuts'; args = '--demo'; env = @{ DI_DEMO_STEP = 'shortcuts' } }
    # The Clipboard page (fixed data, never the real clipboard), a search,
    # and "Copied" in the notch.
    @{ name = 'clipboard'; args = '--demo'; env = @{ DI_DEMO_STEP = 'clipboard' } }
    @{ name = 'clipboard-long'; args = '--demo'; env = @{ DI_DEMO_STEP = 'clipboard-long' } }
    @{ name = 'clipboard-search'; args = '--demo'; env = @{ DI_DEMO_STEP = 'clipboard-search' } }
    @{ name = 'clipboard-copied'; args = '--demo'; env = @{ DI_DEMO_STEP = 'notch:clipboard-copied' } }
    # The screenshot preview with fixed figures and a drawn picture (never a
    # real capture, never the real clipboard or folder), then "Saved" and
    # "Deleted".
    @{ name = 'screenshot'; args = '--demo'; env = @{ DI_DEMO_STEP = 'notch:screenshot' } }
    @{ name = 'screenshot-saved'; args = '--demo'; env = @{ DI_DEMO_STEP = 'notch:screenshot-saved' } }
    @{ name = 'screenshot-deleted'; args = '--demo'; env = @{ DI_DEMO_STEP = 'notch:screenshot-deleted' } }
    # The System page with fixed figures (never this machine's), and the
    # low-disk notice with Bata looking low.
    @{ name = 'system'; args = '--demo'; env = @{ DI_DEMO_STEP = 'system' } }
    @{ name = 'system-warning'; args = '--demo-robot'; env = @{ DI_DEMO_STEP = 'notch:bata-sistem' } }
    # The Devices page with the design's three made-up drives (never this
    # machine's, never an eject): SanDisk Ultra, WD Elements refused by Excel,
    # Kartu SD.
    @{ name = 'devices'; args = '--demo-devices'; env = @{ DI_DEMO_STEP = 'devices' } }
    # The cleaner over the System page with the design's sizes (never a real
    # scan), its button armed, and the happy notice when it is done.
    @{ name = 'cleaner'; args = '--demo'; env = @{ DI_DEMO_STEP = 'cleaner' } }
    @{ name = 'cleaner-confirm'; args = '--demo'; env = @{ DI_DEMO_STEP = 'cleaner-confirm' } }
    @{ name = 'cleaner-done'; args = '--demo'; env = @{ DI_DEMO_STEP = 'notch:cleaner-done' } }
    # The cleaner's "Otomatis · Mole" mode with the design's fixed data: Mole
    # is never looked for or run. Its preview, the armed button, the log while
    # it cleans, the measured result, the "Cara memasang" card when Mole is
    # missing, and the done notice beside its "Log" chip.
    @{ name = 'cleaner-mole'; args = '--demo'; env = @{ DI_DEMO_STEP = 'cleaner-mole' } }
    @{ name = 'cleaner-mole-confirm'; args = '--demo'; env = @{ DI_DEMO_STEP = 'cleaner-mole-confirm' } }
    @{ name = 'cleaner-mole-log'; args = '--demo'; env = @{ DI_DEMO_STEP = 'cleaner-mole-log' } }
    @{ name = 'cleaner-mole-result'; args = '--demo'; env = @{ DI_DEMO_STEP = 'cleaner-mole-result' } }
    @{ name = 'cleaner-mole-missing'; args = '--demo'; env = @{ DI_DEMO_STEP = 'cleaner-mole-missing' } }
    @{ name = 'cleaner-mole-done'; args = '--demo'; env = @{ DI_DEMO_STEP = 'notch:mole-done' } }
    # A lyric line too long for the notch, mid-scroll with its right edge faded.
    @{ name = 'lyric-long'; args = '--demo-robot'; env = @{ DI_DEMO_STEP = 'notch:bata-lirik-long'; DI_LYRIC_SCROLL = '4000' } }
    @{ name = 'cli'; args = '--demo-cli'; env = @{ DI_DEMO_STEP = 'cli-done' } }
    # The CLI followed File Explorer, and its folder menu open.
    @{ name = 'cli-explorer'; args = '--demo-cli'; env = @{ DI_DEMO_STEP = 'cli-from-explorer' } }
    @{ name = 'cli-folder-menu'; args = '--demo-cli'; env = @{ DI_DEMO_STEP = 'cli-folder-menu' } }
    @{ name = 'robot'; args = '--demo-robot'; env = @{ DI_DEMO_STEP = 'bata-sheet' } }
    @{ name = 'greeting'; args = '--demo-robot'; env = @{ DI_DEMO_STEP = 'bata-sapaan' } }
    @{ name = 'editor'; args = '--demo-editor event'; env = @{ DI_WINDOW = $Window } }
    @{ name = 'reminder-editor'; args = '--demo-editor reminder-weekly'; env = @{ DI_WINDOW = $Window } }
    # Appearance scrolled to its end: the assistant (Bata) rows.
    @{ name = 'settings'; args = "--settings appearance $Window en"; env = @{}; wheel = 6 }
    # Appearance scrolled to its end with two own tiles in the Pintasan card.
    @{ name = 'settings-tiles'; args = "--settings appearance $Window en"; env = @{ DI_TILES = '1' }; wheel = 40 }
    # General scrolled to its end: the alerts' rows.
    @{ name = 'settings-general'; args = "--settings general $Window en"; env = @{}; wheel = 10 }
    # Tombol pintas: the five hotkeys with their fields.
    @{ name = 'settings-hotkeys'; args = "--settings hotkeys $Window en"; env = @{} }
    @{ name = 'settings-clipboard'; args = "--settings clipboard $Window en"; env = @{} }
    # Clipboard scrolled to its end: the screenshot preview's card.
    @{ name = 'settings-screenshot'; args = "--settings clipboard $Window en"; env = @{}; wheel = 6 }
)

$backdrop = New-Object System.Windows.Forms.Form
$backdrop.FormBorderStyle = 'None'
$backdrop.ShowInTaskbar = $false
$backdrop.StartPosition = 'Manual'

function Paint-Backdrop([System.Drawing.Color]$color) {
    $backdrop.BackColor = $color
    $backdrop.Refresh()
    [System.Windows.Forms.Application]::DoEvents()
    Start-Sleep -Milliseconds 150
}

foreach ($shot in $shots) {
    if ($Only.Count -and $Only -notcontains $shot.name) { continue }
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = (Resolve-Path $Exe).Path
    $psi.Arguments = $shot.args
    $psi.UseShellExecute = $false
    $psi.EnvironmentVariables['DI_LANG'] = 'en'
    foreach ($k in $shot.env.Keys) { $psi.EnvironmentVariables[$k] = $shot.env[$k] }
    $proc = [System.Diagnostics.Process]::Start($psi)
    try {
        $hwnd = [IntPtr]::Zero
        for ($i = 0; $i -lt 50 -and $hwnd -eq [IntPtr]::Zero; $i++) {
            Start-Sleep -Milliseconds 200
            $hwnd = @([Shot]::Windows($proc.Id)) | Select-Object -First 1
            if ($null -eq $hwnd) { $hwnd = [IntPtr]::Zero }
        }
        if ($hwnd -eq [IntPtr]::Zero) { throw "$($shot.name): no window" }
        # Morph, fonts and the first paint.
        Start-Sleep -Milliseconds 2500
        if ($shot.wheel) {
            # WM_MOUSEWHEEL, one notch down each.
            for ($n = 0; $n -lt $shot.wheel; $n++) {
                [void][Shot]::PostMessage($hwnd, 0x020A, [IntPtr]([int](-120 -shl 16)), [IntPtr]::Zero)
            }
            Start-Sleep -Milliseconds 800
        }
        $layered = [Shot]::Layered($hwnd)
        $rect = [Shot]::Bounds($hwnd)
        $backdrop.Bounds = [System.Drawing.Rectangle]::Inflate($rect, 24, 24)
        $backdrop.Show()
        [Shot]::Raise($hwnd)
        Paint-Backdrop ([System.Drawing.Color]::Black)
        $black = [Shot]::Grab($rect)
        Paint-Backdrop ([System.Drawing.Color]::White)
        $white = [Shot]::Grab($rect)
        $backdrop.Hide()
        $png = [Shot]::Matte($black, $white, $(if ($layered) { 0 } else { 12 }))
        $path = Join-Path $Out "$($shot.name).png"
        $png.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
        '{0,-20} {1}x{2}  {3:N0} KB' -f $shot.name, $png.Width, $png.Height, ((Get-Item $path).Length / 1KB)
        $black.Dispose(); $white.Dispose(); $png.Dispose()
    } finally {
        if (-not $proc.HasExited) { $proc.Kill() }
        $proc.WaitForExit()
    }
}
$backdrop.Dispose()
