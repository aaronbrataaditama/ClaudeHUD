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
# Rounded tile inside the 2816 x 1536 artwork (proportional crop of the source)
$tile = [System.Drawing.Rectangle]::new(831, 190, 1155, 1155)

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
