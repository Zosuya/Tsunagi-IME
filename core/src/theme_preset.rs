//! 主題預設：內建的淺色／深色，以及使用者放在主題資料夾裡的。
//!
//! # 主題跟設定的關係
//!
//! 主題**只是一組顏色**（`config::Colors`），套用＝把那組值填進
//! 目前的設定。所以套用之後使用者還是可以繼續微調個別顏色，
//! 也不會影響行為設定（啟用的語言、選字按鍵那些）。
//!
//! ```text
//! %APPDATA%\tsunagi-ime\
//! ├── config.toml        ← 使用者目前的設定
//! └── themes\            ← 主題資料夾
//!     ├── 森林.toml
//!     └── 夜間.toml
//! ```
//!
//! # 為什麼是 toml 不是 json
//!
//! 設定檔本來就是 toml。同一種格式的話，主題檔可以直接從 `config.toml`
//! 的 `[colors]` 那段複製貼上，不必轉換。

use crate::config::Colors;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 一個主題：名字 + 一組顏色。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    /// 顯示用的名字。檔案沒寫就用檔名。
    #[serde(default)]
    pub name: String,
    pub colors: Colors,
    /// 這組配色搭配的文字描邊濃度。0 = 不描邊。
    ///
    /// **主題是一整套外觀，不只是顏色**。使用者為了看清背景圖上的字
    /// 而開了描邊，把配色匯出成主題時那個設定要跟著走——不然拿到
    /// 另一台電腦套用會少一半，而且看起來像主題壞掉。
    ///
    /// `#[serde(default)]`：舊的主題檔沒有這個欄位，讀進來是 0
    /// （不描邊），跟以前的行為一樣。
    #[serde(default)]
    pub text_outline: f32,
}

/// 內建的淺色主題——Windows 11 淺色模式的配色。
pub fn light() -> Theme {
    Theme {
        name: "淺色".into(),
        colors: Colors {
            window_bg: "#FBFBFB".into(),
            // 極淡的灰階漸層，模仿 Win11 的毛玻璃質感
            window_bg2: "#F0F0F0".into(),
            text: "#1A1A1A".into(),
            index: "#909090".into(),
            highlight_bg: "#0078D4".into(),
            highlight_text: "#FFFFFF".into(),
            preview_text: "#0060A8".into(),
            preview_bg: "#F2F7FB".into(),
            preview_bg2: "#E8F1F8".into(),
            separator: "#E4E4E4".into(),
        },
        text_outline: 0.0,
    }
}

/// 內建的深色主題——Windows 11 深色模式的配色。
///
/// 不是把淺色反相：深色模式的底色是**帶藍的深灰**（不是純黑），
/// 強調色也要提亮才看得清楚（`#0078D4` 在深底上太暗）。
pub fn dark() -> Theme {
    Theme {
        name: "深色".into(),
        colors: Colors {
            window_bg: "#2C2C2C".into(),
            window_bg2: "#252525".into(),
            text: "#F0F0F0".into(),
            index: "#8A8A8A".into(),
            // 深底上的藍要拿捏：太暗會糊進背景，**太亮則壓縮白字的對比**
            // ——原本的 `#2C7FD6` 白字只有 4.11，低於無障礙門檻 4.5，
            // 是「內建主題的對比度都夠」那個測試抓出來的。
            highlight_bg: "#1273C4".into(),
            highlight_text: "#FFFFFF".into(),
            preview_text: "#7FC4F5".into(),
            preview_bg: "#333A40".into(),
            preview_bg2: "#2C3238".into(),
            separator: "#3F3F3F".into(),
        },
        text_outline: 0.0,
    }
}

/// 夜鶯——深藍夜色配青色強調。
///
/// 跟 `dark()` 的中性灰不同，這組整體偏藍，長時間看比較不刺眼。
pub fn nightingale() -> Theme {
    Theme {
        name: "夜鶯".into(),
        colors: Colors {
            window_bg: "#1B2432".into(),
            window_bg2: "#161E29".into(),
            text: "#E6EDF5".into(),
            index: "#8FA0B6".into(),
            highlight_bg: "#1E6FA8".into(),
            highlight_text: "#FFFFFF".into(),
            preview_text: "#8ADCF0".into(),
            preview_bg: "#223046".into(),
            preview_bg2: "#1C2839".into(),
            separator: "#2E3B4E".into(),
        },
        text_outline: 0.0,
    }
}

/// 和紙——暖米白底、墨色字、朱紅強調。
///
/// 這組是給這個輸入法的身分做的：和紙與朱印的配色，中日文都對味。
pub fn washi() -> Theme {
    Theme {
        name: "和紙".into(),
        colors: Colors {
            window_bg: "#F7F2E7".into(),
            window_bg2: "#F0E9DA".into(),
            text: "#2B2620".into(),
            index: "#7A7062".into(),
            // 朱紅。白字壓在上面要夠深才讀得到
            highlight_bg: "#B33A2B".into(),
            highlight_text: "#FFFFFF".into(),
            preview_text: "#8C3A2E".into(),
            preview_bg: "#FBF7EE".into(),
            preview_bg2: "#F4EDE0".into(),
            separator: "#E0D7C4".into(),
        },
        text_outline: 0.0,
    }
}

/// 苔綠——深綠灰，強調色取苔蘚的綠。
pub fn moss() -> Theme {
    Theme {
        name: "苔綠".into(),
        colors: Colors {
            window_bg: "#232B26".into(),
            window_bg2: "#1D241F".into(),
            text: "#E4EBE3".into(),
            index: "#93A68C".into(),
            highlight_bg: "#3F7A3B".into(),
            highlight_text: "#FFFFFF".into(),
            preview_text: "#A8D89F".into(),
            preview_bg: "#2B352D".into(),
            preview_bg2: "#242D26".into(),
            separator: "#36423A".into(),
        },
        text_outline: 0.0,
    }
}

/// 藤紫——深紫底，強調色取藤花。
pub fn wisteria() -> Theme {
    Theme {
        name: "藤紫".into(),
        colors: Colors {
            window_bg: "#282331".into(),
            window_bg2: "#221D2A".into(),
            text: "#EDE7F4".into(),
            index: "#A296B0".into(),
            highlight_bg: "#6B4F99".into(),
            highlight_text: "#FFFFFF".into(),
            preview_text: "#C4A9E8".into(),
            preview_bg: "#322B3D".into(),
            preview_bg2: "#2A2434".into(),
            separator: "#3D3549".into(),
        },
        text_outline: 0.0,
    }
}

/// 高對比——純黑底、白字、黃色強調。
///
/// **這組不是為了好看**，是給視力不好或在強光下用的：每一組配色都
/// 遠超無障礙標準的門檻。
pub fn high_contrast() -> Theme {
    Theme {
        name: "高對比".into(),
        colors: Colors {
            window_bg: "#000000".into(),
            window_bg2: "#000000".into(),
            text: "#FFFFFF".into(),
            index: "#C8C8C8".into(),
            highlight_bg: "#FFD400".into(),
            highlight_text: "#000000".into(),
            preview_text: "#FFE566".into(),
            preview_bg: "#0A0A0A".into(),
            preview_bg2: "#000000".into(),
            separator: "#606060".into(),
        },
        text_outline: 0.0,
    }
}

/// 內建主題。
///
/// **內建而不是放主題資料夾**：資料夾在 `%APPDATA%`，不會跟著版控走，
/// 換一台電腦就沒了。內建的跟著程式碼跑。
pub fn builtin() -> Vec<Theme> {
    vec![
        light(),
        dark(),
        nightingale(),
        washi(),
        moss(),
        wisteria(),
        high_contrast(),
    ]
}

/// 主題資料夾：跟 `config.toml` 放一起。
pub fn themes_dir() -> Option<PathBuf> {
    crate::config::Config::save_path().and_then(|p| p.parent().map(|d| d.join("themes")))
}

/// 讀主題資料夾裡的 `.toml`。
///
/// 讀不到（資料夾不存在、格式錯）就跳過那一個——**一個壞掉的主題檔
/// 不該讓整份清單消失**。
pub fn load_dir(dir: &Path) -> Vec<Theme> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "toml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(mut t) = toml::from_str::<Theme>(&text) else {
            continue;
        };
        // 檔案沒寫名字就用檔名
        if t.name.is_empty() {
            t.name = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "（無名）".into());
        }
        out.push(t);
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// 內建的 + 資料夾裡的。
pub fn all() -> Vec<Theme> {
    let mut out = builtin();
    if let Some(d) = themes_dir() {
        out.extend(load_dir(&d));
    }
    out
}

/// 把一個主題存成檔案（設定頁的「匯出目前配色」用）。
pub fn save(theme: &Theme, dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let safe: String = theme
        .name
        .chars()
        .filter(|c| !r#"\/:*?"<>|"#.contains(*c))
        .collect();
    let name = if safe.is_empty() {
        "主題".to_string()
    } else {
        safe
    };
    let path = dir.join(format!("{name}.toml"));
    let text = toml::to_string_pretty(theme)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 的對比度。1（完全看不見）到 21（黑對白）。
    ///
    /// **配色最容易犯的錯是「好看但看不清楚」**，而那個用眼睛看不出來
    /// ——尤其自己調的時候看久了會習慣。只能用算的。
    fn 對比度(前景: &str, 背景: &str) -> f32 {
        fn 相對亮度(hex: &str) -> f32 {
            let h = hex.trim_start_matches('#');
            let ch = |i: usize| {
                let v = u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
                // sRGB 要先去 gamma 才是線性的光量
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(0) + 0.7152 * ch(2) + 0.0722 * ch(4)
        }
        let (a, b) = (相對亮度(前景), 相對亮度(背景));
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// **每一組內建主題都要讀得到**。
    ///
    /// 新增主題時這個測試就是守門員——調出一組漂亮但反白字看不清的
    /// 配色是很容易的事，而且要等真的用起來才會發現。
    #[test]
    fn 內建主題的對比度都夠() {
        for t in builtin() {
            let c = &t.colors;
            let n = &t.name;
            // 4.5 是無障礙標準對一般文字的門檻
            for (前景, 背景, 說明) in [
                (&c.text, &c.window_bg, "候選字"),
                (&c.highlight_text, &c.highlight_bg, "反白的字"),
                (&c.preview_text, &c.preview_bg, "預覽列的字"),
            ] {
                let r = 對比度(前景, 背景);
                assert!(
                    r >= 4.5,
                    "主題「{n}」的{說明}對比度只有 {r:.2}（{前景} 對 {背景}），至少要 4.5"
                );
            }
            // 提示文字是次要資訊，刻意比較淡，門檻放寬到 3.0
            let r = 對比度(&c.index, &c.window_bg);
            assert!(
                r >= 3.0,
                "主題「{n}」的提示文字對比度只有 {r:.2}，至少要 3.0"
            );
        }
    }

    #[test]
    fn 內建的第一組是淺色第二組是深色() {
        // 順序有意義：下拉選單照這個順序列，最常用的兩組要在最前面
        let b = builtin();
        assert_eq!(b[0].name, "淺色");
        assert_eq!(b[1].name, "深色");
        assert!(b.len() >= 2);
    }

    #[test]
    fn 內建主題的名字不重複() {
        // 重名的話下拉選單會出現兩個一樣的項目，選了也分不出是哪個
        let names: Vec<String> = builtin().into_iter().map(|t| t.name).collect();
        let mut uniq = names.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), names.len(), "有重名：{names:?}");
    }

    #[test]
    fn 深色不是把淺色反相() {
        // 深色模式的底是**帶藍的深灰**不是純黑，強調色也要提亮
        let d = dark();
        assert_ne!(d.colors.window_bg, "#000000");
        assert_ne!(
            d.colors.highlight_bg,
            light().colors.highlight_bg,
            "深底上要用更亮的藍"
        );
    }

    #[test]
    fn 兩組主題的對比都夠() {
        // 底色跟文字的亮度差要夠大，不然看不清楚
        for t in builtin() {
            let bg = lum(&t.colors.window_bg);
            let fg = lum(&t.colors.text);
            assert!(
                (bg - fg).abs() > 100.0,
                "{} 的底色與文字對比不足：{bg} vs {fg}",
                t.name
            );
        }
    }

    #[test]
    fn 主題可以存讀往返() {
        let dir = std::env::temp_dir().join("ime_theme_test");
        let _ = std::fs::remove_dir_all(&dir);
        let t = dark();
        save(&t, &dir).expect("存檔要成功");
        let back = load_dir(&dir);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].name, "深色");
        assert_eq!(back[0].colors.window_bg, t.colors.window_bg);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 壞掉的主題檔跳過就好() {
        // **一個壞檔不該讓整份清單消失**
        let dir = std::env::temp_dir().join("ime_theme_bad");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("壞的.toml"), "這不是 toml {{{").unwrap();
        save(&light(), &dir).unwrap();
        let back = load_dir(&dir);
        assert_eq!(back.len(), 1, "好的那個要讀得到");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 粗略的亮度（給對比檢查用）。
    fn lum(hex: &str) -> f32 {
        let h = hex.trim_start_matches('#');
        let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as f32;
        0.299 * v(0) + 0.587 * v(2) + 0.114 * v(4)
    }
}
