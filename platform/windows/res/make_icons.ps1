# 產生工作列模式圖示與輸入法主圖示。
#
# # 為什麼要有這支腳本
#
# .ico 是二進位，改配色或換字都得整組重做。手工用繪圖軟體做八個尺寸
# 乘五張圖太容易失手，寫成腳本則是改一個常數就全部重生。
#
# # 設計取捨
#
# **白字 ＋ 黑外框，不要底色**：工作列可能是淺色也可能是深色，Windows
# 不會幫圖示反色。白字配黑外框兩邊都讀得到——深色列上看到白字，淺色列
# 上看到黑框。省下底色的空間，字還能畫得更大。
#
# **外框畫在下面再蓋白字**：筆畫用粗筆描一次黑邊，再填白色蓋回中間。
# 筆畫是沿路徑置中的，所以筆寬要抓成想要外框厚度的兩倍。
#
# **超取樣**：小尺寸的中日文字筆畫多，直接畫 16px 會糊成一團。改成先
# 畫 4 倍再高品質縮小，筆畫才留得住。
#
# # 用法
#
#     pwsh -File platform/windows/res/make_icons.ps1

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$here = $PSScriptRoot
# 尺寸涵蓋 100%～200% DPI 的工作列，外加設定頁與檔案總管會用到的大圖
$sizes = @(16, 20, 24, 32, 48, 64, 128, 256)

# 字型。
#
# 思源黑體一套涵蓋漢字、注音、假名、拉丁字母，四個圖示才會像同一家人。
#
# 字重與外框厚度是**互相牽制**的：字愈細、框愈粗，16px 下白色內部就
# 愈容易被框吃掉。想換 Heavy 的話把 $FontName 加上 ' Heavy' 即可，
# 兩個常數都在下面，改完重跑這支腳本。
#
# **這只在產生圖示時用得到**——字形已經烘進 .ico，別台電腦沒裝這個
# 字型也不影響輸入法執行。沒裝就退回系統內建的黑體。
$FontName = '思源黑體'
$Fallback = 'Microsoft JhengHei'

# 外框厚度，相對於圖示邊長。
#
# **這個數字的取捨全在 16px 的淺色工作列上**——深色列看的是白字，
# 框多粗都無所謂；淺色列看的是框，框太粗會把白色內部吃光，「通」
# 那種筆畫多的字就糊成一團灰。0.035 是筆畫還留得住的下限附近。
$EdgeRatio = 0.035

# 圖示定義：檔名、字
#
# **ime.ico 與 mode_auto.ico 已經不在這裡產生**（2026-09-02）——那兩個
# 改用設計稿 `doc/icon.svg`，由 `make_icons_from_svg.py` 轉出。這支腳本
# 只負責另外三個模式圖示。
#
# 為什麼不合併成一支：這支用 System.Drawing 畫字（不需要額外套件），
# 那支要解 PNG 與做形態學膨脹（需要 Pillow）。把 Python 依賴限制在
# 「換設計稿才會跑」的那一支，日常重生模式圖示不必裝任何東西。
$specs = @(
    @{ File = 'mode_bopomofo.ico'; Glyph = 'ㄅ' }
    @{ File = 'mode_romaji.ico';   Glyph = 'あ' }
    @{ File = 'mode_english.ico';  Glyph = 'A' }
)

$installed = (New-Object System.Drawing.Text.InstalledFontCollection).Families.Name
if ($installed -notcontains $FontName) {
    Write-Warning "找不到「$FontName」，改用 $Fallback"
    $FontName = $Fallback
}

function New-IconImage([int]$size, [string]$glyph, [string]$fontName) {
    # 先畫 4 倍再縮小——小尺寸的中日文字直接畫會糊掉
    $ss = 4
    $big = $size * $ss
    $bmp = New-Object System.Drawing.Bitmap($big, $big, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = 'AntiAlias'
    $g.Clear([System.Drawing.Color]::Transparent)

    # 字：用路徑而不是 DrawString，才能精準量到實際墨跡範圍並置中。
    # 字型自帶的行高留白很大，直接置中會偏上。
    $fam = New-Object System.Drawing.FontFamily($fontName)
    # **本身已經是 Heavy 的家族沒有 Bold 面**，硬指定會被合成假粗體
    # （筆畫向外糊一圈，跟外框疊在一起變髒）。有才用。
    $style = if ($fam.IsStyleAvailable([System.Drawing.FontStyle]::Bold)) {
        [System.Drawing.FontStyle]::Bold
    } else {
        [System.Drawing.FontStyle]::Regular
    }
    $tp = New-Object System.Drawing.Drawing2D.GraphicsPath
    $fmt = [System.Drawing.StringFormat]::GenericTypographic
    $tp.AddString($glyph, $fam, [int]$style, $big * 0.7, (New-Object System.Drawing.PointF(0, 0)), $fmt)
    $b = $tp.GetBounds()
    if ($b.Width -gt 0 -and $b.Height -gt 0) {
        # 外框會往外長，所以字要縮小一點，整體才不會頂到邊界
        $edge = $big * $script:EdgeRatio
        $target = $big - $edge * 2 - $big * 0.06
        $scale = [Math]::Min($target / $b.Width, $target / $b.Height)
        $mx = New-Object System.Drawing.Drawing2D.Matrix
        $mx.Translate($big / 2, $big / 2)
        $mx.Scale($scale, $scale)
        $mx.Translate(-($b.X + $b.Width / 2), -($b.Y + $b.Height / 2))
        $tp.Transform($mx)

        # 先描黑框再填白字。筆畫沿路徑置中，一半會吃進字裡，
        # 所以筆寬取想要厚度的兩倍，被白色蓋回去後剛好剩外面那一半。
        $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb(230, 0, 0, 0)), ($edge * 2)
        $pen.LineJoin = 'Round'
        $g.DrawPath($pen, $tp)
        $pen.Dispose()
        $g.FillPath([System.Drawing.Brushes]::White, $tp)
    }
    $g.Dispose()

    # 縮到目標尺寸
    $out = New-Object System.Drawing.Bitmap($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g2 = [System.Drawing.Graphics]::FromImage($out)
    $g2.InterpolationMode = 'HighQualityBicubic'
    $g2.PixelOffsetMode = 'HighQuality'
    $g2.DrawImage($bmp, (New-Object System.Drawing.Rectangle(0, 0, $size, $size)))
    $g2.Dispose(); $bmp.Dispose()
    return $out
}

function Save-Ico([string]$path, $images) {
    # ICO 格式：6 byte 檔頭 ＋ 每張 16 byte 目錄 ＋ 影像資料。
    # 影像用 PNG 塞（Vista 以後支援），不必自己組 BMP 與遮罩點陣圖。
    $pngs = @()
    foreach ($im in $images) {
        $ms = New-Object System.IO.MemoryStream
        $im.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $pngs += , $ms.ToArray()
        $ms.Dispose()
    }
    $fs = [System.IO.File]::Create($path)
    $w = New-Object System.IO.BinaryWriter($fs)
    $w.Write([uint16]0); $w.Write([uint16]1); $w.Write([uint16]$images.Count)
    $offset = 6 + 16 * $images.Count
    for ($i = 0; $i -lt $images.Count; $i++) {
        $s = $images[$i].Width
        # 256 在 ICO 目錄裡記成 0——欄位只有一個 byte
        $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
        $w.Write([byte]($(if ($s -ge 256) { 0 } else { $s })))
        $w.Write([byte]0); $w.Write([byte]0)
        $w.Write([uint16]1); $w.Write([uint16]32)
        $w.Write([uint32]$pngs[$i].Length)
        $w.Write([uint32]$offset)
        $offset += $pngs[$i].Length
    }
    foreach ($p in $pngs) { $w.Write($p) }
    $w.Flush(); $fs.Close()
}

foreach ($spec in $specs) {
    $imgs = foreach ($s in $sizes) { New-IconImage $s $spec.Glyph $FontName }
    $out = Join-Path $here $spec.File
    Save-Ico $out $imgs
    foreach ($im in $imgs) { $im.Dispose() }
    "{0,-20} {1}  {2:N1} KB" -f $spec.File, $spec.Glyph, ((Get-Item $out).Length / 1KB)
}
