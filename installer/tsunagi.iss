; 通譯輸入法的安裝程式。
;
; # 怎麼建
;
;     .\installer\build-installer.ps1
;
; 那支腳本會先 cargo build、確認詞庫在位，再叫 ISCC 編這個檔。
; **不要直接開 Inno Setup 的 IDE 按編譯**——版本號是由建置腳本傳進來的。
;
; # 這個檔在對付什麼
;
; 輸入法的安裝比一般軟體麻煩，因為 DLL 被載在**每一個**宿主行程裡
; （檔案總管永遠挖著它）。升級時檔案覆寫不掉，反安裝時刪不掉。
; 完整的實測紀錄見開發文件 §2.34，這裡只寫結論。

#define AppName "通譯輸入法"
#define Publisher "Zosuya"
#define AppUrl "https://github.com/Zosuya/Tsunagi-IME"

; 版本由建置腳本用 /DAppVersion=x.y.z 傳進來，沒傳就用這個
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

; **VersionInfoVersion 只吃純數字**（x.y.z.w）——`0.1.0-beta` 這種
; 預發布後綴會被 ISCC 直接擋下來（實測：`Value of [Setup] section
; directive "VersionInfoVersion" is invalid`）。
;
; 所以兩者分開：顯示與檔名保留後綴（使用者看得到這是 beta），
; 寫進執行檔的版本資訊用去掉後綴的那個。
#define Dash Pos("-", AppVersion)
#if Dash > 0
  #define NumericVersion Copy(AppVersion, 1, Dash - 1)
#else
  #define NumericVersion AppVersion
#endif

; 專案根目錄（這個 .iss 在 installer/ 底下）
#define Root ".."

[Setup]
; **AppId 一旦發布就不能改**——Windows 靠它認出「這是同一個程式的新版本」，
; 改了會變成兩份並存，舊的還留在解除安裝清單裡。
AppId={{01BC8BB0-ECD1-4F99-AE33-3F0F5B6AEE32}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#Publisher}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
VersionInfoVersion={#NumericVersion}

; 資料夾名稱用可讀的形式（使用者會在檔案總管裡看到它）。
;
; **AppendDefaultDirName=no**：不加這條的話，使用者在目錄頁按「瀏覽」
; 選中已存在的安裝資料夾時，Inno 會再接一層 AppName，變成
; 「Tsunagi IME\Tsunagi IME」的巢狀目錄。實測踩到過。
DefaultDirName={commonpf}\Tsunagi IME
AppendDefaultDirName=no
LicenseFile={#Root}\LICENSE
OutputDir={#Root}\target\installer
OutputBaseFilename=tsunagi-ime-{#AppVersion}-setup
SetupIconFile={#Root}\platform\windows\res\ime.ico
UninstallDisplayIcon={app}\ime_settings.exe
DisableProgramGroupPage=yes
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes

; 三語都支援，所以讓使用者自己選——不要只依系統語言猜。
; 這個輸入法的使用者本來就常在中日英之間切換。
ShowLanguageDialog=yes

; **一定要提權**：TSF 註冊寫 HKLM、安裝到 Program Files、排程開機刪除，
; 三件都需要。見開發文件 §3.2——非提權時 msctf 回的是毫無資訊量的 E_FAIL。
PrivilegesRequired=admin

; DLL 是 x64
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

; **關掉 Restart Manager**——實測出來的必要設定（開發文件 §2.34.3）。
;
; Inno 預設偵測到檔案被占用就主動關閉那些程式。對一般軟體合理，對輸入法
; 是災難：占用我們 DLL 的是檔案總管，關掉等於桌面與工作列消失，而且它會
; 自動重啟又立刻載回 DLL。第一次 spike 就死在這裡——RM 花 30 秒試圖關閉，
; 失敗後直接中止安裝。
;
; 檔案被鎖住的問題改由下面 [Code] 的「改名讓路」處理。
CloseApplications=no
RestartApplications=no

[Languages]
; **繁中的 .isl 放在專案裡，不是 Inno 的安裝目錄**——CI 上那台機器沒有
; 手動放進去的檔案。日文與英文用 Inno 內建的（compiler: 前綴）。
;
; 繁中來源：jrsoftware/issrc 的 Files/Languages/ChineseTraditional.isl
; （維護者 GoneTone，適用 6.5.0+）
Name: "zh"; MessagesFile: "languages\ChineseTraditional.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
zh.EnableIme=安裝後直接加入輸入法清單（不必自己去設定裡新增）
ja.EnableIme=インストール後、入力方式の一覧に追加する
en.EnableIme=Add to the input method list after installation

zh.MsgRegistering=正在向系統註冊輸入法…
ja.MsgRegistering=入力方式をシステムに登録しています…
en.MsgRegistering=Registering the input method...

zh.MsgEnabling=正在加入輸入法清單…
ja.MsgEnabling=入力方式の一覧に追加しています…
en.MsgEnabling=Adding to the input method list...

zh.OpenSettings=開啟設定
ja.OpenSettings=設定を開く
en.OpenSettings=Open settings

zh.ErrLocked=無法更新輸入法：舊的程式檔正在使用中，而且無法讓路。請重新開機後再安裝一次。
ja.ErrLocked=入力方式を更新できません：古いプログラムファイルが使用中で、退避もできませんでした。再起動してからもう一度インストールしてください。
en.ErrLocked=Cannot update: the old program file is in use and could not be moved aside. Please restart your computer and install again.

[Files]
Source: "{#Root}\target\release\ime_tip_windows.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\target\release\ime_settings.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\target\release\register_tool.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\CREDITS.md"; DestDir: "{app}"; Flags: ignoreversion

; 詞庫。**只裝編譯後的二進位檔**——原始下載檔（147MB）不打包，它們只是
; 產生這幾個檔的原料。授權盤點見開發文件 §2.31。
;
; 目錄結構要跟開發環境一致：程式用「執行檔旁邊的 data/」找這些檔案。
Source: "{#Root}\data\bopomofo\dict_zh.bin"; DestDir: "{app}\data\bopomofo"; Flags: ignoreversion
Source: "{#Root}\data\japanese\dict_ja.bin"; DestDir: "{app}\data\japanese"; Flags: ignoreversion
Source: "{#Root}\data\japanese\connection.bin"; DestDir: "{app}\data\japanese"; Flags: ignoreversion
Source: "{#Root}\data\english\en_50k.txt"; DestDir: "{app}\data\english"; Flags: ignoreversion

[Icons]
; **只放一個捷徑，不建資料夾。**
;
; 輸入法自己的工作列選單已經有「設定…」，這裡是備援——剛裝完還沒切過去
; 的時候，那個選單根本不存在，開始功能表是唯一的入口。
;
; 不放「移除」捷徑：Windows 10 之後的慣例是從「設定 → 應用程式」移除，
; Inno 本來就會登記在那裡。
Name: "{commonprograms}\{#AppName}"; Filename: "{app}\ime_settings.exe"

[Tasks]
Name: "enableime"; Description: "{cm:EnableIme}"

[Run]
; 註冊（機器層級，寫 HKLM）。安裝程式本身已提權，直接跑。
Filename: "{app}\register_tool.exe"; Parameters: "register ""{app}\ime_tip_windows.dll"""; StatusMsg: "{cm:MsgRegistering}"; Flags: runhidden waituntilterminated

; 加進使用者的輸入法清單（使用者層級）。
;
; **runasoriginaluser 不能少**：安裝程式是以系統管理員身分跑的，直接呼叫
; 會把輸入法加到 Administrator 的清單，而不是實際使用者的。
Filename: "{app}\register_tool.exe"; Parameters: "enable"; Tasks: enableime; StatusMsg: "{cm:MsgEnabling}"; Flags: runhidden waituntilterminated runasoriginaluser

Filename: "{app}\ime_settings.exe"; Description: "{cm:OpenSettings}"; Flags: postinstall nowait skipifsilent

[UninstallRun]
; **只做 unregister，不做 disable。**
;
; disable 動的是「目前使用者」的輸入法清單，安裝時要用 runasoriginaluser
; 降回原使用者——但那個旗標 [UninstallRun] 不支援（Inno 只在 [Run] 提供）。
; 不降權就會去動 Administrator 的清單，對實際使用者沒有任何效果。
;
; 不做也沒關係：unregister 會把 TSF profile 整個移掉，使用者清單裡的項目
; 失去對應的 TIP 之後自然消失。實測移除流程時確認過（開發文件 §2.34）。
Filename: "{app}\register_tool.exe"; Parameters: "unregister ""{app}\ime_tip_windows.dll"""; Flags: runhidden waituntilterminated; RunOnceId: "UnregisterIme"

[Code]
const
  MOVEFILE_DELAY_UNTIL_REBOOT = $4;

// 第二個參數宣告成 Cardinal 才傳得了 NULL——傳 NULL 才是「刪除」的意思，
// 傳空字串不等於 NULL。
function MoveFileExDelete(lpExistingFileName: String; lpNewFileName: Cardinal;
  dwFlags: Cardinal): Boolean;
  external 'MoveFileExW@kernel32.dll stdcall';

// 能刪就刪，刪不掉就登記下次開機刪。
//
// DLL 被載入時是 image section，Windows 不讓刪（實測見 §2.34.1）。使用者
// 不見得會再跑一次安裝程式，所以登記開機刪除——下次自然重開機就清乾淨，
// 不必為了收垃圾要求使用者現在重開。
procedure DeleteOrScheduleDelete(Path: String);
begin
  if DeleteFile(Path) then
    Log('已刪: ' + Path)
  else if MoveFileExDelete(Path, 0, MOVEFILE_DELAY_UNTIL_REBOOT) then
    Log('刪不掉，已登記下次開機刪除: ' + Path)
  else
    Log('刪不掉，登記也失敗: ' + Path);
end;

procedure SweepLeftovers(Dir: String);
var
  R: TFindRec;
begin
  if FindFirst(Dir + '\ime_tip_windows.dll.old-*', R) then begin
    try
      repeat
        DeleteOrScheduleDelete(Dir + '\' + R.Name);
      until not FindNext(R);
    finally
      FindClose(R);
    end;
  end;
end;

// 在檔案複製之前跑——唯一能趕在覆寫失敗之前動手的時機。
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Dir, Old, Stamp: String;
begin
  Result := '';
  Dir := ExpandConstant('{app}');
  SweepLeftovers(Dir);

  Old := Dir + '\ime_tip_windows.dll';
  if not FileExists(Old) then
    Exit;

  // **先試刪除**。沒人佔用時直接刪掉，不留任何殘骸。
  if DeleteFile(Old) then begin
    Log('舊 DLL 沒被佔用，直接刪掉');
    Exit;
  end;

  // 刪不掉才讓路。**時間戳命名**是刻意的：固定名稱會在上一個殘骸還被
  // 佔用時卡住，這一輪就改不了名。
  Stamp := Old + '.old-' + GetDateTimeString('yyyymmddhhnnss', #0, #0);
  if RenameFile(Old, Stamp) then
    Log('舊 DLL 被佔用，已改名讓路: ' + Stamp)
  else
    Result := ExpandConstant('{cm:ErrLocked}') + #13#10 + Old;
end;

// 反安裝時 DLL 同樣可能被載著刪不掉，一樣的處理。
procedure CurUninstallStepChanged(CurStep: TUninstallStep);
var
  Dir: String;
begin
  if CurStep = usPostUninstall then begin
    Dir := ExpandConstant('{app}');
    SweepLeftovers(Dir);
    if FileExists(Dir + '\ime_tip_windows.dll') then
      DeleteOrScheduleDelete(Dir + '\ime_tip_windows.dll');

    // **連資料夾本身也要收掉**，不然移除完 Program Files 底下會留一個
    // 空殼。RemoveDir 只在目錄真的空了才會成功；此時通常還有登記了
    // 開機刪除、但當下仍在的 DLL 殘骸，所以多半會走到第二條。
    //
    // 順序是對的：檔案先登記、目錄後登記，而 PendingFileRenameOperations
    // 是按登記順序執行的——開機時先刪檔案，目錄那時才空得掉。
    if not RemoveDir(Dir) then
      if MoveFileExDelete(Dir, 0, MOVEFILE_DELAY_UNTIL_REBOOT) then
        Log('資料夾還不能刪，已登記下次開機刪除: ' + Dir)
      else
        Log('資料夾刪不掉，登記也失敗: ' + Dir);
  end;
end;
