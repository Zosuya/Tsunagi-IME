# 量輸入法在真實宿主裡的記憶體成本（相容性測試清單.md 的 M1~M3）。
#
# # 為什麼要量差值，不能量總量
#
# 宿主行程自己就很肥——explorer 是檔案總管＋桌面＋工作列三合一，
# 本身三百多 MB；Brave 一百多。輸入法那幾 MB 埋在裡面看不出來。
#
# §2.29.5 記的「每多一個宿主只多 6MB」是 bench_dict --hold 量的
# **獨立行程**（只載詞庫、什麼都不幹，全身家當就 6MB）。真實宿主
# 沒有那種乾淨環境，只能量**載入輸入法前後的差**。
#
# # 三個指標為什麼都要看
#
#   WorkingSet    含跟別的行程共用的頁（mmap 的詞庫檔就在這裡共用）
#   私有工作集    扣掉共用之後真正屬於這個行程的。**判準看這個**
#   PrivateBytes  已認可的私有位址空間，含被換出去的部分
#
# 量二進位詞庫那輪拍到過 WorkingSet 2.6MB 而私有 64MB 的矛盾快照
# （工作集不可能小於它的私有部分），只看一個會被瞬間值騙。
#
# # 用法
#
#     1. 把要測的 App 都開好，但**先不要切到輸入法**
#     2. .\tools\mem.ps1 -Mark      記下基準
#     3. 切到輸入法，在**每個** App 裡實際打幾個字
#     4. .\tools\mem.ps1 -Diff      看增量
#
#     .\tools\mem.ps1            只看現況（不比較）
#     .\tools\mem.ps1 -Watch -Diff  每 5 秒重印增量，M3 用
#
# **中途不可以關掉重開 App**——比對靠 PID，重開就對不上了，那時重跑
# 步驟 2。
param(
    [string]$Module = 'ime_tip_windows.dll',
    [switch]$Mark,
    [switch]$Diff,
    [switch]$Watch
)

$baseFile = Join-Path $env:TEMP 'tsunagi_mem_baseline.json'

# 私有工作集不在 Get-Process 裡，要向效能計數器要。用 IDProcess 對應
# PID，**不要用行程名稱**——chrome 那種同名一堆會對錯行程
function Get-Snapshot {
    $perf = @{}
    Get-CimInstance Win32_PerfRawData_PerfProc_Process -ErrorAction SilentlyContinue |
        ForEach-Object { $perf[[int]$_.IDProcess] = $_ }

    $rows = @{}
    foreach ($p in (Get-Process -ErrorAction SilentlyContinue)) {
        $c = $perf[[int]$p.Id]
        if (-not $c) { continue }
        $rows[[string]$p.Id] = [pscustomobject]@{
            Name    = $p.ProcessName
            WS      = [math]::Round($p.WorkingSet64 / 1MB, 1)
            Private = [math]::Round($c.WorkingSetPrivate / 1MB, 1)
            PB      = [math]::Round($c.PrivateBytes / 1MB, 1)
        }
    }
    $rows
}

function Get-Hosts {
    # Modules 對某些行程會拋權限錯誤（提權的、系統的），try 吞掉——
    # 那些行程本來就不是我們的宿主
    Get-Process -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Modules.ModuleName -contains $Module } catch { $false }
    }
}

if ($Mark) {
    $snap = Get-Snapshot
    $loaded = @(Get-Hosts)
    if ($loaded) {
        Write-Host "!! 這些行程已經載入輸入法了，基準會含它的成本：" -ForegroundColor Yellow
        $loaded | ForEach-Object { Write-Host "   $($_.ProcessName) ($($_.Id))" -ForegroundColor Yellow }
        Write-Host "   要量乾淨的差值，把它們整個關掉重開再 -Mark" -ForegroundColor Yellow
    }
    $snap | ConvertTo-Json -Depth 4 | Set-Content -Path $baseFile -Encoding UTF8
    Write-Host "基準已記下（$($snap.Count) 個行程）→ $baseFile" -ForegroundColor Cyan
    Write-Host "接著切到輸入法、在每個 App 裡打幾個字，再跑 -Diff" -ForegroundColor DarkGray
    return
}

function Show-Once {
    $procs = @(Get-Hosts)
    if (-not $procs) {
        Write-Host "沒有行程載入 $Module —— 切到輸入法、在那個 App 裡打幾個字再跑" -ForegroundColor DarkGray
        Write-Host "（TSF 是用到才載，只切過去不打字 DLL 不會進到行程裡）" -ForegroundColor DarkGray
        return
    }

    $base = $null
    if ($Diff) {
        if (-not (Test-Path $baseFile)) {
            Write-Host "沒有基準檔，先跑 .\tools\mem.ps1 -Mark" -ForegroundColor Red
            return
        }
        $base = Get-Content $baseFile -Raw -Encoding UTF8 | ConvertFrom-Json
    }

    $now = Get-Snapshot
    $sum = 0.0; $n = 0; $miss = 0

    $rows = foreach ($p in $procs) {
        $k = [string]$p.Id
        $c = $now[$k]
        if (-not $c) { continue }
        $o = [ordered]@{
            '行程' = $c.Name
            'PID'  = $p.Id
        }
        if ($Diff) {
            $b = $base.$k
            if ($b) {
                $d = [math]::Round($c.Private - $b.Private, 1)
                $sum += $d; $n++
                $o['私有 前→後'] = "$($b.Private) → $($c.Private)"
                $o['增量(MB)']   = $d
                $o['WS 增量']    = [math]::Round($c.WS - $b.WS, 1)
            } else {
                $miss++
                $o['私有 前→後'] = '基準裡沒有這個 PID'
                $o['增量(MB)']   = '?'
                $o['WS 增量']    = '?'
            }
        } else {
            $o['WorkingSet(MB)']   = $c.WS
            $o['私有工作集(MB)']   = $c.Private
            $o['PrivateBytes(MB)'] = $c.PB
        }
        [pscustomobject]$o
    }

    $rows | Format-Table -AutoSize

    if ($Diff) {
        if ($n -gt 0) {
            $avg = [math]::Round($sum / $n, 1)
            Write-Host ("{0} 個宿主載入輸入法，私有工作集增量總和 {1} MB（平均每個 {2} MB）" -f `
                $n, [math]::Round($sum, 1), $avg) -ForegroundColor Cyan
            if ($avg -gt 40) {
                Write-Host "平均超過 40MB —— 詞庫可能沒有共用，確認 memmap2 feature 是開的" -ForegroundColor Yellow
            }
            Write-Host "explorer 的數字噪音大（桌面與工作列一直在動），以 Notepad 那列為準" -ForegroundColor DarkGray
        }
        if ($miss) { Write-Host "$miss 個行程不在基準裡（開得比 -Mark 晚），重跑 -Mark" -ForegroundColor Yellow }
    }
}

if ($Watch) {
    while ($true) {
        Clear-Host
        Write-Host (Get-Date -Format 'HH:mm:ss') -ForegroundColor DarkGray
        Show-Once
        Start-Sleep -Seconds 5
    }
} else {
    Show-Once
}
