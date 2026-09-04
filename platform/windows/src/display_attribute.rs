//! 組字文字的視覺樣式（display attribute）。
//!
//! **底線不是宿主自動畫的，是輸入法自己要求的。** TSF 的分工是：組字範圍
//! 要長什麼樣（底線樣式、顏色、粗細）由輸入法提供，宿主只負責照著畫。
//! 少了這一整套，宿主拿到的就是一段沒有任何樣式標記的文字，於是照一般
//! 文字呈現——組字時看不到底線就是這個原因。
//!
//! 要讓底線出現，三件事缺一不可：
//! 1. 註冊 `GUID_TFCAT_DISPLAYATTRIBUTEPROVIDER` category（見 `registration.rs`）
//! 2. 實作本檔的 `ITfDisplayAttributeProvider`／`ITfDisplayAttributeInfo`
//!    ／`IEnumTfDisplayAttributeInfo`，宣告「我提供哪些樣式」
//! 3. 組字時把 `GUID_PROP_ATTRIBUTE` 屬性套到組字範圍上（見 `text_service.rs`）

use std::cell::RefCell;

use windows::core::{implement, Result, BSTR, GUID};
use windows::Win32::Foundation::{E_INVALIDARG, S_FALSE};
use windows::Win32::UI::TextServices::{
    IEnumTfDisplayAttributeInfo, IEnumTfDisplayAttributeInfo_Impl, ITfDisplayAttributeInfo,
    ITfDisplayAttributeInfo_Impl, TF_ATTR_INPUT, TF_CT_NONE, TF_DA_COLOR, TF_DISPLAYATTRIBUTE,
    TF_LS_DOT,
};

/// 我們唯一一種樣式（「正在輸入中的文字」）的 GUID。
/// 隨機產生，只要全案唯一即可。
pub const GUID_DISPLAY_ATTRIBUTE_INPUT: GUID =
    GUID::from_u128(0x7d5d4e21_9c3f_4a86_bb17_2e8f61c0a934);

/// 組字中文字的樣式：文字與背景都用預設色，底下加一條點線。
///
/// 刻意不指定文字/背景顏色（`TF_CT_NONE`）——讓宿主沿用它自己的配色，
/// 這樣在深色與淺色主題下都不會出現看不見的字。
fn input_style() -> TF_DISPLAYATTRIBUTE {
    TF_DISPLAYATTRIBUTE {
        crText: TF_DA_COLOR {
            r#type: TF_CT_NONE,
            ..Default::default()
        },
        crBk: TF_DA_COLOR {
            r#type: TF_CT_NONE,
            ..Default::default()
        },
        lsStyle: TF_LS_DOT,
        fBoldLine: false.into(),
        crLine: TF_DA_COLOR {
            r#type: TF_CT_NONE,
            ..Default::default()
        },
        bAttr: TF_ATTR_INPUT,
    }
}

/// 單一一種樣式的描述。TSF 會透過它來問「這個樣式長什麼樣」。
#[implement(ITfDisplayAttributeInfo)]
pub struct DisplayAttributeInfo;

impl ITfDisplayAttributeInfo_Impl for DisplayAttributeInfo_Impl {
    fn GetGUID(&self) -> Result<GUID> {
        Ok(GUID_DISPLAY_ATTRIBUTE_INPUT)
    }

    fn GetDescription(&self) -> Result<BSTR> {
        Ok(BSTR::from("組字中"))
    }

    fn GetAttributeInfo(&self, pda: *mut TF_DISPLAYATTRIBUTE) -> Result<()> {
        if pda.is_null() {
            return Err(E_INVALIDARG.into());
        }
        unsafe { *pda = input_style() };
        Ok(())
    }

    /// 允許使用者自訂樣式的介面，Phase 0 不支援，直接忽略。
    fn SetAttributeInfo(&self, _pda: *const TF_DISPLAYATTRIBUTE) -> Result<()> {
        Ok(())
    }

    fn Reset(&self) -> Result<()> {
        Ok(())
    }
}

/// 樣式清單的列舉器。我們只有一種樣式，所以用一個 bool 記是否已取走。
#[implement(IEnumTfDisplayAttributeInfo)]
pub struct EnumDisplayAttributeInfo {
    done: RefCell<bool>,
}

impl EnumDisplayAttributeInfo {
    pub fn new() -> Self {
        Self {
            done: RefCell::new(false),
        }
    }
}

impl IEnumTfDisplayAttributeInfo_Impl for EnumDisplayAttributeInfo_Impl {
    fn Clone(&self) -> Result<IEnumTfDisplayAttributeInfo> {
        // 複製時要連「取到哪了」一起複製，否則呼叫端會拿到重複或遺漏的項目。
        let copy = EnumDisplayAttributeInfo {
            done: RefCell::new(*self.done.borrow()),
        };
        Ok(copy.into())
    }

    fn Next(
        &self,
        ulcount: u32,
        rginfo: *mut Option<ITfDisplayAttributeInfo>,
        pcfetched: *mut u32,
    ) -> Result<()> {
        let mut fetched = 0u32;
        if ulcount > 0 && !*self.done.borrow() && !rginfo.is_null() {
            let info: ITfDisplayAttributeInfo = DisplayAttributeInfo.into();
            unsafe { *rginfo = Some(info) };
            *self.done.borrow_mut() = true;
            fetched = 1;
        }
        if !pcfetched.is_null() {
            unsafe { *pcfetched = fetched };
        }
        // 取到的數量少於要求時要回 S_FALSE，這是 COM 列舉器的慣例。
        if fetched < ulcount {
            Err(S_FALSE.into())
        } else {
            Ok(())
        }
    }

    fn Reset(&self) -> Result<()> {
        *self.done.borrow_mut() = false;
        Ok(())
    }

    fn Skip(&self, ulcount: u32) -> Result<()> {
        if ulcount > 0 {
            *self.done.borrow_mut() = true;
        }
        Ok(())
    }
}

/// TSF 用這個介面來問「你提供哪些樣式」。
///
/// 由 `TextService` 一併實作（同一個 CLSID 對外提供多個介面），所以這裡
/// 只放共用的建構邏輯，實際的 impl 掛在 `text_service.rs`。
pub fn enum_display_attribute_info() -> IEnumTfDisplayAttributeInfo {
    EnumDisplayAttributeInfo::new().into()
}

/// 依 GUID 取得對應的樣式描述。目前只有一種。
pub fn display_attribute_info_by_guid(guid: &GUID) -> Result<ITfDisplayAttributeInfo> {
    if *guid == GUID_DISPLAY_ATTRIBUTE_INPUT {
        Ok(DisplayAttributeInfo.into())
    } else {
        Err(E_INVALIDARG.into())
    }
}
