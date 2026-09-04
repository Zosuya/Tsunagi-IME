# 建置安裝程式。
#
# # 用法
#
#     .\installer\build-installer.ps1
#     .\installer\build-installer.ps1 -Version 0.2.0
#     .\installer\build-installer.ps1 -SkipBuild      # 跳過 cargo，只重打包
#
# # 為什麼要能無人值守跑完
#
# 開源專案的免費 code signing（SignPath）是**走 CI 管線簽**的——建置完把
# 產物送過去簽好再回來，不是給你憑證讓你本機簽。所以這支不能有任何互動：
# 不問問題、不跳對話框、失敗就回非零結束碼。見開發文件 §2.30。

param(
    [string]$Version,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

function Fail($msg) {
    Write-Host "✗ $msg" -ForegroundColor Red
    exit 1
}

# --- 版本號：沒指定就從 Cargo.toml 讀 ---
if (-not $Version) {
    $cargo = Join-Path $root 'platform\windows\Cargo.toml'
    $m = Select-String -Path $cargo -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $m) { Fail "從 $cargo 讀不到版本號" }
    $Version = $m.Matches[0].Groups[1].Value
}
Write-Host "版本 $Version" -ForegroundColor Cyan

# --- 1. 建置 ---
if (-not $SkipBuild) {
    $cargo = if (Get-Command cargo -ErrorAction SilentlyContinue) { 'cargo' }
             else { Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe' }
    if (-not (Test-Path $cargo) -and $cargo -ne 'cargo') { Fail "找不到 cargo" }

    Write-Host "`n[1/3] cargo build --release" -ForegroundColor Cyan
    & $cargo build --release --manifest-path (Join-Path $root 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { Fail "cargo build 失敗" }
} else {
    Write-Host "`n[1/3] 跳過建置" -ForegroundColor DarkGray
}

# --- 2. 檢查要打包的檔案 ---
Write-Host "`n[2/3] 檢查打包清單" -ForegroundColor Cyan
$need = @(
    'target\release\ime_tip_windows.dll',
    'target\release\ime_settings.exe',
    'target\release\register_tool.exe',
    'data\bopomofo\dict_zh.bin',
    'data\japanese\dict_ja.bin',
    'data\japanese\connection.bin',
    'data\english\en_50k.txt',
    'LICENSE',
    'CREDITS.md'
)
$missing = @()
$total = 0
foreach ($f in $need) {
    $p = Join-Path $root $f
    if (Test-Path $p) {
        $len = (Get-Item $p).Length
        $total += $len
        Write-Host ("  {0,12:N0}  {1}" -f $len, $f)
    } else {
        $missing += $f
        Write-Host ("  {0,12}  {1}" -f '缺', $f) -ForegroundColor Red
    }
}
if ($missing) {
    Write-Host ""
    if ($missing -match '^data\\') {
        Write-Host "詞庫還沒產生。先跑：" -ForegroundColor Yellow
        Write-Host "    .\data\download.ps1" -ForegroundColor Yellow
        Write-Host "（會下載原始檔並編譯成二進位詞庫，只要做一次）" -ForegroundColor DarkGray
    }
    Fail "缺 $($missing.Count) 個檔案"
}
Write-Host ("  合計 {0:N1} MB" -f ($total / 1MB)) -ForegroundColor DarkGray

# --- 3. 打包 ---
Write-Host "`n[3/3] 編譯安裝程式" -ForegroundColor Cyan
$iscc = @(
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $iscc) {
    Write-Host "找不到 Inno Setup。安裝方式：" -ForegroundColor Yellow
    Write-Host "    winget install JRSoftware.InnoSetup" -ForegroundColor Yellow
    Fail "缺 Inno Setup"
}

& $iscc "/DAppVersion=$Version" (Join-Path $PSScriptRoot 'tsunagi.iss')
if ($LASTEXITCODE -ne 0) { Fail "ISCC 編譯失敗" }

$out = Join-Path $root "target\installer\tsunagi-ime-$Version-setup.exe"
if (-not (Test-Path $out)) { Fail "編譯說成功，但找不到 $out" }

Write-Host ""
Write-Host ("✓ {0}  ({1:N1} MB)" -f $out, ((Get-Item $out).Length / 1MB)) -ForegroundColor Green
