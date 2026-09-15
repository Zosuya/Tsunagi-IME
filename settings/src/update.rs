//! 更新檢查：去 GitHub Releases 查有沒有新版，有就提示並給下載頁連結。
//!
//! # 只查、只提示，不下載也不替換（開發文件 §2.32）
//!
//! - **DLL 換不掉**：Windows 不許覆寫被載入的 DLL，就算背景換成功，已經
//!   開著的程式跑的還是舊版
//! - **沒簽章之前，自動更新是攻擊面**：那等於一條能把任意程式碼送進每個
//!   宿主行程的通道，而輸入法看得到使用者打的每一個字
//!
//! # 為什麼叫系統的 curl，不拉 HTTP 套件
//!
//! 一年查不了幾次的功能，不值得為它拉進 TLS 那一整串相依（rustls、ring、
//! webpki……）。macOS 內建 `/usr/bin/curl`，Windows 10 1803 起內建
//! `curl.exe`，TLS 用系統的。找不到 curl 的極舊系統就是「查不到」，
//! 跟沒網路同一種安靜的失敗。
//!
//! # 只提示正式版（2026-09-14 使用者裁決）
//!
//! beta 不提示——標成 prerelease 的、或標籤帶 `-beta` 這類後綴的一律跳過。
//! 目前發布過的四版全是 beta，所以**現在一定是「已是最新版」**，等第一個
//! 正式版發出來才會提示。
//!
//! # 踩過的兩個 GitHub API 細節（2026-09-14 實測）
//!
//! - **不帶 User-Agent 直接 403**
//! - **`/releases/latest` 在「一個正式版都沒有」時回 404**——跟 repo 不存在、
//!   網址打錯同一個狀態碼，分不出是「沒有正式版」（該顯示已是最新）還是
//!   「查詢壞了」（該顯示無法檢查）。所以拿清單自己挑

use std::sync::{Arc, Mutex};

/// 發布用的公開 repo（開發 repo 是私有的，見 `tools/publish-snapshot.sh`）。
const RELEASES_API: &str = "https://api.github.com/repos/Zosuya/Tsunagi-IME/releases?per_page=20";

/// 一個版本號：`0.3.1`、`v0.3.1-beta`。
///
/// 比較規則照語意化版本的精神，但只做到用得到的程度：先比三段數字；
/// 數字相同時**沒有後綴的正式版比 `-beta` 新**；兩個都有後綴就照字串比。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    nums: [u64; 3],
    pre: Option<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) if !p.is_empty() => (c, Some(p.to_string())),
            Some(_) => return None,
            None => (s, None),
        };
        let mut it = core.split('.');
        let mut nums = [0u64; 3];
        for n in &mut nums {
            *n = it.next()?.parse().ok()?;
        }
        if it.next().is_some() {
            return None;
        }
        Some(Self { nums, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        self.nums
            .cmp(&other.nums)
            .then_with(|| match (&self.pre, &other.pre) {
                (None, None) => Equal,
                (None, Some(_)) => Greater,
                (Some(_), None) => Less,
                (Some(a), Some(b)) => a.cmp(b),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// 查到的最新一版。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    /// Release 頁面。**給頁面不給安裝檔**：頁面上有兩個平台的安裝檔與
    /// Release 說明（macOS 未簽章版要先跑的 `xattr` 那行就寫在那裡），
    /// 直接給檔案連結會跳過說明。
    pub url: String,
}

/// 從 Releases API 回的 JSON 挑出版本號最大的**正式版**。
///
/// - `Err`：回傳看不懂（不是清單）——查詢壞了
/// - `Ok(None)`：查得到，但**一個正式版都沒有**——等於已是最新
///
/// 跳過的：草稿（公開 API 本來就看不到，保險起見再擋一次）、**prerelease**、
/// **標籤帶後綴的**（`-beta`）、標籤解析不了的。prerelease 旗標與後綴兩個都看：
/// 發布時忘了勾 prerelease、或勾了卻用了正式版號，任一邊漏掉都不會誤報。
pub fn newest(json: &str) -> Result<Option<Release>, ()> {
    let list: serde_json::Value = serde_json::from_str(json).map_err(|_| ())?;
    let list = list.as_array().ok_or(())?;
    Ok(list
        .iter()
        .filter(|r| !r["draft"].as_bool().unwrap_or(false))
        .filter(|r| !r["prerelease"].as_bool().unwrap_or(false))
        .filter_map(|r| {
            let tag = r["tag_name"].as_str()?;
            let url = r["html_url"].as_str()?;
            let v = Version::parse(tag)?;
            v.pre.is_none().then_some((v, tag, url))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, tag, url)| Release {
            tag: tag.to_string(),
            url: url.to_string(),
        }))
}

/// 檢查的結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Checking,
    UpToDate,
    Available(Release),
    /// 沒網路、限流、找不到 curl、回傳看不懂——**一律同一種**，不跳視窗。
    Failed,
}

/// 現在的版本跟查到的比，決定要不要提示。
pub fn judge(current: &str, found: Result<Option<Release>, ()>) -> Status {
    let (Some(cur), Ok(found)) = (Version::parse(current), found) else {
        return Status::Failed;
    };
    // 查得到但還沒有任何正式版
    let Some(rel) = found else {
        return Status::UpToDate;
    };
    match Version::parse(&rel.tag) {
        Some(v) if v > cur => Status::Available(rel),
        Some(_) => Status::UpToDate,
        None => Status::Failed,
    }
}

/// 叫系統的 curl 抓 JSON。失敗一律回 `None`。
fn fetch() -> Option<String> {
    let mut cmd = std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    cmd.args([
        "--silent",
        "--fail",
        "--location",
        // 設定頁開著等它，不能無限期卡在背景
        "--max-time",
        "10",
        "--header",
        "Accept: application/vnd.github+json",
        // **不帶 User-Agent，GitHub 直接回 403**
        "--user-agent",
        concat!("tsunagi-ime-settings/", env!("CARGO_PKG_VERSION")),
        RELEASES_API,
    ]);
    // 設定頁是 GUI 程式，叫 console 程式預設會**閃一個黑色視窗**
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    out.status.success().then_some(())?;
    String::from_utf8(out.stdout).ok()
}

/// 背景檢查。結果寫進共用的格子，畫面每一幀去看。
///
/// **不能在畫面執行緒上等**：egui 每一幀都要畫，網路慢的時候整個設定頁
/// 會卡住。查完叫 `request_repaint`，不然要等使用者動一下滑鼠才看得到結果。
#[derive(Clone, Default)]
pub struct Checker {
    state: Arc<Mutex<Option<Status>>>,
}

impl Checker {
    /// 目前的狀態。`None` = 這次開設定頁還沒查過。
    pub fn status(&self) -> Option<Status> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 開始查。正在查的時候再按不會重複發請求。
    pub fn start(&self, ctx: &egui::Context) {
        {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if *s == Some(Status::Checking) {
                return;
            }
            *s = Some(Status::Checking);
        }
        let state = self.state.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let found = fetch().ok_or(()).and_then(|j| newest(&j));
            let status = judge(env!("CARGO_PKG_VERSION"), found);
            *state.lock().unwrap_or_else(|e| e.into_inner()) = Some(status);
            ctx.request_repaint();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn 版本號解析() {
        assert_eq!(v("0.3.1"), v("v0.3.1"));
        assert!(Version::parse("0.3").is_none());
        assert!(Version::parse("0.3.1.2").is_none());
        assert!(Version::parse("abc").is_none());
        assert!(Version::parse("0.3.1-").is_none());
    }

    #[test]
    fn 版本比較() {
        assert!(v("0.3.2") > v("0.3.1"));
        assert!(v("0.10.0") > v("0.9.9"), "要照數字比，不是照字串");
        assert!(v("1.0.0") > v("0.99.99"));
        // 數字相同：正式版比 beta 新
        assert!(v("0.3.1") > v("v0.3.1-beta"));
        assert!(v("v0.3.2-beta") > v("0.3.1"));
    }

    #[test]
    fn 有新的正式版才提示() {
        let rel = Release {
            tag: "v0.4.0".into(),
            url: "https://example/r".into(),
        };
        assert_eq!(
            judge("0.3.1", Ok(Some(rel.clone()))),
            Status::Available(rel)
        );
        let old = Release {
            tag: "v0.3.1".into(),
            url: "u".into(),
        };
        assert_eq!(judge("0.3.1", Ok(Some(old))), Status::UpToDate);
    }

    /// **沒有正式版 ≠ 查詢失敗**。現在發布的全是 beta，這是現況——要顯示
    /// 「已是最新版」，不是「無法檢查更新」。
    #[test]
    fn 還沒有正式版算已是最新() {
        assert_eq!(judge("0.3.1", Ok(None)), Status::UpToDate);
        assert_eq!(judge("0.3.1", Err(())), Status::Failed);
    }

    /// 真實回傳的形狀（2026-09-14 從 API 抓的，欄位刪到只剩用得到的）。
    /// **清單不照版本排**的情況也要挑對，所以故意把舊版放前面。
    #[test]
    fn 從清單挑最大的正式版() {
        let json = r#"[
            {"tag_name":"v0.4.0-beta","html_url":"https://g/r/v0.4.0-beta","draft":false,"prerelease":true},
            {"tag_name":"v0.3.0","html_url":"https://g/r/v0.3.0","draft":false,"prerelease":false},
            {"tag_name":"v0.3.2","html_url":"https://g/r/v0.3.2","draft":false,"prerelease":false},
            {"tag_name":"v0.5.0","html_url":"https://g/r/忘了勾","draft":false,"prerelease":true},
            {"tag_name":"v0.6.0-rc1","html_url":"https://g/r/沒勾","draft":false,"prerelease":false},
            {"tag_name":"v9.9.9","html_url":"https://g/r/draft","draft":true,"prerelease":false},
            {"tag_name":"nightly","html_url":"https://g/r/nightly","draft":false,"prerelease":false}
        ]"#;
        assert_eq!(
            newest(json),
            Ok(Some(Release {
                tag: "v0.3.2".into(),
                url: "https://g/r/v0.3.2".into(),
            })),
            "beta、prerelease、帶後綴、草稿、解析不了的都要跳過"
        );
    }

    /// 現在的真實情況：四版全是 beta。
    #[test]
    fn 全是beta時沒有可提示的() {
        let json = r#"[
            {"tag_name":"v0.3.1-beta","html_url":"https://g/r/1","draft":false,"prerelease":true},
            {"tag_name":"v0.3.0-beta","html_url":"https://g/r/2","draft":false,"prerelease":true}
        ]"#;
        assert_eq!(newest(json), Ok(None));
    }

    /// **真的連一次 GitHub**。平常跳過（要網路、會吃限流額度），改動 curl
    /// 參數或 API 網址之後手動跑：
    /// `cargo test -p ime-settings -- --ignored 真的連`
    #[test]
    #[ignore]
    fn 真的連一次github() {
        let json =
            fetch().expect("curl 抓不到——沒網路、被限流，或參數寫錯（少了 User-Agent 會 403）");
        let found = newest(&json).expect("回傳看不懂——API 格式變了？");
        // 還沒發正式版時是 None，那也算連線與解析都正常
        if let Some(rel) = found {
            assert!(rel.url.starts_with("https://github.com/"), "{rel:?}");
        }
    }

    #[test]
    fn 看不懂的回傳不崩() {
        assert_eq!(newest(""), Err(()));
        assert_eq!(newest(r#"{"message":"Not Found"}"#), Err(()));
        assert_eq!(newest("[]"), Ok(None), "空清單是查得到但沒東西，不是壞掉");
    }
}
