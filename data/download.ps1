# 下載 Phase 1 三個語言引擎所需的詞庫原始檔。
#
# 這些檔案不進版控（見 .gitignore）：McBopomofo 的映射表約 5MB、
# mozc 的辭典分片共約 60MB，不適合放進 git。每台開發機需要各自
# 執行一次本腳本。
#
# 授權（已於 2026-08-23 逐項查證，見開發文件.md §2.3）：
#   - McBopomofo BPMFMappings.txt / BPMFBase.txt : MIT
#   - mozc dictionary_oss/*.txt                  : BSD-3-Clause
#   - hermitdave/FrequencyWords（en 與 zh_tw）    : MIT

$ErrorActionPreference = "Stop"
$dataDir = $PSScriptRoot

function Get-File($url, $dest) {
    if (Test-Path $dest) {
        Write-Output "已存在，略過: $dest"
        return
    }
    Write-Output "下載: $url"
    Invoke-WebRequest -Uri $url -OutFile $dest
}

# 把教育部字頻總表（Big5 + 表格框線）轉成「字 頻次」的純文字。
#
# 原始格式長這樣（│ 是全形直線）：
#     │     1  │ 的 │白│08│  32739 │   32739│  1.651 │
#     序號       字   部首 筆畫  頻次    累積     百分比
#
# 用 │ 切欄取第 2 欄（字）與倒數第 3 欄（頻次）。**不能用正規表示式
# 硬比對整行**——部首欄有些字 Big5 解不開，會把欄位切歪，實測「夏」
# 就是這樣漏掉的。
function Convert-MoeCharFreq($ZipPath, $OutPath) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    # .NET Core 之後 Big5(950) 要先註冊 CodePages provider 才拿得到。
    [System.Text.Encoding]::RegisterProvider(
        [System.Text.CodePagesEncodingProvider]::Instance)

    $archive = [System.IO.Compression.ZipFile]::OpenRead($ZipPath)
    try {
        $entry = $archive.Entries | Where-Object { $_.Name -eq 'BIAU1.TXT' }
        if (-not $entry) { throw "BIAU1.zip 裡找不到 BIAU1.TXT" }
        $reader = New-Object System.IO.StreamReader(
            $entry.Open(), [System.Text.Encoding]::GetEncoding(950))
        $text = $reader.ReadToEnd()
        $reader.Close()
    } finally {
        $archive.Dispose()
    }

    $sep = [char]0x2502   # │
    $out = New-Object System.Collections.Generic.List[string]
    foreach ($line in ($text -split "`n")) {
        $cols = $line.Split($sep)
        if ($cols.Count -lt 4) { continue }
        $ch = $cols[1].Trim()
        if ($ch.Length -ne 1) { continue }
        $code = [int][char]$ch
        # 只收漢字。表裡還有「○」這種符號。
        if ($code -lt 0x4E00 -or $code -gt 0x9FFF) { continue }
        $freqCol = $cols[$cols.Count - 3]
        if ($freqCol -match '(\d+)\s*$') {
            $out.Add("$ch $($Matches[1])")
        }
    }
    # 官方說 5731 字，Big5 解不開的罕見字（頻次都 <=49）會少幾筆。
    if ($out.Count -lt 5000) {
        throw "字頻表只解析出 $($out.Count) 筆，格式可能變了"
    }
    Set-Content -Path $OutPath -Value $out -Encoding UTF8
    Write-Output "  字頻總表 $($out.Count) 筆 -> $OutPath"
}

# 把三份詞頻資料合併成一份「詞 每百萬詞頻×1000」的純文字。
#
# ## 為什麼要三份合併
#
# 沒有任何一份單獨夠用，各有各的缺口（2026-08-25 實測）：
#
# | 資料 | 詞數 | 問題 |
# |---|---|---|
# | 國教院《通用詞頻表》 | 16.4 萬 | 「不是」「一個」「你好」被切成兩個詞，整詞統計幾乎是零 |
# | 教育部《詞頻總表》 | 4.6 萬 | 詞少，但補得到國教院沒有的 |
# | hermitdave zh_tw_50k | 5 萬 | **字幕語料，混火星文/簡體/英文**，但口語常用組合的整詞統計完整 |
#
# 字幕語料的污染有多嚴重：依頻次排序的前幾名是 `и`(23萬) `琌`(12萬)
# `ぃ`(10萬) `硂` `τ` `璶`——**火星文比真正的中文詞還高頻**。
# 但它是唯一收錄「不是」(9.4萬) 的一份，而正式語料庫裡「不是」只有 64。
#
# 所以：**國教院為主 → 教育部補缺 → 字幕語料過濾後補「被切開的常用組合」**。
#
# ## 過濾字幕語料的判準
#
# 用 `BPMFBase.txt`（注音詞庫的單字表，以《國字標準字體表》為據）當
# 白名單：每個字都查得到才留。這一刀擋掉 6530 筆——英文、注音符號、
# 日文假名、希臘字母，以及「眔」「疭」「穝」這類火星文漢字和
# 「们」「说」「临」這類簡體字。實測**沒有誤殺真的中文詞**。
#
# ## 三份怎麼合
#
# 全部先換算成「每百萬詞頻」才可比——三份的語料庫大小不同，絕對次數
# 直接比會錯得離譜。同一個詞在多份都有時取**最大值**：那代表「至少有
# 一份語料庫認為它這麼常用」，而缺口才是我們要補的東西。
#
# 國教院那份分書面語/口語/新聞三欄，用加權平均（口語 0.4、其餘各 0.3）
# ——輸入法打的多半是口語。
function Convert-WordFreq($NaerXlsx, $MoeZip, $BpmfBase, $SubtitleTxt, $CharFreq, $OutPath) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.Text.Encoding]::RegisterProvider(
        [System.Text.CodePagesEncodingProvider]::Instance)
    $utf8 = [System.Text.Encoding]::UTF8
    $merged = New-Object 'System.Collections.Generic.Dictionary[string,double]'

    # ── 一、國教院（xlsx）──────────────────────────────
    # xlsx 是 zip 裝的 XML。詞字串放在 sharedStrings.xml，儲存格只存索引。
    $zip = [System.IO.Compression.ZipFile]::OpenRead($NaerXlsx)
    try {
        $e = $zip.Entries | Where-Object { $_.FullName -eq 'xl/sharedStrings.xml' }
        $sr = New-Object System.IO.StreamReader($e.Open(), $utf8)
        $ssXml = $sr.ReadToEnd(); $sr.Close()
        # 標題列用了 rich text（<si><r><t>），所以要把 <si> 裡所有 <t> 串起來
        $words = [regex]::Matches($ssXml, '(?s)<si>(.*?)</si>') | ForEach-Object {
            -join ([regex]::Matches($_.Groups[1].Value, '<t[^>]*>(.*?)</t>') |
                   ForEach-Object { $_.Groups[1].Value })
        }
        $ssXml = $null

        $e2 = $zip.Entries | Where-Object { $_.FullName -eq 'xl/worksheets/sheet1.xml' }
        $sr2 = New-Object System.IO.StreamReader($e2.Open(), $utf8)
        $sheet = $sr2.ReadToEnd(); $sr2.Close()
    } finally {
        $zip.Dispose()
    }
    # B=詞 D=書面語每百萬 G=口語每百萬 J=新聞每百萬
    $cellRe = [regex]'<c r="([A-L])\d+"[^>]*?(?: t="(s)")?[^>]*>(?:<v>(.*?)</v>)?</c>'
    $naerCount = 0
    foreach ($rm in [regex]::Matches($sheet, '(?s)<row [^>]*>(.*?)</row>')) {
        $cells = @{}
        foreach ($cm in $cellRe.Matches($rm.Groups[1].Value)) {
            $cells[$cm.Groups[1].Value] = @{
                IsStr = ($cm.Groups[2].Value -eq 's'); V = $cm.Groups[3].Value
            }
        }
        if (-not $cells.ContainsKey('B') -or -not $cells['B'].IsStr) { continue }
        $w = $words[[int]$cells['B'].V]
        if (-not $w) { continue }
        $get = { param($col) if ($cells.ContainsKey($col) -and $cells[$col].V) { [double]$cells[$col].V } else { 0.0 } }
        $v = 0.3 * (& $get 'D') + 0.4 * (& $get 'G') + 0.3 * (& $get 'J')
        if ($v -gt 0) { $merged[$w] = $v; $naerCount++ }
    }
    $sheet = $null; $words = $null
    Write-Output "  國教院通用詞頻表 $naerCount 詞"

    # ── 二、教育部（dbf）補缺 ───────────────────────────
    # 欄位：NO(5) PHRASE(36,Big5) DPIWN(7) TDPIWN(7) PERCENT(12) ...
    # 每筆最前面有 1 byte 的刪除標記，要跳過。
    $zip2 = [System.IO.Compression.ZipFile]::OpenRead($MoeZip)
    try {
        $entry = $zip2.Entries | Where-Object { $_.Name -eq 'shrest2.dbf' }
        if (-not $entry) { throw "shrest2.zip 裡找不到 shrest2.dbf" }
        $ms = New-Object System.IO.MemoryStream
        $entry.Open().CopyTo($ms)
        $bytes = $ms.ToArray(); $ms.Close()
    } finally {
        $zip2.Dispose()
    }
    $big5 = [System.Text.Encoding]::GetEncoding(950)
    $recCount = [BitConverter]::ToInt32($bytes, 4)
    $headerLen = [BitConverter]::ToInt16($bytes, 8)
    $recLen = [BitConverter]::ToInt16($bytes, 10)
    $moeAdded = 0
    for ($i = 0; $i -lt $recCount; $i++) {
        $o = $headerLen + $i * $recLen + 1
        if ($o + $recLen -gt $bytes.Length) { break }
        $phrase = $big5.GetString($bytes, $o + 5, 36).Trim()
        $pct = [System.Text.Encoding]::ASCII.GetString($bytes, $o + 55, 12).Trim()
        $v = 0.0
        if (-not $phrase -or -not [double]::TryParse($pct, [ref]$v)) { continue }
        $perMillion = $v * 10000    # PERCENT 是百分比
        # **只補缺，不覆蓋**。國教院那份樣本更大、又分語域，同一個詞
        # 兩份都有時以它為準。實測讓教育部也去拉高國教院的值會變差
        # （600 句 86.2% → 85.5%、doc_text 47 → 46）。
        if (-not $merged.ContainsKey($phrase)) { $merged[$phrase] = $perMillion; $moeAdded++ }
    }
    $bytes = $null
    Write-Output "  教育部詞頻總表補了 $moeAdded 詞"

    # ── 三、字幕語料過濾後補「被切開的常用組合」────────────
    $known = New-Object 'System.Collections.Generic.HashSet[char]'
    foreach ($ln in [System.IO.File]::ReadAllLines($BpmfBase, $utf8)) {
        $w = ($ln -split ' ')[0]
        if ($w.Length -eq 1) { [void]$known.Add($w[0]) }
    }
    $subFreq = New-Object 'System.Collections.Generic.Dictionary[string,double]'
    $subTotal = 0.0
    foreach ($ln in [System.IO.File]::ReadAllLines($SubtitleTxt, $utf8)) {
        $pp = $ln -split ' '
        if ($pp.Count -lt 2) { continue }
        $clean = $true
        foreach ($c in $pp[0].ToCharArray()) { if (-not $known.Contains($c)) { $clean = $false; break } }
        if (-not $clean) { continue }
        $n = 0.0
        if (-not [double]::TryParse($pp[1], [ref]$n)) { continue }
        $subFreq[$pp[0]] = $n
        $subTotal += $n
    }
    $subAdded = 0; $subRaised = 0
    foreach ($kv in $subFreq.GetEnumerator()) {
        $perMillion = $kv.Value / $subTotal * 1000000
        if (-not $merged.ContainsKey($kv.Key)) { $merged[$kv.Key] = $perMillion; $subAdded++ }
        elseif ($perMillion -gt $merged[$kv.Key]) { $merged[$kv.Key] = $perMillion; $subRaised++ }
    }
    Write-Output "  字幕語料過濾後保留 $($subFreq.Count) 詞（補 $subAdded 個、拉高 $subRaised 個）"

    # ── 四、單字污染清洗 ─────────────────────────────────
    #
    # 三份合併之後仍有一批**單字**帶著天文數字的詞頻，但它們根本不是
    # 中文字：火星文（`琌` 419 萬、`硂` 242 萬、`璶`、`穦`、`碞`）、
    # 注音符號（`ㄓ` 155 萬、`ㄆ`、`ㄇ`、`ㄠ`）、簡體（`摆`、`麽`）。
    #
    # 量級有多離譜：「琌」419 萬，而真正的「不」只有 850 萬——同一個
    # 數量級。注音引擎排同音字時它就贏了。
    #
    # 上面第三步的 BPMFBase 白名單擋不住這批：那份收了 20,965 字，
    # 連 `琌`、`硂` 都在裡面（它是「注音打得出來的字」不是「常用字」）。
    # 實測 1,959 個可疑單字裡 1,943 個都通過那道門。
    #
    # 有效的判準是**教育部字頻總表**（5,701 字，以《國字標準字體表》
    # 為據）：這批污染字一個都不在裡面。
    #
    # **降到地板值 1 而不是刪掉**——依核心原則「候選只排序、不排除」，
    # 使用者真要打「琌」時它仍在候選裡，只是排最後。實測兩種做法效果
    # 相同（doc_text 78→79、前 5 93→94，其餘 8 支 0 差異）。
    $charSet = New-Object 'System.Collections.Generic.HashSet[string]'
    foreach ($ln in [System.IO.File]::ReadAllLines($CharFreq, $utf8)) {
        $w = ($ln -split ' ')[0]
        if ($w) { [void]$charSet.Add($w) }
    }
    $cleaned = 0
    foreach ($k in @($merged.Keys)) {
        if ($k.Length -eq 1 -and -not $charSet.Contains($k)) {
            $merged[$k] = 0.001   # ×1000 取整後是 1，也就是地板
            $cleaned++
        }
    }
    Write-Output "  單字污染清洗：$cleaned 個字降到地板（教育部字頻表查無）"

    if ($merged.Count -lt 150000) {
        throw "詞頻表只合併出 $($merged.Count) 筆，格式可能變了"
    }
    # ×1000 取整：引擎讀的是整數，而每百萬詞頻的小數部分有意義
    $sb = New-Object System.Text.StringBuilder
    foreach ($kv in ($merged.GetEnumerator() | Sort-Object -Property Value -Descending)) {
        $n = [long][math]::Round($kv.Value * 1000)
        if ($n -lt 1) { $n = 1 }
        [void]$sb.AppendLine("$($kv.Key) $n")
    }
    [System.IO.File]::WriteAllText($OutPath, $sb.ToString(), (New-Object System.Text.UTF8Encoding($false)))
    Write-Output "  合併詞頻表 $($merged.Count) 筆 -> $OutPath"
}


# 注音：多字詞映射表 + 單字對照表（後者用來補足前者完全不收單字
# 條目的缺口，2026-08-23 實測發現「大」「的」「一」這類最常用單字
# 在 BPMFMappings.txt 裡查無對應詞，見開發文件.md §2.1.1）
New-Item -ItemType Directory -Force -Path "$dataDir\bopomofo" | Out-Null
Get-File `
    "https://raw.githubusercontent.com/openvanilla/McBopomofo/master/Source/Data/BPMFMappings.txt" `
    "$dataDir\bopomofo\BPMFMappings.txt"
Get-File `
    "https://raw.githubusercontent.com/openvanilla/McBopomofo/master/Source/Data/BPMFBase.txt" `
    "$dataDir\bopomofo\BPMFBase.txt"

# 注音的詞頻來源。McBopomofo 的詞庫只有「注音 → 字詞」的對應關係，
# 沒有詞頻欄位，同音字只能照檔案出現順序排——實測「市」會排在「是」
# 前面，用開發文件的真實中文測 100 組，第一候選正確率只有 51%。
#
# **這三份會被合併成 `word_freq.txt`**（見 `Convert-WordFreq`），
# 引擎讀的是合併後那份。三份各有缺口，單獨用哪一份都不夠。
Get-File `
    "https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/zh_tw/zh_tw_50k.txt" `
    "$dataDir\bopomofo\zh_tw_50k.txt"
# 國教院《通用詞頻表》16.4 萬詞，分書面語/口語/新聞三種語域統計。
# 授權：國教院開放資料政策（可改作、可再授權，標示出處即可）。
Get-File `
    "https://coct.naer.edu.tw/file/files/%E9%80%9A%E7%94%A8%E8%A9%9E%E9%A0%BB%E8%A1%A8%20-%20%E5%AE%9A%E7%A8%BF1141208.xlsx" `
    "$dataDir\bopomofo\naer_wordfreq.xlsx"
# 教育部《詞頻總表》4.6 萬詞，跟上面的字頻總表同源。
# 授權：CC BY-ND 3.0 TW（「禁止改作」不限制格式轉換，見開發文件 §4.32）。
$moeWordZip = Join-Path $env:TEMP "shrest2.zip"
Get-File `
    "https://language.moe.gov.tw/001/Upload/files/SITE_CONTENT/M0001/PRIMARY/download/shrest2.zip" `
    $moeWordZip

# 日文：mozc 的 10 個辭典分片（讀音 / 左id / 右id / 詞頻 / 表記）
New-Item -ItemType Directory -Force -Path "$dataDir\japanese" | Out-Null
0..9 | ForEach-Object {
    $n = "{0:D2}" -f $_
    Get-File `
        "https://raw.githubusercontent.com/google/mozc/master/src/data/dictionary_oss/dictionary$n.txt" `
        "$dataDir\japanese\dictionary$n.txt"
}

# 日文：動詞/形容詞的活用規則。詞庫只收原形（買う），變化形（買って、買った）
# 靠這兩個檔展開出來，否則日文動詞句一律打不出漢字。
#   id.def     ：詞性 id -> 活用類別（買う 的 id=813 -> 五段・ワ行促音便）
#   cforms.def ：活用類別 -> 各活用形的語尾（五段・ワ行促音便 的連用タ接続 = っ）
Get-File `
    "https://raw.githubusercontent.com/google/mozc/master/src/data/dictionary_oss/id.def" `
    "$dataDir\japanese\id.def"
Get-File `
    "https://raw.githubusercontent.com/google/mozc/master/src/data/rules/cforms.def" `
    "$dataDir\japanese\cforms.def"

# 日文：接續成本矩陣（connection matrix）。
#
# mozc 的斷句成本 = 每個詞自己的 cost + 每個接縫的接續成本；引擎目前只用
# 前者（unigram），所以「じゅう」永遠是「中」(717) 贏「十」，「十四日」輸給
# 「中四日」（見開發文件 §4.36、§4.38）。這份表就是後者：
#   表[前一個詞的右id][後一個詞的左id] = 接續成本
# 格式是單欄、一行一個數字，第一行是 id 數（2672），之後依 rid×2672+lid
# 排列，共 2672×2672 ≈ 714 萬行、約 30MB。
# 2026-08-25 加入，先給沙盒（方向 c）用，產品端尚未接。
Get-File `
    "https://raw.githubusercontent.com/google/mozc/master/src/data/dictionary_oss/connection_single_column.txt" `
    "$dataDir\japanese\connection_single_column.txt"

# 英文：詞頻清單（word<TAB>frequency）
New-Item -ItemType Directory -Force -Path "$dataDir\english" | Out-Null
Get-File `
    "https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/en/en_50k.txt" `
    "$dataDir\english\en_50k.txt"

# 中文：教育部字頻總表。**這份要就地轉檔**，不是單純下載。
#
# 為什麼需要它：注音的單字候選原本依「詞庫檔案行序」排，那不是常用度
# ——`ㄇㄧㄥˊ` 的第一名是「螟」而不是「明」，`ㄊㄧㄢ` 排在「天」後面的是
# 「倎屇酟婖」這種沒人用的字。
#
# 為什麼不用既有的 zh_tw_50k：那是**詞**頻不是**字**頻，而且來源是字幕
# 語料，混了簡體（摆、参、担）。教育部這份以《國字標準字體表》為據，
# 天然只收繁體，罕見字也不在表中。
#
# 為什麼要轉檔：原始檔是 Big5 編碼的 ASCII 表格（用 │ 畫框線），
# 程式讀不了，要抽成「字 頻次」的純文字。轉換在使用者的機器上做，
# 我們只提供腳本、不再散布那份資料。
New-Item -ItemType Directory -Force -Path "$dataDir\bopomofo" | Out-Null
$moeZip = Join-Path $env:TEMP "BIAU1.zip"
Get-File `
    "https://language.moe.gov.tw/001/Upload/files/SITE_CONTENT/M0001/BIAU1.zip" `
    $moeZip
Convert-MoeCharFreq -ZipPath $moeZip -OutPath "$dataDir\bopomofo\char_freq.txt"

# 中文：把三份詞頻資料合併成引擎實際讀的 word_freq.txt。
# 為什麼要三份、怎麼合，見 Convert-WordFreq 的說明。
Convert-WordFreq `
    -NaerXlsx    "$dataDir\bopomofo\naer_wordfreq.xlsx" `
    -MoeZip      $moeWordZip `
    -BpmfBase    "$dataDir\bopomofo\BPMFBase.txt" `
    -SubtitleTxt "$dataDir\bopomofo\zh_tw_50k.txt" `
    -CharFreq    "$dataDir\bopomofo\char_freq.txt" `
    -OutPath     "$dataDir\bopomofo\word_freq.txt"

# 讀音別字頻：字頻表沒有讀音維度，會讓「吃」霸佔 ㄐㄧˊ 的第一名。
# 這一步把 word_freq 依 BPMFMappings 的逐字對齊分攤成「字念這個音的
# 佔比」。跟上面兩份一樣是衍生資料，在各自的機器上產生、不進版控。
Write-Output "產生讀音別字頻表…"
Push-Location (Split-Path $dataDir -Parent)
cargo run --release -q -p ime-core --bin gen_reading_freq
Pop-Location

# 日文接續矩陣：整句轉換（Viterbi）要用整份 2672x2672 矩陣。
# 原始檔是 36MB 文字、714 萬行，解析要 90ms 而且配置一大堆暫時字串；
# 轉成 13.6MB 二進位之後載入只要 7ms、零解析。同樣是衍生資料。
Write-Output "轉換日文接續矩陣…"
Push-Location (Split-Path $dataDir -Parent)
cargo run --release -q -p ime-core --bin gen_connection
Pop-Location

# 日文詞庫的二進位版面：文字版每次啟動要 700ms 逐行切欄位、為 187.5 萬
# 個字串各配一次記憶體，常駐 287MB。編成版面之後載入 51ms、常駐剩零頭，
# 而且按鍵延遲最慢一鍵 14.7ms → 10.6ms（連續版面的快取局部性）。
# 不跑這一步也能用——引擎找不到 .bin 會現場從文字建同一份版面，
# 只是每次啟動都要等。同樣是衍生資料，不進版控。
# 注音詞庫的二進位版面：文字版常駐 23MB、建表峰值再多 21MB（字頻、詞頻、
# 偏好表、讀音別字頻那四份只有建表要用），而資料本身只有 2.3MB。
# 編成 3.7MB 的版面之後載入 1ms、執行期根本不碰那四份表。
Write-Output "編譯注音二進位詞庫…"
Push-Location (Split-Path $dataDir -Parent)
cargo run --release -q -p ime-core --bin gen_dict_zh
Pop-Location

Write-Output "編譯日文二進位詞庫…"
Push-Location (Split-Path $dataDir -Parent)
cargo run --release -q -p ime-core --bin gen_dict_ja
Pop-Location

Write-Output "完成。"
