# 建置輸入法 DLL，**不必先關掉正在用它的程式**。
#
# # 解決什麼問題
#
# Windows 不允許覆寫正在被載入的 DLL。只要記事本、瀏覽器、檔案總管
# 任何一個還挖著 `ime_tip_windows.dll`，`cargo build` 就會失敗：
#
#     error: failed to remove file `...\ime_tip_windows.dll`
#
# 以前的做法是切走輸入法、關掉那些程式再編——但檔案總管（工作列的
# 輸入指示器）幾乎一定挖著它，等於每次改程式都要重啟檔案總管。
#
# # 原理
#
# **Windows 不允許刪除被載入的 DLL，但允許改名。**
# 把舊檔改名之後，原路徑就空出來了，cargo 可以在那裡建立新檔。
# 舊那份繼續留在記憶體裡給既有行程用（它們跑的還是舊程式碼），
# 之後**新啟動**的程式才會載入新版。
#
# # 為什麼用時間戳而不是固定的 .old
#
# 固定名稱會卡住：上一輪的 `.old` 如果還被行程佔用，這一輪改名就
# 失敗（實測確認會失敗），等於又回到原點。用時間戳保證名字唯一，
# 改名一定成功；殘留的檔案在後續執行時會被自動清掉——那時佔用它的
# 行程通常已經關了。
#
# # 用法
#
#     .\build-ime.ps1              # 建置 DLL
#     .\build-ime.ps1 -All         # 連設定頁一起建
#     .\build-ime.ps1 -CleanOnly   # 只清殘留檔，不建置
#     .\build-ime.ps1 -PanicTest   # 帶「故意 panic」的測試開關（測完要重建）
#
# # 注意
#
# 改的是程式碼的話**不需要重新註冊**——註冊表存的是「CLSID → 路徑」
# 這種指標，路徑沒變就不必動。只有改了顯示名稱、註冊類別、GUID 或
# 搬動了 DLL 位置才要重註冊（那要提權終端機）。
#
# 既有行程跑的仍是舊程式碼。要驗證新版，**開一個全新的**記事本或
# 瀏覽器分頁，那才會從磁碟載入新檔。

param(
    [switch]$All,
    [switch]$CleanOnly,
    [switch]$PanicTest
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$outDir = Join-Path $root 'target\release'
$depsDir = Join-Path $outDir 'deps'

# **兩個路徑都要讓開**。
#
# `target\release\ime_tip_windows.dll` 只是**硬連結**，連結器實際寫的是
# `target\release\deps\ime_tip_windows.dll`。只搬走前者的話，連結器還是
# 開不了後者，會失敗在：
#
#     LINK : fatal error LNK1104: 無法開啟檔案 '...\deps\ime_tip_windows.dll'
$targets = @(
    (Join-Path $outDir 'ime_tip_windows.dll'),
    (Join-Path $depsDir 'ime_tip_windows.dll')
)

# ── 先清掉能清的殘留檔 ──
#
# 還被佔用的刪不掉，跳過就好——下次執行時通常就清得掉了。
$stale = @()
foreach ($d in @($outDir, $depsDir)) {
    $stale += Get-ChildItem -Path $d -Filter 'ime_tip_windows.dll.old.*' -ErrorAction SilentlyContinue
}
$removed = 0
$kept = 0
foreach ($f in $stale) {
    try {
        Remove-Item $f.FullName -Force -ErrorAction Stop
        $removed++
    } catch {
        $kept++
    }
}
if ($removed -gt 0 -or $kept -gt 0) {
    Write-Host "清理殘留：刪除 $removed 個" -NoNewline
    if ($kept -gt 0) { Write-Host "，$kept 個仍被佔用（下次再清）" } else { Write-Host "" }
}

if ($CleanOnly) { return }

# ── 把現有的 DLL 讓開 ──
$parkedAny = $false
foreach ($dll in $targets) {
    if (-not (Test-Path $dll)) { continue }
    # 先試著直接刪——沒人佔用時這樣最乾淨，不會留下殘留檔
    try {
        Remove-Item $dll -Force -ErrorAction Stop
        continue
    } catch {
        # 有人佔用。改名讓路——時間戳保證名字唯一，改名一定成功
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss-fff'
        try {
            Move-Item $dll "$dll.old.$stamp" -Force -ErrorAction Stop
            $parkedAny = $true
        } catch {
            Write-Host "無法讓路：$dll" -ForegroundColor Red
            Write-Host $_ -ForegroundColor Red
            exit 1
        }
    }
}
if ($parkedAny) {
    $holders = (Get-Process | Where-Object {
        $_.Modules.ModuleName -contains 'ime_tip_windows.dll'
    } | Select-Object -ExpandProperty ProcessName -Unique) -join ', '
    Write-Host "DLL 被佔用（$holders），已改名讓路" -ForegroundColor DarkGray
}

# ── 建置 ──
$cargo = if (Get-Command cargo -ErrorAction SilentlyContinue) {
    'cargo'
} else {
    Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
}

# -PanicTest：把「故意 panic」的觸發點編進去，用來驗證崩潰隔離真的
# 會降級（見 guard::maybe_panic 與相容性測試清單.md 的 G1～G3）。
# **測完要不帶這個開關重建一次**，否則裝著測試開關的 DLL 會一直在。
$feat = if ($PanicTest) { @('--features', 'panic-test') } else { @() }
& $cargo build --release -p ime-tip-windows @feat
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if ($All) {
    & $cargo build --release -p ime-settings
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host ""
if ($PanicTest) {
    Write-Host "!! 這份 DLL 含 panic-test 開關，測完請不帶 -PanicTest 重建一次" -ForegroundColor Yellow
}
Write-Host "建置完成。既有行程跑的仍是舊版——要驗證新版請開新的記事本／瀏覽器分頁。" -ForegroundColor Green
