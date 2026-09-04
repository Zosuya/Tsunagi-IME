# 檢查哪些行程跑著最新的 DLL。
#
# # 為什麼需要這個
#
# `build-ime.ps1` 是**改名讓路**——舊檔被改成 `.old.<時間戳>`，新檔佔用
# 原路徑。既有行程還映射著舊檔的內容，但**它們回報的路徑會跟著改名走**，
# 看起來跟新檔一模一樣。
#
# 結果就是：光看路徑完全分不出新舊，連模組大小都可能巧合相同。
# 本專案曾為此重複測到舊版、把診斷帶偏好幾輪。
#
# 可靠的判準是 **PE 標頭裡的建置時間戳**——每次連結都會更新，
# 而且是從**行程的記憶體**讀出來的，不受檔案改名影響。
#
# # 用法
#
#     .\check-dll.ps1
#
# DLL 只有在該行程**實際切到這個輸入法**之後才會載入。所以要先
# 開新的記事本、切過去，再跑這個檢查。

$src = @'
using System;
using System.Runtime.InteropServices;
public class RPM {
  [DllImport("kernel32.dll")] public static extern IntPtr OpenProcess(uint a, bool i, int pid);
  [DllImport("kernel32.dll")] public static extern bool ReadProcessMemory(IntPtr h, IntPtr addr, byte[] buf, int size, out IntPtr read);
  [DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
}
'@
Add-Type -TypeDefinition $src

function Get-StampFile($path) {
  $b = [System.IO.File]::ReadAllBytes($path)
  $pe = [BitConverter]::ToInt32($b, 0x3C)
  return [BitConverter]::ToUInt32($b, $pe + 8)
}

# 從**行程的記憶體**讀 PE 標頭，不是從檔案——這才問得到「它實際跑的是哪一版」
function Get-StampMem($procId, $base) {
  $h = [RPM]::OpenProcess(0x410, $false, $procId)   # QUERY_INFORMATION | VM_READ
  if ($h -eq [IntPtr]::Zero) { return $null }
  $buf = New-Object byte[] 1024
  $r = [IntPtr]::Zero
  $ok = [RPM]::ReadProcessMemory($h, $base, $buf, $buf.Length, [ref]$r)
  [RPM]::CloseHandle($h) | Out-Null
  if (-not $ok) { return $null }
  $pe = [BitConverter]::ToInt32($buf, 0x3C)
  if ($pe -lt 0 -or ($pe + 12) -gt $buf.Length) { return $null }
  return [BitConverter]::ToUInt32($buf, $pe + 8)
}

$dll = Join-Path $PSScriptRoot 'target\release\ime_tip_windows.dll'
if (-not (Test-Path $dll)) { Write-Host "找不到 DLL，先跑 build-ime.ps1" -ForegroundColor Red; exit 1 }

$want = Get-StampFile $dll
$wt = [DateTimeOffset]::FromUnixTimeSeconds($want).LocalDateTime.ToString('MM/dd HH:mm:ss')
Write-Host "磁碟上的 DLL：$wt" -ForegroundColor Cyan
Write-Host ""

$any = $false
Get-Process | Where-Object { $_.Modules.ModuleName -contains 'ime_tip_windows.dll' } | ForEach-Object {
  $any = $true
  $p = $_
  $m = $p.Modules | Where-Object { $_.ModuleName -like 'ime_tip*' } | Select-Object -First 1
  $got = Get-StampMem $p.Id $m.BaseAddress
  if ($null -eq $got) {
    Write-Host ("{0,-12} PID {1,-7} 讀不到（權限不足）" -f $p.ProcessName, $p.Id) -ForegroundColor DarkGray
  } elseif ($got -eq $want) {
    Write-Host ("{0,-12} PID {1,-7} 最新版 ✓" -f $p.ProcessName, $p.Id) -ForegroundColor Green
  } else {
    $ot = [DateTimeOffset]::FromUnixTimeSeconds($got).LocalDateTime.ToString('MM/dd HH:mm:ss')
    Write-Host ("{0,-12} PID {1,-7} 舊版（$ot）" -f $p.ProcessName, $p.Id) -ForegroundColor Yellow
  }
}
if (-not $any) {
  Write-Host "目前沒有行程載入這個輸入法——開個記事本切過去再試" -ForegroundColor DarkGray
}
