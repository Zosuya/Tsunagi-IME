//! `Session` 的測試。三個模組：一般、鎖定、注音鎖定。

#[cfg(test)]
mod 一般 {

    /// 日文詞界調整（文節伸縮）。
    ///
    /// **這是「第一次輸入引擎不認識的詞」的唯一途徑**——Viterbi 只能給
    /// 詞典查得到的分法，詞典沒收的專有名詞再怎麼選字都拼不出來。
    mod 日文詞界 {
        use super::*;

        fn 選到第一格(keys: &str) -> Session {
            let mut s = typed(keys);
            s.enter_select_first();
            s
        }

        #[test]
        fn 往右推會把假名吃過來() {
            if !load() {
                return;
            }
            // ごはん|を|たべ|ます
            let mut s = 選到第一格("gohannwotabemasu");
            let before: Vec<String> = s.slots().iter().map(|x| x.keys.clone()).collect();
            assert!(before.len() >= 2, "該切成多格：{before:?}");
            assert!(s.widen_word(), "第一格該推得動");
            let after: Vec<String> = s.slots().iter().map(|x| x.keys.clone()).collect();
            assert_ne!(before, after, "詞界該變了");
            // **按鍵一定要接得回原字串**——check_rewrite、整格刪除、
            // 學習全都靠這個性質
            assert_eq!(after.concat(), "gohannwotabemasu");
        }

        #[test]
        fn 往左收也接得回原字串() {
            if !load() {
                return;
            }
            let mut s = 選到第一格("gohannwotabemasu");
            s.widen_word();
            s.narrow_word();
            let after: String = s.slots().iter().map(|x| x.keys.as_str()).collect();
            assert_eq!(after, "gohannwotabemasu");
        }

        /// 注音格不該被詞界調整動到——那是日文才有的概念。
        #[test]
        fn 注音格調不動() {
            if !load() {
                return;
            }
            let mut s = 選到第一格("su3cl3");
            assert!(!s.widen_word(), "注音不該有詞界調整");
            assert!(!s.narrow_word());
        }
    }

    /// 只打一個字時不該跳去別的語言。
    ///
    /// 根因是 `covered` 對注音**只算多音節詞**，所以單音節的 covered 是 0，
    /// 而切成「日文假名＋殘渣」反而有分（`ru` `su` 都是合法假名）。
    /// 實測 150 個高頻單音節有 7 個被這樣搶走。
    mod 單音節 {
        use super::*;

        #[test]
        fn 不會被切成日文碎片() {
            if !load() {
                return;
            }
            // 這五個都是實測抓到的：ㄌㄧㄠˇ了、ㄐㄧㄚ家、ㄋㄧㄢˊ年、
            // ㄐㄧㄥ經、ㄐㄧㄡˋ就
            for (keys, want) in [
                ("xul3", '了'),
                ("ru8 ", '家'),
                ("su06", '年'),
                ("ru/ ", '經'),
                ("ru.4", '就'),
            ] {
                let s = typed(keys);
                let got = s.text();
                assert_eq!(
                    got.chars().next(),
                    Some(want),
                    "{keys} 該給「{want}」，實得 {got:?}"
                );
            }
        }

        /// **單音節也要有中文代表**——不然選單裡連一個保證的中文選項
        /// 都沒有，第一名萬一錯了就沒得選。
        #[test]
        fn 選單裡有中文代表() {
            if !load() {
                return;
            }
            let menu = typed("su3").cutting_menu(8);
            assert!(
                menu.iter().any(|m| m.starts_with("（中")),
                "該有中文代表：{menu:?}"
            );
        }

        /// **單母音的選單要有日文可選**。
        ///
        /// `a` 在日文引擎是合法的（あ），但它同時是英文最常用的詞，
        /// `lang_of` 那條「很常用的英文詞不讓給日文」（本來是為了擋
        /// `you`→よう）把它判給英文，於是切法裡一個日文段都沒有。
        /// 英文代表是造出來的，日文原本只能從既有切法裡挑，所以就沒得選。
        #[test]
        fn 單母音也有日文可選() {
            if !load() {
                return;
            }
            for k in ["a", "i", "u", "e", "o"] {
                let menu = typed(k).cutting_menu(8);
                assert!(
                    menu.iter().any(|m| m.starts_with("（日")),
                    "{k} 該有日文選項：{menu:?}"
                );
            }
        }

        /// 這條規則**只在整串剛好是一個音節時**作用，長輸入不受影響。
        #[test]
        fn 長輸入不受影響() {
            if !load() {
                return;
            }
            // 兩個音節：切法照原本的排序決定，不該被單音節規則干擾
            assert_eq!(typed("su3cl3").text(), "你好");
        }
    }

    /// 鎖定注音時的標點：`,` `.` `;` `/` `-` 在大千配置上是 ㄝㄡㄤㄥㄦ，
    /// 一鍵兩用。判準是**多看一鍵**——接聲調就是注音，否則構不成字。
    mod 鎖定注音的標點 {
        use super::*;
        use crate::config::LockPunct;

        fn 鎖定注音(keys: &str) -> Session {
            let mut s = Session::new();
            s.set_lock(Some(crate::language::Language::Bopomofo));
            for c in keys.chars() {
                s.push(c);
            }
            s
        }

        /// **打完一個字接逗號**——最常見的情況，改之前完全打不出來。
        #[test]
        fn 字後面接得出逗號() {
            if !load() {
                return;
            }
            // 打完就要看得到——直接按 Enter 送出也是這個字串
            let s = 鎖定注音("su3,");
            assert!(
                s.text().ends_with('，') || s.text().ends_with(','),
                "逗號要出得來：{:?}",
                s.text()
            );
            // 後面再接字也還在
            let s = 鎖定注音("su3,cl3");
            assert!(
                s.text().contains('，') || s.text().contains(','),
                "{:?}",
                s.text()
            );
        }

        /// 接了聲調就是注音——`,4` 是 ㄝˋ（欸），不能被當成標點。
        #[test]
        fn 接了聲調就是注音() {
            if !load() {
                return;
            }
            let s = 鎖定注音(",4");
            assert!(
                !s.text().contains('，') && !s.text().contains(','),
                "ㄝˋ 是注音不是標點：{:?}",
                s.text()
            );
        }

        /// 「二」「而」都是 ㄦ 開頭——不能因為 `-` 長得像標點就打不出來。
        #[test]
        fn ㄦ開頭的字還打得出來() {
            if !load() {
                return;
            }
            let s = 鎖定注音("-4");
            assert!(
                !s.text().contains('-'),
                "ㄦˋ（二）要打得出來：{:?}",
                s.text()
            );
        }

        /// `Ctrl+鍵` 明講「我要標點」——**不管設定怎麼設都照做**。
        ///
        /// 它存在的理由就是給「一律當注音符號」那個設定一條出路。
        #[test]
        fn 明講的標點不管設定都照做() {
            if !load() {
                return;
            }
            for mode in [LockPunct::Auto, LockPunct::Symbol] {
                let mut s = Session::new();
                s.set_lock(Some(crate::language::Language::Bopomofo));
                s.set_lock_punct(mode);
                for c in "su3".chars() {
                    s.push(c);
                }
                assert!(s.push_punct(','), "鎖定注音時該接手");
                assert!(
                    s.text().contains('，') || s.text().contains(','),
                    "{mode:?} 下明講的標點要出得來：{:?}",
                    s.text()
                );
            }
        }

        /// **明講的標點不該被後面的聲調取回去當注音**。
        ///
        /// 自動判斷才有「多看一鍵」那條規則；使用者已經表態了就不該再猜。
        #[test]
        fn 明講之後按聲調不會把標點吃掉() {
            if !load() {
                return;
            }
            let mut s = Session::new();
            s.set_lock(Some(crate::language::Language::Bopomofo));
            for c in "su3".chars() {
                s.push(c);
            }
            s.push_punct(',');
            s.push('4');
            assert!(
                s.text().contains('，') || s.text().contains(','),
                "標點要還在：{:?}",
                s.text()
            );
        }

        /// 沒有鎖定注音時不接手——那些模式的標點鍵本來就打得出標點。
        #[test]
        fn 非鎖定注音時不接手() {
            if !load() {
                return;
            }
            let mut s = Session::new();
            assert!(!s.push_punct(','), "自動模式不該接手");
        }

        /// 設定成「一律當注音符號」時不做延後判斷。
        #[test]
        fn 設定關掉就一律當注音() {
            if !load() {
                return;
            }
            let mut s = Session::new();
            s.set_lock(Some(crate::language::Language::Bopomofo));
            s.set_lock_punct(LockPunct::Symbol);
            for c in "su3,".chars() {
                s.push(c);
            }
            assert!(
                !s.text().contains('，'),
                "關掉之後不該冒出標點：{:?}",
                s.text()
            );
        }
    }

    /// 鎖定語言時，有反白框就用倒退鍵刪掉整格。
    mod 鎖定時刪整格 {
        use super::*;

        fn 鎖定注音(keys: &str) -> Session {
            let mut s = Session::new();
            s.set_lock(Some(crate::language::Language::Bopomofo));
            for c in keys.chars() {
                s.push(c);
            }
            s
        }

        #[test]
        fn 刪掉反白那一格() {
            if !load() {
                return;
            }
            let mut s = 鎖定注音("su3cl3");
            let before = s.slots().len();
            assert!(before >= 2, "該有兩格：{:?}", s.text());
            s.enter_select_last();
            s.backspace();
            assert_eq!(s.slots().len(), before - 1, "整格該不見了");
        }

        #[test]
        fn 刪完框往前挪一格() {
            if !load() {
                return;
            }
            let mut s = 鎖定注音("su3cl3");
            s.enter_select_last(); // 停在第 1 格
            assert_eq!(s.select_index(), Some(1));
            s.backspace();
            assert_eq!(s.select_index(), Some(0), "框該挪到前一格，不是消失");
        }

        #[test]
        fn 沒有框時維持原本的退格() {
            if !load() {
                return;
            }
            // **鎖定注音的退格本來就是刪掉整個音節**（新酷音式的音節
            // 緩衝），不是刪一個鍵。這條路不能被新功能改掉。
            let mut s = 鎖定注音("su3cl3");
            let before = s.slots().len();
            s.backspace();
            assert_eq!(s.slots().len(), before - 1, "尾端那一格該不見了");
        }

        #[test]
        fn 框在中間時刪的是中間那一格() {
            if !load() {
                return;
            }
            // **這才是新功能真正的價值**：框在尾端時新舊行為一樣（都刪
            // 最後一格），差別在框停在中間的時候——舊行為只會從尾端啃。
            let mut s = 鎖定注音("su3cl3");
            assert_eq!(s.slots().len(), 2);
            let last = s.slots()[1].text.clone();
            s.enter_select_first(); // 框停在第 0 格
            s.backspace();
            assert_eq!(s.slots().len(), 1, "該少一格");
            assert_eq!(s.slots()[0].text, last, "留下的該是後面那一格");
        }

        #[test]
        fn default建的也是開的() {
            if !load() {
                return;
            }
            // **TSF 那層的 `State` 是 `derive(Default)` 建的**，正式環境
            // 真的走這條路。`bool` 的預設是 false，跟產品預設相反——
            // 那個落差用 `DefaultOn` 這個型別擋掉了，這裡把它釘住。
            let mut s = Session::default();
            s.set_lock(Some(crate::language::Language::Bopomofo));
            for c in "su3cl3".chars() {
                s.push(c);
            }
            let before = s.slots().len();
            s.enter_select_first();
            s.backspace();
            assert_eq!(s.slots().len(), before - 1, "Default 建的也該刪整格");
        }

        #[test]
        fn 關掉之後回到原本的退格() {
            if !load() {
                return;
            }
            let mut s = 鎖定注音("su3cl3");
            s.set_backspace_whole_cell(false);
            let before = s.slots().len();
            s.enter_select_first(); // 框在第 0 格
            s.backspace();
            // 關掉之後走原本那條路——刪的是**尾端**的音節，不是框那一格
            assert_eq!(s.slots().len(), before - 1);
            assert_eq!(s.slots()[0].text, "你", "第一格該還在");
        }

        #[test]
        fn 自動模式不受影響() {
            if !load() {
                return;
            }
            // 這條路只在鎖定時走——自動模式的一格未必對應一個字
            let mut s = typed("su3cl3");
            s.enter_select_last();
            let keys_before = s.keys().len();
            s.backspace();
            assert_eq!(s.keys().len(), keys_before - 1, "自動模式仍是刪一個鍵");
        }

        #[test]
        fn 刪到空的不會出錯() {
            if !load() {
                return;
            }
            let mut s = 鎖定注音("su3");
            s.enter_select_last();
            s.backspace();
            assert!(s.is_empty() || s.slots().is_empty());
            s.backspace(); // 再刪一次也不能炸
        }
    }

    /// 切法選單的 4～6 名固定放三種語言各自的代表。
    mod 三語代表 {
        use super::*;

        #[test]
        fn 整串英文一定看得到() {
            if !load() {
                return;
            }
            // `su3cl3` 是中文，純英文 passthrough 在排序裡沒有任何
            // 依據，不特別補的話翻很久都翻不到
            let s = typed("su3cl3");
            let menu = s.cutting_menu(8);
            // 前面會有「（英）」記號，比對結尾就好
            assert!(
                menu.iter().any(|m| m.ends_with("su3cl3")),
                "整串 passthrough 該在選單裡：{menu:?}"
            );
        }

        #[test]
        fn 前三名一動也不動() {
            if !load() {
                return;
            }
            let s = typed("su3cl3");
            let menu = s.cutting_menu(3);
            assert!(
                menu[0].ends_with("你好"),
                "第一名是引擎算的最佳解，不能被擠掉：{menu:?}"
            );
            assert_eq!(menu.len(), 3);
        }

        #[test]
        fn 已經在前面的不重複列() {
            if !load() {
                return;
            }
            let s = typed("sushi");
            let menu = s.cutting_menu(8);
            let mut uniq = menu.clone();
            uniq.sort();
            uniq.dedup();
            assert_eq!(uniq.len(), menu.len(), "不該有重複項：{menu:?}");
        }

        #[test]
        fn 候選很少時不會出錯() {
            if !load() {
                return;
            }
            // 只打一個鍵，切法沒幾種
            let s = typed("a");
            let menu = s.cutting_menu(8);
            assert!(!menu.is_empty());
            let mut uniq = menu.clone();
            uniq.sort();
            uniq.dedup();
            assert_eq!(uniq.len(), menu.len(), "不該有重複項：{menu:?}");
        }

        #[test]
        fn 代表前面有語言記號() {
            if !load() {
                return;
            }
            let s = typed("su3cl3");
            let menu = s.cutting_menu(8);
            assert!(menu[0].starts_with("（中）"), "第一名是中文代表：{menu:?}");
            assert!(
                menu.iter().any(|m| m.starts_with("（英）")),
                "整串英文也要標：{menu:?}"
            );
        }

        #[test]
        fn 代表看引擎認可的字數不是按鍵數() {
            if !load() {
                return;
            }
            // 「ちぇ喝一下」的注音按鍵比「check 一下」多，但那些不是詞。
            // 用按鍵數挑的話中文代表會挑到前者——那是使用者不要的東西。
            let s = typed("check u vu84");
            let menu = s.cutting_menu(8);
            let zh = menu
                .iter()
                .find(|m| m.starts_with("（中"))
                .expect("該有中文代表");
            assert!(zh.contains("check 一下"), "中文代表挑錯了：{zh}");
        }

        /// **整串讀得通的話，（中）那一列就該是純中文**。
        ///
        /// 舊做法挑「涵蓋字數最多」的那一種，而 `covered_by` 問的是
        /// 「整段是不是一個詞」——一整句中文拿 0 分，反而輸給被切碎、
        /// 每小塊剛好是個詞的那一種，於是（中）混進了日文段。
        #[test]
        fn 中文代表整串讀得通時是純中文() {
            if !load() {
                return;
            }
            // 這種情況就要用切法的
            let s = typed("5k45j/3fu/6dj;4ru.4ul4m/4fu, z832k7");
            let menu = s.cutting_menu(8);
            let zh = menu
                .iter()
                .find(|m| m.starts_with("（中"))
                .expect("該有中文代表");
            assert!(
                !zh.chars().any(|c| c.is_ascii_alphanumeric()),
                "（中）不該混著沒轉換的按鍵：{zh}"
            );
            assert!(zh.contains("這種情況"), "中文代表挑錯了：{zh}");
        }

        /// **句子中間不該露出沒轉換的按鍵**。
        ///
        /// `fu/6`（ㄥˊ）是合法的打字中途狀態，但沒有字念這個音，
        /// 顯示出來就是一串原始按鍵夾在中文裡。
        #[test]
        fn 顯示不出來的切法要排後面() {
            if !load() {
                return;
            }
            let s = typed("5k45j/3fu/6dj;4ru.4g4ul4m/4");
            let menu = s.cutting_menu(3);
            assert!(
                !menu.iter().any(|m| m.contains("fu/6")),
                "前三名不該有露出按鍵的：{menu:?}"
            );
        }

        #[test]
        fn 同時是兩個語言的代表就都標() {
            if !load() {
                return;
            }
            // 覆蓋的話畫面上會少一個記號，使用者就找不到那個語言的代表
            let s = typed("sushi");
            let menu = s.cutting_menu(8);
            let marked = menu.iter().filter(|m| m.starts_with("（")).count();
            assert!(marked >= 2, "至少該標到日文與英文：{menu:?}");
        }

        #[test]
        fn 沒打字時不會生出東西() {
            if !load() {
                return;
            }
            let s = Session::new();
            assert!(s.cutting_menu(8).is_empty());
        }
    }

    use crate::session::*;

    fn load() -> bool {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("data");
        crate::preload(&data, crate::config::Engines::default());
        crate::dict::bopomofo_loaded()
    }

    fn typed(keys: &str) -> Session {
        let mut s = Session::new();
        for c in keys.chars() {
            s.push(c);
        }
        s
    }

    /// 注音符號直出：選單尾巴那一列。
    ///
    /// 它**不是一種切法**（不經過選詞層、沒有候選），所以要確認的不只是
    /// 「有沒有出現」，還有選中之後的狀態對不對。
    mod 符號直出 {
        use super::*;

        /// 選單上找出「（ㄅ）」那一列的位置。
        fn 符號列(s: &Session) -> Option<(usize, String)> {
            s.cutting_menu(12)
                .into_iter()
                .enumerate()
                .find(|(_, m)| m.starts_with("（ㄅ）"))
        }

        #[test]
        fn 音節連寫不加空格() {
            if !load() {
                return;
            }
            let s = typed("su3cl3");
            let (_, row) = 符號列(&s).expect("該有符號那一列");
            assert_eq!(row, "（ㄅ）ㄋㄧˇㄏㄠˇ", "音節連寫，中間不加空格");
        }

        /// **單獨的聲母與聲調都要打得出來**——這正是這個功能的用途。
        ///
        /// `ㄅ` 不能自成音節，所以「要求合法音節」的版本打不出來；
        /// 「符號＋空白」那招也救不回聲母（空白是一聲，等於在問
        /// 「ㄅ 自己能不能成為一個字」，答案是不能）。
        #[test]
        fn 單獨的聲母與聲調都打得出來() {
            if !load() {
                return;
            }
            // 1qaz = ㄅㄆㄇㄈ
            let (_, row) = 符號列(&typed("1qaz")).expect("聲母序列要打得出來");
            assert_eq!(row, "（ㄅ）ㄅㄆㄇㄈ");
            // 3 = ˇ（寫文件講聲調時要用）
            let (_, row) = 符號列(&typed("3")).expect("聲調符號要打得出來");
            assert_eq!(row, "（ㄅ）ˇ");
        }

        /// 一聲是空白鍵、沒有對應符號——跳過才是正確的注音寫法。
        #[test]
        fn 一聲不標符號() {
            if !load() {
                return;
            }
            let (_, row) = 符號列(&typed("y ")).expect("ㄗ 一聲");
            assert_eq!(row, "（ㄅ）ㄗ", "一聲不該多出任何記號");
        }

        /// **排在三語代表後面**（使用者定）。
        #[test]
        fn 排在中英日後面() {
            if !load() {
                return;
            }
            let s = typed("su3cl3");
            let menu = s.cutting_menu(12);
            let (i, _) = 符號列(&s).unwrap();
            let 最後一個代表 = menu
                .iter()
                .rposition(|m| {
                    m.starts_with("（中") || m.starts_with("（日") || m.starts_with("（英")
                })
                .expect("該有語言代表");
            assert!(i > 最後一個代表, "符號那列要在代表後面：{menu:?}");
        }

        /// 選中之後：送出的是符號，而且**不能逐格選字**（沒有候選可挑）。
        #[test]
        fn 選中之後送出符號且不給選字() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            let (i, _) = 符號列(&s).unwrap();
            s.set_cutting_index(i);
            assert_eq!(s.text(), "ㄋㄧˇㄏㄠˇ");
            assert_eq!(s.slots().len(), 1, "整串一格");
            assert!(!s.slots()[0].selectable, "符號沒有候選可選");
        }

        /// **選了之後再打字要留在這一列**。
        ///
        /// 每打一鍵選單都會重排，不記著的話會跳回中文——那正是使用者
        /// 覺得「前面的字突然變了」的那類問題。
        #[test]
        fn 再打一個字仍留在符號列() {
            if !load() {
                return;
            }
            let mut s = typed("su3");
            let (i, _) = 符號列(&s).unwrap();
            s.set_cutting_index(i);
            for c in "cl3".chars() {
                s.push(c);
            }
            assert_eq!(
                s.text(),
                "ㄋㄧˇㄏㄠˇ",
                "還在符號那一列：{:?}",
                s.cutting_menu(12)
            );
        }

        /// 大千配置把 26 個字母全用掉了，所以幾乎任何輸入都有這一列。
        /// 真的沒有的情況是**打了不在配置上的鍵**——大寫字母不算
        /// （輸入層會轉小寫，suE 一樣給得出 ㄋㄧㄍ），要像 = 這種
        /// 鍵盤上有、注音配置沒有的鍵。
        #[test]
        fn 打了配置外的鍵就沒有這一列() {
            if !load() {
                return;
            }
            let s = typed("su3=");
            assert!(
                符號列(&s).is_none(),
                "有配置外的鍵就不該有：{:?}",
                s.cutting_menu(12)
            );
            assert!(!s.text().is_empty(), "還是要有輸出");
        }
    }

    #[test]
    fn 打字後有文字() {
        if !load() {
            eprintln!("詞庫未下載，跳過（跑 data/download.ps1）");
            return;
        }
        let s = typed("su3cl3");
        assert_eq!(s.text(), "你好");
    }

    #[test]
    fn 切法選單可以翻() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        let first = s.text();
        assert!(s.cutting_count() > 1, "該有多種切法");
        s.next_cutting();
        assert_ne!(s.cutting_index(), 0);
        s.prev_cutting();
        assert_eq!(s.cutting_index(), 0);
        assert_eq!(s.text(), first, "轉一圈要回到原點");
    }

    /// 離開選字之後「停在哪一格」這件事要記著。使用者定的行為。
    mod 離開選字之後 {
        use super::*;

        #[test]
        fn 標記留在剛改的那一格() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            s.select_right(); // 移到第二格
            assert_eq!(s.select_index(), Some(1));
            s.exit_select();
            assert_eq!(s.select_index(), None, "已經離開選字");
            assert_eq!(
                s.marked_index(),
                Some(1),
                "但那一格的標記要留著，使用者才看得到自己改了哪個字"
            );
        }

        #[test]
        fn 再進選字從那一格接續() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            s.select_right();
            s.exit_select();
            s.enter_select_first();
            assert_eq!(s.select_index(), Some(1), "不該跳回第一格");
        }

        #[test]
        fn 左鍵進選字也接續() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            s.exit_select(); // 停在第 0 格
            s.enter_select_last();
            assert_eq!(s.select_index(), Some(0), "接續優先於「跳到最後一格」");
        }

        #[test]
        fn 又打字就把標記收掉() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            s.exit_select();
            assert!(s.marked_index().is_some());
            s.push('d');
            assert_eq!(
                s.marked_index(),
                None,
                "繼續打字之後那個標記就過期了，留著會是個沒人管的框"
            );
        }

        #[test]
        fn 按完enter再按右鍵就直接跳下一格() {
            if !load() {
                return;
            }
            // 「重新進入選字」是內部細節——使用者看到框在那裡，
            // 按右鍵就該移一格，不是按兩下才動
            let mut s = typed("su3cl3");
            s.enter_select_first();
            assert_eq!(s.select_index(), Some(0));
            s.exit_select(); // 等同按 Enter（選完就離開那個模式）
            s.arrow_right();
            assert_eq!(s.select_index(), Some(1), "一下就該移到下一格");
        }

        #[test]
        fn 按完enter再按左鍵也直接移動() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            s.select_right(); // 停在第 1 格
            s.exit_select();
            s.arrow_left();
            assert_eq!(s.select_index(), Some(0), "一下就該往左移");
        }

        #[test]
        fn 什麼都沒有時方向鍵只是進選字() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            let last = (0..s.slots().len())
                .rev()
                .find(|&i| s.slots()[i].selectable);
            s.arrow_right();
            assert_eq!(s.select_index(), last, "第一下只負責把框叫出來，不移動");
        }

        #[test]
        fn 剛打完字進選字落在最後一格() {
            if !load() {
                return;
            }
            // 生產環境的方向鍵走這條：插入點在尾端，最可能要改的是
            // 剛打的那個字，落在最前面等於要一路按回來
            let mut s = typed("su3cl3");
            let last = (0..s.slots().len())
                .rev()
                .find(|&i| s.slots()[i].selectable);
            s.enter_select_last();
            assert_eq!(s.select_index(), last, "該落在最後一個能選的格子");
            assert_ne!(s.select_index(), Some(0), "不該跳到最前面");
        }

        #[test]
        fn 沒選過就照原本的規則() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            assert_eq!(s.select_index(), Some(0), "第一次進來還是從第一格");
        }
    }

    /// 反白框與候選清單是兩件事：框是游標，清單要按下鍵才叫出來。
    mod 框與候選分開 {
        use super::*;

        #[test]
        fn 進選字時清單是關的() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            assert!(s.select_index().is_some(), "框要出來");
            assert!(!s.cands_open(), "但清單不該跟著彈出來");
        }

        #[test]
        fn 左右移動不會打開清單() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            s.select_right();
            s.select_left();
            assert!(!s.cands_open(), "只是把框移過去看看，清單不該開");
        }

        #[test]
        fn 按下鍵第一下只負責打開() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            let before = s.cand_index();
            s.next_cand();
            assert!(s.cands_open(), "第一下要把清單打開");
            assert_eq!(s.cand_index(), before, "但不該順便換到第二個字");
        }

        #[test]
        fn 打開之後才真的換字() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            s.next_cand(); // 打開
            s.next_cand(); // 這一下才換
            assert_eq!(s.cand_index(), 1);
        }

        #[test]
        fn 移動到別格時清單維持開著() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            s.open_cands();
            s.select_right();
            assert!(s.cands_open(), "正在挑字時換一格，應該繼續挑");
        }

        #[test]
        fn 離開選字會關掉清單() {
            if !load() {
                return;
            }
            let mut s = typed("su3cl3");
            s.enter_select_first();
            s.open_cands();
            s.exit_select();
            assert!(!s.cands_open());
        }
    }

    #[test]
    fn 選字跳過英文段() {
        if !load() {
            return;
        }
        // check␣一下：check 是英文，不能選字
        let mut s = typed("check u vu84");
        s.enter_select_first();
        let i = s.select_index().expect("該找得到能選的格子");
        assert!(s.slots()[i].selectable);
        assert_ne!(s.slots()[i].text, "check", "英文段要跳過");
    }

    #[test]
    fn 選字左右移動() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        assert_eq!(s.select_index(), Some(0));
        s.select_right();
        assert_eq!(s.select_index(), Some(1));
        s.select_left();
        assert_eq!(s.select_index(), Some(0));
        // 到頭了不該再往左
        s.select_left();
        assert_eq!(s.select_index(), Some(0));
    }

    #[test]
    fn 選字之後後面跟著改() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.pick_char("妳");
        assert_eq!(s.slots()[0].text, "妳", "使用者選的不能被改掉");
    }

    #[test]
    fn 退格會重算() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.backspace();
        assert_eq!(s.keys(), "su3cl");
        s.backspace();
        assert_eq!(s.keys(), "su3c");
    }

    #[test]
    fn 按左鍵進選字要從最後一格開始() {
        if !load() {
            return;
        }
        // 使用者回報：打「你好」按左鍵會跳過「好」
        let mut s = typed("su3cl3");
        s.enter_select_last();
        assert_eq!(s.select_index(), Some(1), "左鍵進入該反白最後一格");
        s.select_left();
        assert_eq!(s.select_index(), Some(0));
    }

    #[test]
    fn 候選字反白可以上下移動() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.open_cands(); // 這裡測的是候選字的移動，不是清單的開關
        assert_eq!(s.cand_index(), 0);
        let n = s.char_candidates().len();
        assert!(n > 1, "su3 該有多個同音字");
        s.next_cand();
        assert_eq!(s.cand_index(), 1);
        s.prev_cand();
        assert_eq!(s.cand_index(), 0);
        // 往上繞到**可見範圍**的最後一個。
        // 沒展開時只列 CHAR_PAGE 個，繞回的是第 10 個而不是第 29 個——
        // 反白不能指在看不見的地方。
        s.prev_cand();
        assert_eq!(s.cand_index(), n.min(CHAR_PAGE) - 1);
    }

    #[test]
    fn 換格時反白回到第一個() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.open_cands(); // 這裡測的是候選字的移動，不是清單的開關
        s.next_cand();
        s.next_cand();
        assert_eq!(s.cand_index(), 2);
        s.select_right();
        assert_eq!(s.cand_index(), 0, "換格要從頭反白");
    }

    #[test]
    fn 確認候選字會選中並往右() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.open_cands(); // 這裡測的是候選字的移動，不是清單的開關
        s.next_cand();
        let want = s.char_candidates()[1].clone();
        assert!(!s.confirm_cand(), "還有下一格，不該離開選字");
        assert_eq!(s.slots()[0].text, want, "該選中反白的那個");
        assert_eq!(s.select_index(), Some(1), "選完往右移一格");
    }

    #[test]
    fn 選完最後一格就離開選字狀態() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        // 第一格：還有下一格
        assert!(!s.confirm_cand());
        assert_eq!(s.select_index(), Some(1));
        // 最後一格：選完就結束，不能卡在原地讓使用者按了沒反應
        assert!(s.confirm_cand(), "最後一格該回報已離開");
        assert_eq!(s.select_index(), None, "要退出選字模式");
    }

    #[test]
    fn 手動選的字不會被繼續打字覆蓋() {
        if !load() {
            return;
        }
        // 使用者要求：手動選字過的要保留，不可以被重算覆蓋掉
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.pick_char("妳");
        assert_eq!(s.slots()[0].text, "妳");
        // 繼續打字——格子會整個重建
        s.push('m');
        s.push('4');
        assert_eq!(s.slots()[0].text, "妳", "手動選的字不能被重算蓋掉");
    }

    #[test]
    fn 選字之後繼續打字不會換掉切法() {
        if !load() {
            return;
        }
        // 使用者回報：打 ! 會觸發重算，蓋掉選過的字。
        // 病因是每打一鍵 cutting_idx 就歸零跳回第一名——
        // 手動選的字還在，但它們屬於的分段被換掉了。
        let mut s = typed("rup wu0 5p 2k7");
        // 固定半形，這個測試驗的是切法保留，不該被全半形轉換干擾
        s.set_width(crate::width::Width::Half);
        s.enter_select_first();
        s.select_right();
        let want = s.char_candidates()[2].clone();
        s.pick_char(&want);
        let before = s.text();
        s.push('!');
        assert_eq!(s.text(), format!("{before}!"), "多打一個符號不該把前面重切");
    }

    #[test]
    fn 翻過切法之後繼續打字也保留() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.set_width(crate::width::Width::Half);
        s.next_cutting();
        let before = s.text();
        s.push('!');
        assert_eq!(
            s.text(),
            format!("{before}!"),
            "使用者挑過的切法不該被重排掉"
        );
    }

    #[test]
    fn 手動選的字換切法也保留() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.pick_char("妳");
        s.next_cutting();
        s.prev_cutting();
        assert_eq!(s.slots()[0].text, "妳", "換切法轉一圈回來還要在");
    }

    /// **選一個字要連動修好整個詞**。
    ///
    /// 「城市」與「程式」讀音完全相同（ㄔㄥˊㄕˋ）。詞層舊版一鍵只留
    /// 一個詞，分數低的「程式」在建表時就被丟掉——於是選了「程」，
    /// 「市」也無從變成「式」（實測是「程市」）。
    #[test]
    fn 選一個字要連動修好整個詞() {
        if !load() {
            return;
        }
        let mut s = typed("t/6g4");
        assert_eq!(s.text(), "城市", "預設仍是最常用的那個");
        s.enter_select_first();
        s.pick_char("程");
        assert_eq!(s.text(), "程式", "選了「程」，「市」要跟著變「式」");

        // 一個讀音底下不只兩個詞：`u4g4`（ㄧˋㄕˋ）有 14 個
        let mut s2 = typed("u4g4");
        assert_eq!(s2.text(), "意識");
        s2.enter_select_first();
        s2.pick_char("議");
        assert_eq!(s2.text(), "議事", "跳到另一個同音詞");
    }

    #[test]
    fn 手動選的字不被詞庫改掉() {
        if !load() {
            return;
        }
        // 「你好」是詞，但使用者選了「妳」就該留著「妳」
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.pick_char("妳");
        assert_eq!(s.slots()[0].text, "妳");
        assert!(s.slots()[0].picked, "該標記為手動選過");
        assert!(!s.slots()[1].picked, "沒選過的不該被標記");
    }

    #[test]
    fn 退格重算也保留手動選字() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3m4");
        s.enter_select_first();
        s.pick_char("妳");
        s.backspace();
        assert_eq!(s.slots()[0].text, "妳", "退格重算後還在");
    }

    #[test]
    fn default_建的_session_也能打字() {
        if !load() {
            return;
        }
        // TSF 那層的 `State` 是 `derive(Default)` 建的，所以 `Session`
        // 走的是 `Default` 而不是 `new()`。兩者必須等價——
        // 不然重新載入 DLL 之後第一次打字會完全沒有切法。
        let mut s = Session::default();
        for c in "su3cl3".chars() {
            s.push(c);
        }
        assert!(s.cutting_count() > 0, "Default 建的也要生得出切法");
        assert_eq!(s.text(), "你好");
    }

    #[test]
    fn 設定成選完就退出時不往下一格() {
        if !load() {
            return;
        }
        // behavior.enter_in_select = "exit"（微軟注音式）
        let mut s = typed("su3cl3");
        s.enter_select_first();
        assert!(s.confirm_cand_with(false), "該直接離開選字");
        assert_eq!(s.select_index(), None);
    }

    #[test]
    fn 展開全部候選() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        assert!(!s.cand_expanded(), "一開始不展開");
        assert_eq!(s.cand_columns(), 1, "沒展開就是一欄");

        s.expand_cands();
        assert!(s.cand_expanded());
        // su3 有 29 個同音字，每欄 10 個 → 3 欄
        let n = s.char_candidates().len();
        assert!(n > CHAR_COLUMN, "該有超過一欄的候選：{n}");
        assert_eq!(s.cand_columns(), n.div_ceil(CHAR_COLUMN).min(MAX_COLUMNS));
    }

    #[test]
    fn 展開後上下只在同欄內走() {
        if !load() {
            return;
        }
        // 使用者定的：上下同欄移動、左右換欄
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.open_cands(); // 這裡測的是候選字的移動，不是清單的開關
        s.expand_cands();
        // 從第 0 個往上 → 繞回同一欄的底（第 9 個），不是跑到別欄
        s.prev_cand();
        assert_eq!(s.cand_index(), CHAR_COLUMN - 1, "該繞回同欄底端");
        // 再往下一個就回到欄頂
        s.next_cand();
        assert_eq!(s.cand_index(), 0);
    }

    #[test]
    fn 展開後左右換欄() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.expand_cands();
        s.cand_right_column();
        assert_eq!(s.cand_index(), CHAR_COLUMN, "往右跳一整欄");
        s.cand_left_column();
        assert_eq!(s.cand_index(), 0, "往左跳回來");
        // 到最左邊不該再往左
        s.cand_left_column();
        assert_eq!(s.cand_index(), 0);
    }

    #[test]
    fn 按到第九個再往下就自動展開() {
        if !load() {
            return;
        }
        // 九個裡沒有想要的字，再按一次下鍵＝「還要看更多」
        let mut s = typed("u4");
        s.enter_select_first();
        s.open_cands();
        for _ in 0..CHAR_PAGE - 1 {
            s.next_cand();
        }
        assert_eq!(s.cand_index(), CHAR_PAGE - 1, "先走到第九個");
        assert!(!s.cand_expanded(), "還沒展開");

        s.next_cand();
        assert!(s.cand_expanded(), "再按一次就展開");
        assert_eq!(s.cand_index(), CHAR_COLUMN, "跳到第二欄的第一個");
    }

    #[test]
    fn 候選不到十個就不自動展開() {
        if !load() {
            return;
        }
        // ㄏㄠˇ 只有四個字，沒有第二欄可以跳
        let mut s = typed("cl3");
        s.enter_select_first();
        s.open_cands();
        let n = s.char_candidates().len();
        assert!(n <= CHAR_PAGE, "前提：候選不到一欄（{n}）");
        for _ in 0..n {
            s.next_cand();
        }
        assert!(!s.cand_expanded(), "沒有更多候選就不該展開");
        assert_eq!(s.cand_index(), 0, "繞回第一個");
    }

    #[test]
    fn 展開最多十欄超過就捲動() {
        if !load() {
            return;
        }
        // ㄧˋ 有三百多個同音字，全部攤開是三十幾欄——橫向長度超過任何
        // 螢幕，所以只畫十欄，反白頂到右邊時整片跟著捲
        let mut s = typed("u4");
        s.enter_select_first();
        s.open_cands();
        s.expand_cands();
        let n = s.char_candidates().len();
        assert!(n > MAX_COLUMNS * CHAR_COLUMN, "ㄧˋ 該有超過十欄的候選：{n}");
        assert_eq!(s.cand_columns(), MAX_COLUMNS, "最多就是十欄");
        assert_eq!(s.cand_visible_range(), 0..MAX_COLUMNS * CHAR_COLUMN);

        // 走到第十欄還在可見範圍內，不該捲
        for _ in 0..MAX_COLUMNS - 1 {
            s.cand_right_column();
        }
        assert_eq!(s.cand_visible_range().start, 0, "還看得到就不捲");
        assert_eq!(
            s.cand_index_in_view(),
            Some((MAX_COLUMNS - 1) * CHAR_COLUMN)
        );

        // 再往右一欄：整片推一欄，反白仍停在最右欄
        s.cand_right_column();
        assert_eq!(s.cand_visible_range().start, CHAR_COLUMN, "只推一欄");
        assert_eq!(
            s.cand_index_in_view(),
            Some((MAX_COLUMNS - 1) * CHAR_COLUMN),
            "反白在畫面最右欄"
        );
        assert_eq!(s.cand_columns(), MAX_COLUMNS, "捲動後仍是十欄");

        // 往左走回頭，可見範圍也要跟著捲回去
        for _ in 0..MAX_COLUMNS {
            s.cand_left_column();
        }
        assert_eq!(s.cand_visible_range().start, 0, "捲回開頭");
        assert_eq!(s.cand_index(), 0);
    }

    #[test]
    fn 拖捲軸只換可見範圍不動反白() {
        if !load() {
            return;
        }
        let mut s = typed("u4");
        s.enter_select_first();
        s.open_cands();
        s.expand_cands();
        let total = s.char_candidates().len().div_ceil(CHAR_COLUMN);
        assert_eq!(s.cand_scroll(), Some((0, total)), "十欄裝不下才有捲軸");

        s.set_cand_col_first(5);
        assert_eq!(s.cand_visible_range().start, 5 * CHAR_COLUMN);
        assert_eq!(s.cand_index(), 0, "反白不跟著跑");
        assert_eq!(s.cand_index_in_view(), None, "捲到看不見它就不該反白");

        // 拖過頭要夾住，不能捲出一片空白
        s.set_cand_col_first(9999);
        assert_eq!(
            s.cand_visible_range().start,
            (total - MAX_COLUMNS) * CHAR_COLUMN
        );
        assert_eq!(s.cand_visible_range().end, s.char_candidates().len());
    }

    #[test]
    fn 十欄以內不給捲軸() {
        if !load() {
            return;
        }
        // ㄏㄠˇ 只有四個字，展開也只有一欄
        let mut s = typed("cl3");
        s.enter_select_first();
        s.open_cands();
        s.expand_cands();
        assert_eq!(s.cand_scroll(), None, "全部看得到就不該畫捲軸");
        // 沒展開時也沒有
        s.collapse_cands();
        assert_eq!(s.cand_scroll(), None);
    }

    #[test]
    fn 數字鍵認的是目前那一欄() {
        if !load() {
            return;
        }
        let mut s = typed("u4");
        s.enter_select_first();
        s.open_cands();
        // 沒展開時就是直接對應
        assert_eq!(s.cand_number_index(0), Some(0));
        assert_eq!(s.cand_number_index(8), Some(8));

        s.expand_cands();
        s.cand_right_column();
        // 第二欄的「1」是第 10 個候選，不是第 1 個
        assert_eq!(s.cand_number_index(0), Some(CHAR_COLUMN));
        assert_eq!(s.cand_number_index(8), Some(CHAR_COLUMN * 2 - 1));
    }

    #[test]
    fn 滑鼠點的是畫面上的位置() {
        if !load() {
            return;
        }
        let mut s = typed("u4");
        s.enter_select_first();
        s.open_cands();
        s.expand_cands();
        // 捲到第二欄起頭
        for _ in 0..MAX_COLUMNS {
            s.cand_right_column();
        }
        let start = s.cand_visible_range().start;
        assert_eq!(start, CHAR_COLUMN, "前提：已經捲過一欄");
        // 點畫面上第一個，選到的是那一欄的第一個而不是整份的第一個
        s.set_cand_index(0);
        assert_eq!(s.cand_index(), start);
        // 畫面外的位置點不到
        let out = s.cand_visible_range().len();
        s.set_cand_index(out);
        assert_eq!(s.cand_index(), start, "超出畫面就忽略");
    }

    #[test]
    fn 收回展開時反白要夾回範圍() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.expand_cands();
        s.cand_right_column();
        s.cand_right_column();
        assert!(s.cand_index() >= CHAR_PAGE, "先跑到看不見的位置");
        s.collapse_cands();
        assert!(!s.cand_expanded());
        assert!(s.cand_index() < CHAR_PAGE, "收回後反白不能指在看不見的地方");
    }

    #[test]
    fn 上下不會跑出可見範圍() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.open_cands();
        // 展開之前：一般狀態只列 CHAR_PAGE 個
        for _ in 0..CHAR_PAGE - 1 {
            s.next_cand();
            assert!(!s.cand_expanded(), "還沒到底就不該展開");
            assert!(s.cand_index() < CHAR_PAGE, "不能跑出可見範圍");
        }
        // 到底再按一次會自動展開（見「按到第九個再往下就自動展開」），
        // 之後上下只在同一欄內繞，一樣不會跑到畫面外
        for _ in 0..30 {
            s.next_cand();
            let view = s.cand_visible_range();
            assert!(view.contains(&s.cand_index()), "不能跑出可見範圍");
        }
    }

    #[test]
    fn 換格會收回展開狀態() {
        if !load() {
            return;
        }
        let mut s = typed("su3cl3");
        s.enter_select_first();
        s.expand_cands();
        s.select_right();
        assert!(!s.cand_expanded(), "換格是新的一輪，不該還展開著");
    }

    #[test]
    fn 空的_session() {
        let s = Session::new();
        assert!(s.is_empty());
        assert_eq!(s.text(), "");
        assert_eq!(s.cutting_count(), 0);
        assert_eq!(s.select_index(), None);
    }
}

#[cfg(test)]
mod 鎖定 {
    use crate::language::Language;
    use crate::session::*;

    fn keys(s: &mut Session, text: &str) {
        for c in text.chars() {
            s.push(c);
        }
    }

    #[test]
    fn 鎖定注音時英文按鍵也照注音解讀() {
        // 這是「跟一般輸入法相同」的核心：鎖定之後不再猜語言。
        // 新注音打 hello 就是照注音鍵解讀，不會變回英文。
        let mut s = Session::new();
        s.set_lock(Some(Language::Bopomofo));
        keys(&mut s, "hello");
        let langs: Vec<_> = s.slots().iter().map(|x| x.lang).collect();
        assert!(
            langs.iter().all(|&l| l == Language::Bopomofo),
            "鎖定注音時不該出現別的語言：{langs:?}"
        );
    }

    #[test]
    fn 鎖定英文時注音按鍵原樣輸出() {
        let mut s = Session::new();
        s.set_lock(Some(Language::English));
        keys(&mut s, "su3cl3");
        assert_eq!(s.text(), "su3cl3", "鎖定英文就是 passthrough");
    }

    #[test]
    fn 鎖定時只有一種切法() {
        // 沒有「哪裡換語言」的問題，切法選單就沒東西可選
        let mut s = Session::new();
        s.set_lock(Some(Language::Bopomofo));
        keys(&mut s, "su3cl3");
        assert_eq!(s.cutting_menu(10).len(), 1, "鎖定時切法只該有一種");
    }

    #[test]
    fn 自動模式仍會分語言() {
        // 鎖定是選配，不鎖時原本的辨識照常運作
        let mut s = Session::new();
        keys(&mut s, "su3cl3");
        let langs: Vec<_> = s.slots().iter().map(|x| x.lang).collect();
        assert!(
            langs.contains(&Language::Bopomofo),
            "自動模式該認得注音：{langs:?}"
        );
    }

    #[test]
    fn 關掉的語言連自動辨識都跳過() {
        // 不打日文的人關掉它之後，sushi 就穩定判成英文
        let mut s = Session::new();
        s.set_engines(crate::config::Engines {
            bopomofo: true,
            romaji: false,
        });
        keys(&mut s, "sushi");
        let langs: Vec<_> = s.slots().iter().map(|x| x.lang).collect();
        assert!(
            !langs.contains(&Language::Romaji),
            "日文關掉了不該出現：{langs:?}"
        );
    }

    #[test]
    fn 關掉日文不影響注音() {
        let mut s = Session::new();
        s.set_engines(crate::config::Engines {
            bopomofo: true,
            romaji: false,
        });
        keys(&mut s, "su3");
        let langs: Vec<_> = s.slots().iter().map(|x| x.lang).collect();
        assert!(langs.contains(&Language::Bopomofo), "{langs:?}");
    }

    #[test]
    fn 全部關掉還是能打英文() {
        // **不能讓輸入法完全不能用**——英文是 passthrough，永遠接得住
        let mut s = Session::new();
        s.set_engines(crate::config::Engines {
            bopomofo: false,
            romaji: false,
        });
        keys(&mut s, "sushi");
        assert_eq!(s.text(), "sushi");
    }

    #[test]
    fn 輪替跳過關掉的語言() {
        let mut s = Session::new();
        s.set_engines(crate::config::Engines {
            bopomofo: true,
            romaji: false,
        });
        s.cycle_lock();
        assert_eq!(s.lock(), Some(Language::Bopomofo));
        s.cycle_lock();
        assert_eq!(s.lock(), Some(Language::English), "該跳過日文");
        s.cycle_lock();
        assert_eq!(s.lock(), None);
    }

    #[test]
    fn 關掉正在鎖定的語言就退回自動() {
        let mut s = Session::new();
        s.set_lock(Some(Language::Romaji));
        s.set_engines(crate::config::Engines {
            bopomofo: true,
            romaji: false,
        });
        assert_eq!(s.lock(), None, "鎖定的語言被關掉該退回自動");
    }

    #[test]
    fn 送出之後鎖定要保留() {
        // 打完一句話送出去，下一句還在同一個模式裡——
        // 鎖定是「對輸入法的設定」，不是這一次輸入的一部分
        let mut s = Session::new();
        s.set_lock(Some(Language::Bopomofo));
        keys(&mut s, "su3");
        s.clear(); // 送出／取消都走這裡
        assert_eq!(s.lock(), Some(Language::Bopomofo), "鎖定該留著");
        assert!(s.is_empty(), "但按鍵要清掉");
    }

    #[test]
    fn 送出之後全半形也要保留() {
        let mut s = Session::new();
        s.set_width(crate::width::Width::Full);
        keys(&mut s, "su3");
        s.clear();
        assert_eq!(s.width(), crate::width::Width::Full);
    }

    #[test]
    fn 送出之後注音模式仍走音節緩衝() {
        // `clear` 重建輸入層時要配合鎖定的語言，
        // 不然會退回預設的 Cascade，同格覆寫就失效了
        let mut s = Session::new();
        s.set_lock(Some(Language::Bopomofo));
        keys(&mut s, "su3");
        s.clear();
        s.push('1'); // ㄅ
        s.push('q'); // ㄆ
        assert_eq!(s.composition_text(), "ㄆ", "送出後仍該同格覆寫");
    }

    #[test]
    fn 退出選字不影響鎖定() {
        let mut s = Session::new();
        s.set_lock(Some(Language::Bopomofo));
        keys(&mut s, "su3");
        s.enter_select_first();
        s.exit_select();
        assert_eq!(s.lock(), Some(Language::Bopomofo));
    }

    #[test]
    fn 輪替走完一圈回到自動() {
        // Ctrl+空白 一直按下去的行為，跟全半形的三態輪替同一套
        let mut s = Session::new();
        assert_eq!(s.lock(), None, "預設是自動");
        s.cycle_lock();
        assert_eq!(s.lock(), Some(Language::Bopomofo));
        s.cycle_lock();
        assert_eq!(s.lock(), Some(Language::Romaji));
        s.cycle_lock();
        assert_eq!(s.lock(), Some(Language::English));
        s.cycle_lock();
        assert_eq!(s.lock(), None, "走完一圈該回到自動");
    }

    #[test]
    fn 輪替時已打的字跟著重算() {
        // 打到一半才輪到英文，前面打的也要跟著變
        let mut s = Session::new();
        keys(&mut s, "su3");
        s.cycle_lock(); // 注音
        s.cycle_lock(); // 日文
        s.cycle_lock(); // 英文
        assert_eq!(s.text(), "su3", "鎖定英文後該原樣顯示");
    }

    #[test]
    fn 打字中途鎖定會重算目前的字() {
        // 使用者打到一半發現辨識錯了才按鎖定鍵——已經打的也要跟著改
        let mut s = Session::new();
        keys(&mut s, "sushi");
        s.set_lock(Some(Language::English));
        assert_eq!(s.text(), "sushi", "鎖定英文後該原樣顯示");
    }

    #[test]
    fn 標點在鎖定時仍自成一段() {
        // 標點不屬於任何語言，鎖定注音也不該把它拿去查詞庫。
        //
        // **用 `!` 而不是逗號**——逗號在注音鍵盤上是ㄝ，鎖定注音時
        // 它就是注音鍵。只有注音鍵盤上沒有的符號才一定是標點。
        let mut s = Session::new();
        s.set_lock(Some(Language::Bopomofo));
        keys(&mut s, "su3!");
        let marks = s.slots().iter().filter(|x| !x.selectable).count();
        assert!(marks >= 1, "驚嘆號該是不可選字的獨立一段");
    }

    #[test]
    fn 解鎖之後回到自動辨識() {
        let mut s = Session::new();
        s.set_lock(Some(Language::English));
        keys(&mut s, "su3");
        assert_eq!(s.text(), "su3");
        s.set_lock(None);
        let langs: Vec<_> = s.slots().iter().map(|x| x.lang).collect();
        assert!(
            langs.contains(&Language::Bopomofo),
            "解鎖後該重新認出注音：{langs:?}"
        );
    }
}

#[cfg(test)]
mod 注音鎖定 {
    use crate::language::Language;
    use crate::session::*;

    fn keys(s: &mut Session, text: &str) {
        for c in text.chars() {
            s.push(c);
        }
    }

    fn locked() -> Session {
        let mut s = Session::new();
        s.set_lock(Some(Language::Bopomofo));
        s
    }

    #[test]
    fn 打到一半顯示注音符號() {
        // 新酷音打 su 組字區顯示「ㄋㄧ」，不是 su
        let mut s = locked();
        keys(&mut s, "su");
        assert_eq!(s.composition_text(), "ㄋㄧ");
    }

    #[test]
    fn 同格覆寫() {
        // 打錯聲母直接打正確的就換掉，不必按 Backspace
        let mut s = locked();
        s.push('1'); // ㄅ
        assert_eq!(s.composition_text(), "ㄅ");
        s.push('q'); // ㄆ
        assert_eq!(s.composition_text(), "ㄆ", "同是聲母該覆寫");
    }

    #[test]
    fn 聲調收尾之後變成字() {
        let mut s = locked();
        keys(&mut s, "su3");
        assert!(s.pending_symbols().is_empty(), "音節該結算掉了");
        assert_eq!(s.keys(), "su3");
    }

    #[test]
    fn 已完成的字加正在打的音節() {
        // 組字區＝已完成的字 + 還沒收尾的注音
        let mut s = locked();
        keys(&mut s, "su3"); // 一個完整音節
        keys(&mut s, "cl"); // 第二個還沒打完
        let t = s.composition_text();
        assert!(t.ends_with("ㄏㄠ"), "尾巴該是正在打的注音：{t}");
        assert!(t.chars().count() > 2, "前面該有已完成的字：{t}");
    }

    #[test]
    fn backspace_先刪正在打的音節() {
        let mut s = locked();
        keys(&mut s, "su");
        s.backspace();
        assert_eq!(s.composition_text(), "ㄋ");
        s.backspace();
        assert!(s.is_empty());
    }

    #[test]
    fn 自動模式仍顯示原始按鍵() {
        // 鎖定是選配，不鎖時維持原本行為——那時還不知道是注音還是英文
        let mut s = Session::new();
        keys(&mut s, "su3");
        assert_eq!(s.composition_text(), "su3");
    }

    #[test]
    fn 解鎖時未完成的音節要結算掉() {
        // 不然那些按鍵會卡在緩衝裡，看得到卻送不出去
        let mut s = locked();
        keys(&mut s, "su");
        s.set_lock(None);
        assert!(s.pending_symbols().is_empty());
        assert_eq!(s.keys(), "su", "按鍵該接回主串");
    }

    #[test]
    fn 未完成的音節也算有內容() {
        // is_empty 要把緩衝算進去，否則 Esc/送出的判斷會出錯
        let mut s = locked();
        s.push('s');
        assert!(!s.is_empty(), "打了字就不是空的");
    }
}
