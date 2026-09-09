//! macOS 的系統檔案對話框（`NSOpenPanel`）。
//!
//! 對應 Windows 的 `IFileOpenDialog`（`main.rs` 的 `pick_folder`）與
//! `GetOpenFileNameW`（`image_dialog.rs`）。**兩種用途共用同一個面板**，
//! 差別只在「能選什麼」那三個開關。
//!
//! # 為什麼不用 rfd
//!
//! 這個 crate 在 macOS 上要接的東西不只檔案對話框（字型清單、圖片解碼
//! 都在後面等），而 `platform/macos` 已經在用 objc2。多拉一套只為了一個
//! 對話框，等於同時維護兩套 macOS 的呼叫慣例。**版本也刻意跟
//! `platform/macos` 對齊**——分岔的話 workspace 會編出兩份 objc2，
//! 型別互不相容。

use std::path::{Path, PathBuf};

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
use objc2_foundation::{NSString, NSURL};

/// 要選什麼。
pub enum Want {
    /// 資料夾（擴充包放哪）
    Folder,
    /// 圖片檔（候選視窗的背景圖）
    Image,
}

/// 開系統對話框，回傳選到的路徑。取消回 `None`。
///
/// **一定要在主執行緒叫**——`runModal` 會跑一個巢狀的事件迴圈，不在主
/// 執行緒就直接爆。egui 的 `update` 本來就在主執行緒，所以從按鈕的回呼
/// 裡叫是安全的；`MainThreadMarker::new()` 順便把這件事變成編譯期以外
/// 的保險（拿不到就回 `None`，不會崩掉整個設定頁）。
pub fn pick(want: Want, start: Option<&Path>) -> Option<String> {
    let mtm = MainThreadMarker::new()?;
    let panel = NSOpenPanel::openPanel(mtm);

    match want {
        Want::Folder => {
            panel.setCanChooseDirectories(true);
            panel.setCanChooseFiles(false);
        }
        Want::Image => {
            panel.setCanChooseDirectories(false);
            panel.setCanChooseFiles(true);
            set_image_filter(&panel);
        }
    }
    panel.setAllowsMultipleSelection(false);

    if let Some(dir) = start.filter(|p| p.is_dir()) {
        let s = NSString::from_str(&dir.to_string_lossy());
        panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&s)));
    }

    // **這裡會停在這一行**直到使用者按確定或取消——那正是「模態」的意思。
    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    let urls = panel.URLs();
    let url: Retained<NSURL> = urls.iter().next()?;
    let path = url.path()?;
    Some(
        PathBuf::from(path.to_string())
            .to_string_lossy()
            .into_owned(),
    )
}

/// 只讓選圖片檔。
///
/// # 為什麼用已經標為 deprecated 的那支
///
/// 現行的作法是 `setAllowedContentTypes:`，型別是 `UTType`——那要再拉一個
/// `objc2-uniform-type-identifiers`。**只為了三個副檔名多一個依賴不划算**，
/// 而 `setAllowedFileTypes:` 在 AppKit 裡仍然活著（只是 macOS 12 起不建議）。
/// objc2 沒有生出這支的繫結，所以直接送訊息。
///
/// 副檔名跟 `config::Background::image` 的說明一致（PNG／JPG／BMP）。
fn set_image_filter(panel: &NSOpenPanel) {
    use objc2::msg_send;
    use objc2_foundation::NSArray;

    let types = ["png", "jpg", "jpeg", "bmp"].map(NSString::from_str);
    let types = NSArray::from_retained_slice(&types);
    unsafe {
        let _: () = msg_send![panel, setAllowedFileTypes: &*types];
    }
}
