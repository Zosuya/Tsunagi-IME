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
; **檔名帶 win**——macOS 那邊發的是 .pkg，兩個平台的產物並排在同一個
; Release 底下，光看副檔名分不夠清楚（見 platform/macos/build-release.sh）。
OutputBaseFilename=tsunagi-ime-{#AppVersion}-win-setup
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

zh.CompMain=輸入法主程式
ja.CompMain=入力方式本体
en.CompMain=Input method (required)

zh.CompTaigi=台語擴充包（用注音打華語詞，候選出台語漢字）
ja.CompTaigi=台湾語辞書パック
en.CompTaigi=Taiwanese Hokkien pack

[Types]
; **[Types] 與 [Components] 是配套的，不能只留一半。**
;
; 元件的 `Types:` 參數是在說「這個元件預設在哪些安裝類型裡被選中」。
; 少了 [Types]、元件也沒寫 `Types:` 的話，每個元件都不屬於任何類型，
; 於是**預設全部沒選中，按下一步一個檔案都不會裝**——症狀是安裝到
; 一半跳「register_tool.exe 找不到」（實測踩過，兩次都跟這段有關）。
;
; 只給一種類型並標 `iscustom`：這是 Inno 給「我只要勾選清單、不要安裝
; 類型」的標準寫法。iscustom 的類型不會強制任何一組預設值，使用者的
; 勾選會被保留，元件頁看起來就是一張乾淨的勾選清單。
Name: "custom"; Description: "{cm:CompMain}"; Flags: iscustom

[Components]
; **元件（Components）不是工作（Tasks）**：元件決定「裝哪些檔案」，
; 工作決定「做哪些動作」。台語包是檔案，所以走元件。
;
; **台語預設不勾**：它是給特定族群的 1.7MB，多數人用不到，預設裝等於
; 替所有人做決定。想要的人自己勾——`Types:` 不寫就是「不屬於任何安裝
; 類型」，效果就是預設不選中。
;
; 主程式相反：掛在 custom 底下（預設選中）而且 `fixed`（取消不掉）。
;
; 沒勾的人事後要補：重新執行安裝程式、把台語勾起來即可，Inno 的元件
; 選擇本來就可重入，已裝好的部分不受影響。
Name: "main";  Description: "{cm:CompMain}";  Types: custom; Flags: fixed
Name: "taigi"; Description: "{cm:CompTaigi}"

[Files]
; 主程式：`Components: main` 而 main 是 fixed，使用者取消不掉。
Source: "{#Root}\target\release\ime_tip_windows.dll"; DestDir: "{app}"; Flags: ignoreversion; Components: main
Source: "{#Root}\target\release\ime_settings.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: main
Source: "{#Root}\target\release\register_tool.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: main
Source: "{#Root}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion; Components: main
Source: "{#Root}\CREDITS.md"; DestDir: "{app}"; Flags: ignoreversion; Components: main

; 詞庫。**只裝編譯後的二進位檔**——原始下載檔（147MB）不打包，它們只是
; 產生這幾個檔的原料。授權盤點見開發文件 §2.31。
;
; 目錄結構要跟開發環境一致：程式用「執行檔旁邊的 data/」找這些檔案。
Source: "{#Root}\data\bopomofo\dict_zh.bin"; DestDir: "{app}\data\bopomofo"; Flags: ignoreversion; Components: main
Source: "{#Root}\data\japanese\dict_ja.bin"; DestDir: "{app}\data\japanese"; Flags: ignoreversion; Components: main
Source: "{#Root}\data\japanese\connection.bin"; DestDir: "{app}\data\japanese"; Flags: ignoreversion; Components: main
Source: "{#Root}\data\english\en_50k.txt"; DestDir: "{app}\data\english"; Flags: ignoreversion; Components: main
; 中文選字的字級 bigram（9.8MB）。**原樣打包、不轉檔**——它本來就是
; 唯讀的 darts-clone 版面，mmap 友善，跟 .bin 同性質。缺這份檔案不會
; 壞掉，選字只是退回不看前後文的行為（見 core/src/lm.rs 的 load）。
Source: "{#Root}\data\bopomofo\zh_bigram.gram"; DestDir: "{app}\data\bopomofo"; Flags: ignoreversion; Components: main

; 預載的擴充包。目前只有「內建符號」——那份原本寫死在 symbol.rs 裡，
; 2026-09-05 抽出來變成包，使用者才看得到有哪些符號、能複製一份來改。
;
; **裝在執行檔旁邊，不是 %APPDATA%**：使用者可以把 packs_dir 改到別的
; 地方（同步資料夾之類），放進去的話整批預載包就跟著消失。放這裡則是
; 永遠找得到、升級直接覆蓋，而想改的人在自己的包裡放同名的那一組就
; 蓋過去了（`pack::dirs()` 讓使用者目錄排在前面）。
;
; **一定要逐個列出，不可以用 `packs\*.txt`**：開發機的 packs\ 底下還有
; 遊戲名、資訊技術這類個人化的包（`.gitignore` 刻意排除它們），萬用字元
; 會把開發者自己的東西一起打進安裝包送給使用者。
Source: "{#Root}\packs\內建符號.txt"; DestDir: "{app}\packs"; Flags: ignoreversion; Components: main
Source: "{#Root}\packs\內建emoji.txt"; DestDir: "{app}\packs"; Flags: ignoreversion; Components: main

; 台語包（1.7MB）**是可選元件**——只有勾了 taigi 才裝。
;
; 為什麼另外拆成元件：它比其他預載包大一個數量級，而且是給特定族群用的。
; 沒勾的人事後想補，重新執行安裝程式勾起來即可（Inno 的元件選擇可重入）。
;
; 資料是 CC BY-SA 4.0，**檔頭的來源與授權那幾行是授權義務的一部分，
; 不是說明文字**，重產包時不可以拿掉（見 tools/取台語資料.md）。
Source: "{#Root}\packs\台語.txt"; DestDir: "{app}\packs"; Flags: ignoreversion; Components: taigi

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

; 最後一頁的「開啟設定」。**預設不勾**（`unchecked`）：裝完最想做的事
; 是去打字試試看，不是開設定頁。想看的人自己勾。
Filename: "{app}\ime_settings.exe"; Description: "{cm:OpenSettings}"; Flags: postinstall nowait skipifsilent unchecked

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
