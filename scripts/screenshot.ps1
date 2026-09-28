# Captures part of the screen (layered windows included) to a PNG for checking the UI.
#   -Region top   : 600x60 at the top centre of the primary monitor
#   -Region left  : 60x600 at the left middle of the primary monitor
#   -Region tray  : 500x80 at the bottom-right of the primary monitor
#   -Region panel : 900x900 at the top centre of the primary monitor
param([ValidateSet("top", "left", "tray", "panel")][string]$Region = "top", [string]$Out = "$env:TEMP\claudehud-shot.png")
Add-Type -AssemblyName System.Drawing, System.Windows.Forms
Add-Type -Namespace Native -Name Dpi -MemberDefinition '[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(System.IntPtr v);'
[Native.Dpi]::SetProcessDpiAwarenessContext([IntPtr]::new(-4)) | Out-Null   # PER_MONITOR_AWARE_V2
$b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
switch ($Region) {
    "top"   { $r = [System.Drawing.Rectangle]::new($b.X + $b.Width / 2 - 300, $b.Y, 600, 60) }
    "left"  { $r = [System.Drawing.Rectangle]::new($b.X, $b.Y + $b.Height / 2 - 300, 60, 600) }
    "tray"  { $r = [System.Drawing.Rectangle]::new($b.Right - 500, $b.Bottom - 80, 500, 80) }
    "panel" { $r = [System.Drawing.Rectangle]::new($b.X + $b.Width / 2 - 450, $b.Y, 900, 900) }
}
# Graphics.CopyFromScreen rejects SourceCopy|CaptureBlt at runtime (its enum validation
# only recognises single named values, a long-standing .NET Framework limitation), so
# BitBlt is called directly via P/Invoke with the raw ROP code instead. CAPTUREBLT is
# required to include layered windows (WS_EX_LAYERED, e.g. the strip) in the capture.
Add-Type -Namespace Native -Name Gdi -MemberDefinition '
[DllImport("gdi32.dll")] public static extern bool BitBlt(IntPtr hdcDest, int xDest, int yDest, int w, int h, IntPtr hdcSrc, int xSrc, int ySrc, uint rop);
[DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr hWnd);
[DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr hWnd, IntPtr hDC);
'
$SRCCOPY_CAPTUREBLT = 0x40CC0020
$bmp = [System.Drawing.Bitmap]::new($r.Width, $r.Height)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdcDest = $g.GetHdc()
$hdcSrc = [Native.Gdi]::GetDC([IntPtr]::Zero)
[Native.Gdi]::BitBlt($hdcDest, 0, 0, $r.Width, $r.Height, $hdcSrc, $r.X, $r.Y, $SRCCOPY_CAPTUREBLT) | Out-Null
[Native.Gdi]::ReleaseDC([IntPtr]::Zero, $hdcSrc) | Out-Null
$g.ReleaseHdc($hdcDest)
$bmp.Save($Out)
"saved $Out ($r)"
