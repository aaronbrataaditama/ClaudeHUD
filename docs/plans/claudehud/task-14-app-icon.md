# Task 14: App icon asset

**Goal:** Generate `assets/claudehud.ico` once from the artwork (full art at 256/128/64/48/32 px, a simplified tile at 24/20/16 px), commit it, and embed it in the exe so Explorer shows it and the panel header (Task 18) can load it.

**Spec:** §5.1 (read it: crop rectangle, rounded mask, simplified tile, pixel map, colours). Visual reference: `claudehud-mockup.html` §6.

**Files:**
- Move: `ClaudeHUD_icon.jpg` → `assets/ClaudeHUD_icon.jpg` (`git mv`)
- Modify: `claudehud-mockup.html` (image path)
- Create: `scripts/make-icon.ps1`
- Create (generated, committed): `assets/claudehud.ico`
- Modify: `assets/claudehud.rc`

**Interfaces:**
- Consumes: nothing from the crate.
- Produces: icon resource id **1** in the exe (`MAKEINTRESOURCE(1)`), used by Task 16 (window class icon) and Task 18 (panel header via `LoadIconWithScaleDown`).

---

- [ ] **Step 1: Move the artwork and fix the mockup path**

```powershell
git mv ClaudeHUD_icon.jpg assets/ClaudeHUD_icon.jpg
```

In `claudehud-mockup.html` replace the one occurrence of `url(ClaudeHUD_icon.jpg)` with `url(assets/ClaudeHUD_icon.jpg)`, and the text `<code>ClaudeHUD_icon.jpg</code>` with `<code>assets/ClaudeHUD_icon.jpg</code>`. Open the mockup in a browser and check the icons still show.

- [ ] **Step 2: Write the generator**

`scripts/make-icon.ps1`:

```powershell
# Builds assets/claudehud.ico from assets/ClaudeHUD_icon.jpg (spec §5.1).
# 256/128/64/48/32 px: the artwork's rounded tile. 24/20/16 px: simplified tile
# (gradient + pixel creature), because the orbits turn to noise below 32 px.
param(
    [string]$Source = (Join-Path $PSScriptRoot "..\assets\ClaudeHUD_icon.jpg"),
    [string]$Out = (Join-Path $PSScriptRoot "..\assets\claudehud.ico")
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing

$src = [System.Drawing.Image]::FromFile((Resolve-Path $Source).Path)
# Rounded tile inside the 2000 x 1091 artwork
$tile = [System.Drawing.Rectangle]::new(590, 135, 820, 820)

function New-RoundedPath([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $d = 2 * $r
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    return $p
}

function New-Canvas([int]$size) {
    $bmp = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.Clear([System.Drawing.Color]::Transparent)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
    $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
    return @($bmp, $g)
}

# Master: the tile at 512 px, masked with a rounded square (radius 22 %)
$scaled = [System.Drawing.Bitmap]::new(512, 512)
$gs = [System.Drawing.Graphics]::FromImage($scaled)
$gs.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$gs.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$gs.DrawImage($src, [System.Drawing.Rectangle]::new(0, 0, 512, 512), $tile, [System.Drawing.GraphicsUnit]::Pixel)
$gs.Dispose()
$master, $g = New-Canvas 512
$g.FillPath([System.Drawing.TextureBrush]::new($scaled), (New-RoundedPath 0 0 512 512 (512 * 0.22)))
$g.Dispose()

function New-Full([int]$size) {
    $bmp, $g = New-Canvas $size
    $g.DrawImage($master, 0, 0, $size, $size)
    $g.Dispose()
    return $bmp
}

$creature = @("..########..", "..#o####o#..", "############", "############", "..########..", "..#.#..#.#..", "..#.#..#.#..")
function New-Simple([int]$size) {
    $bmp, $g = New-Canvas $size
    $s = $size / 16.0
    $grad = [System.Drawing.Drawing2D.LinearGradientBrush]::new(
        [System.Drawing.PointF]::new(0, 0), [System.Drawing.PointF]::new($size, $size),
        [System.Drawing.ColorTranslator]::FromHtml("#2F5BC4"), [System.Drawing.ColorTranslator]::FromHtml("#4A1F8C"))
    $g.FillPath($grad, (New-RoundedPath 0 0 $size $size (3.6 * $s)))
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::None
    $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::Half
    $body = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml("#DE7356"))
    $eye = [System.Drawing.SolidBrush]::new([System.Drawing.ColorTranslator]::FromHtml("#141414"))
    for ($y = 0; $y -lt 7; $y++) {
        for ($x = 0; $x -lt 12; $x++) {
            $c = $creature[$y][$x]
            if ($c -eq '.') { continue }
            $b = if ($c -eq 'o') { $eye } else { $body }
            $g.FillRectangle($b, [float]((2 + $x) * $s), [float]((4.5 + $y) * $s), [float]$s, [float]$s)
        }
    }
    $g.Dispose()
    return $bmp
}

$frames = @()
foreach ($size in 256, 128, 64, 48, 32) { $frames += , @($size, (New-Full $size)) }
foreach ($size in 24, 20, 16) { $frames += , @($size, (New-Simple $size)) }

# ICO container with PNG-compressed entries (valid on Windows Vista and later)
$pngs = foreach ($f in $frames) {
    $m = [System.IO.MemoryStream]::new()
    $f[1].Save($m, [System.Drawing.Imaging.ImageFormat]::Png)
    , $m.ToArray()
}
$ms = [System.IO.MemoryStream]::new()
$bw = [System.IO.BinaryWriter]::new($ms)
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$frames.Count)
$offset = 6 + 16 * $frames.Count
for ($i = 0; $i -lt $frames.Count; $i++) {
    $size = $frames[$i][0]
    $data = $pngs[$i]
    $dim = if ($size -ge 256) { 0 } else { $size }
    $bw.Write([byte]$dim); $bw.Write([byte]$dim); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32)
    $bw.Write([uint32]$data.Length); $bw.Write([uint32]$offset)
    $offset += $data.Length
}
foreach ($data in $pngs) { $bw.Write([byte[]]$data) }
$bw.Flush()
$outFull = Join-Path (Resolve-Path (Split-Path $Out -Parent)).Path (Split-Path $Out -Leaf)
[System.IO.File]::WriteAllBytes($outFull, $ms.ToArray())
"Wrote $outFull ($($ms.Length) bytes, $($frames.Count) frames)"
```

- [ ] **Step 3: Generate and inspect**

Run: `powershell -ExecutionPolicy Bypass -File scripts/make-icon.ps1`
Expected: `Wrote …\assets\claudehud.ico (60000–250000 bytes, 8 frames)`.

Inspect two frames by writing them to the scratch folder and viewing them with the Read tool:

```powershell
Add-Type -AssemblyName System.Drawing
$ico = [System.Drawing.Icon]::new("assets\claudehud.ico", 256, 256); $ico.ToBitmap().Save("$env:TEMP\claudehud-256.png")
$ico = [System.Drawing.Icon]::new("assets\claudehud.ico", 16, 16);   $ico.ToBitmap().Save("$env:TEMP\claudehud-16.png")
```

Expected: 256 px shows the orbits and the orange creature with transparent rounded corners, and no white or purple fringe from the JPG background. 16 px shows a blue-violet rounded square with the orange creature and two dark eyes. If the 256 px corners show a light fringe, shrink the crop by 4 px on every side (`590+4, 135+4, 812, 812`) and regenerate.

If the file is over 400 KB, re-save only the 256 px frame as a smaller PNG by drawing it at 256 from a 256 master instead of 512. The exe budget is 2 MB.

- [ ] **Step 4: Embed it**

`assets/claudehud.rc`:

```
1 ICON "claudehud.ico"
1 24 "claudehud.manifest"
```

Run: `cargo build --release`
Then verify Explorer's icon:

```powershell
Add-Type -AssemblyName System.Drawing
[System.Drawing.Icon]::ExtractAssociatedIcon((Resolve-Path "target\release\claudehud.exe").Path).ToBitmap().Save("$env:TEMP\claudehud-exe.png")
```

View `%TEMP%\claudehud-exe.png`. Expected: the ClaudeHUD icon, not the generic exe icon.

- [ ] **Step 5: Size check and commit**

Run: `powershell -ExecutionPolicy Bypass -File scripts/check.ps1`
Expected: passes; the exe grows by roughly the `.ico` size and stays well under 2 MB.

```powershell
git add assets/ClaudeHUD_icon.jpg assets/claudehud.ico assets/claudehud.rc scripts/make-icon.ps1 claudehud-mockup.html
git commit -m "feat(icon): generate and embed the ClaudeHUD app icon"
```
