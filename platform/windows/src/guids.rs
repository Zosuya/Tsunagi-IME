use windows::core::GUID;

/// Echo IME 文字服務的 CLSID（隨機產生，全案唯一）。
pub const CLSID_TEXT_SERVICE: GUID = GUID::from_u128(0xa00a3a9b_0c3a_4306_b4ad_5d47ae8c3705);

/// 語言設定檔（language profile）GUID。
pub const GUID_PROFILE: GUID = GUID::from_u128(0xe2a94e34_a5ca_4a62_b331_6470c20e4ab3);

/// 目標語言：繁體中文（台灣）。Phase 0 僅需一個假的語言檔即可掛進輸入法清單。
pub const TEXTSERVICE_LANGID: u16 = 0x0404;

pub const TEXTSERVICE_DESC: &str = "通 · つなぎ 輸入法";
