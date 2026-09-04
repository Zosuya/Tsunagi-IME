# 盤點輸入法在這台電腦上留下了什麼。
#
# 安裝／移除前後各跑一次拿來對照。**唯讀，不動任何東西。**
#
# 三層要分清楚，移除時也是照這個順序：
#
#   使用者層  %APPDATA%\tsunagi-ime  設定、學習檔、擴充包
#             使用者的輸入法清單（工作列選單看得到的那個）
#   機器層    HKLM 的 COM 註冊（CLSID → DLL 路徑）
#             HKLM 的 TSF 註冊（Category、LanguageProfile）
#   執行中    哪些行程正載著 DLL——**這些不關掉，DLL 檔案刪不掉**
#
# 用法：
#     .\tools\check-install.ps1

$clsid = '{A00A3A9B-0C3A-4306-B4AD-5D47AE8C3705}'
$profileGuid = '{E2A94E34-A5CA-4A62-B331-6470C20E4AB3}'

function Line($ok, $text) {
    if ($ok) { Write-Host "  [有] $text" -ForegroundColor Yellow }
    else     { Write-Host "  [無] $text" -ForegroundColor DarkGray }
}

Write-Host "`n=== 使用者資料 ===" -ForegroundColor Cyan
$dir = "$env:APPDATA\tsunagi-ime"
if (Test-Path $dir) {
    Line $true $dir
    Get-ChildItem $dir -Recurse -File | ForEach-Object {
        Write-Host ("       {0,-30} {1,8:N0} bytes" -f $_.FullName.Substring($dir.Length + 1), $_.Length)
    }
} else { Line $false $dir }

# 舊名的資料夾（2026-09-01 之前）。程式會自動改名過去，理論上不該還在
$old = "$env:APPDATA\通用語言輸入法"
if (Test-Path $old) { Line $true "$old  ← 舊名資料夾，理應已被自動改名" }

Write-Host "`n=== COM 註冊 ===" -ForegroundColor Cyan
foreach ($root in 'HKLM','HKCU') {
    $k = "${root}:\SOFTWARE\Classes\CLSID\$clsid"
    if (Test-Path $k) {
        Line $true $k
        $dll = (Get-ItemProperty "$k\InprocServer32" -ErrorAction SilentlyContinue).'(default)'
        if ($dll) {
            Write-Host "       → $dll"
            if (-not (Test-Path $dll)) { Write-Host "         !! 那個檔案不存在，註冊指向空的" -ForegroundColor Red }
        }
    } else { Line $false $k }
}

Write-Host "`n=== TSF 註冊 ===" -ForegroundColor Cyan
$tip = "HKLM:\SOFTWARE\Microsoft\CTF\TIP\$clsid"
if (Test-Path $tip) {
    Line $true $tip
    $lp = "$tip\LanguageProfile\0x00000404\$profileGuid"
    if (Test-Path $lp) {
        $p = Get-ItemProperty $lp -ErrorAction SilentlyContinue
        Write-Host "       顯示名稱: $($p.Description)"
        Write-Host "       啟用狀態: $($p.Enable)   （1=啟用 0=停用，沒有這個值代表看使用者設定）"
    }
} else { Line $false $tip }

Write-Host "`n=== 還載著 DLL 的行程 ===" -ForegroundColor Cyan
$procs = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
    try { $_.Modules.ModuleName -contains 'ime_tip_windows.dll' } catch { $false }
})
if ($procs) {
    Write-Host "  $($procs.Count) 個（這些不關掉，DLL 檔案刪不掉）" -ForegroundColor Yellow
    $procs | ForEach-Object { Write-Host ("       {0,-16} PID {1}" -f $_.ProcessName, $_.Id) }
} else {
    Write-Host "  沒有 —— 乾淨" -ForegroundColor Green
}
