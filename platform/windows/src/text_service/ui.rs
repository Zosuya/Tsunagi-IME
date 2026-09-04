//! 把 session 的狀態組裝成候選視窗要顯示的東西。
//!
//! 這一層只做「組裝」——顏色、版面、動畫都在 `candidate_window` 與
//! `theme` 那邊。分開的理由是這裡要碰 TSF（問插入點位置），那邊完全
//! 不必知道 TSF 的存在。

use super::*;

pub(crate) fn show_candidates(context: &ITfContext, state: &mut State) -> Result<()> {
    if state.session.is_empty() {
        return Ok(());
    }
    // 預覽列一律顯示「目前會送出的文字」，候選清單則看模式：
    //
    //   打字中      不列（只有預覽）
    //   切法選單    列前 N 種切法
    //   選字中      列反白那一格的同音字
    // 選字時預覽列要標出**正在選哪一格**，不然使用者看不出反白在哪。
    // 組字區顯示的是原始按鍵（打什麼顯示什麼），框不上去，
    // 所以標記畫在預覽列。
    //
    // 標記方式是**畫外框**，不是把那一段用【】包起來——預覽列的
    // 用意是「送出去會長這樣」，混進不會送出的符號就不誠實了，
    // 而且中文全形括號很佔位置。這裡只算出「要框哪一段」（位元組
    // 範圍），框怎麼畫是候選視窗的事。
    // **用 `marked_index` 不是 `select_index`**：選完離開選字之後，
    // 那一格的框要留著讓使用者看到自己剛改了哪個字。候選清單那邊
    // 仍然看 `select_index`——離開了就不該再列候選。
    let (preview, preview_box) = match state.session.marked_index() {
        Some(i) => {
            let mut text = String::new();
            let mut range = None;
            for (k, s) in state.session.slots().iter().enumerate() {
                if k == i {
                    range = Some(text.len()..text.len() + s.text.len());
                }
                text.push_str(&s.text);
            }
            (text, range)
        }
        None => (state.session.text(), None),
    };
    // 反白哪一列：切法選單反白目前選到的切法，選字模式反白第一個候選字。
    let selected: Option<usize> = if state.session.select_index().is_some() {
        // **交出去的是畫面上的位置**：展開捲動之後絕對索引跟畫面對不起來
        state
            .session
            .cands_open()
            .then(|| state.session.cand_index_in_view())
            .flatten()
    } else if state.cutting_menu {
        // 反白條只在看得到的範圍內——選到第 12 個但只列 10 個時不反白
        Some(state.session.cutting_index()).filter(|&i| i < state.cutting_shown)
    } else {
        None
    };
    // **框出來不等於要列候選**：左右鍵只是把框移過去看看，
    // 清單要按下鍵才叫出來，見 `Session::cands_open`
    let candidates: Vec<Candidate> =
        if state.session.select_index().is_some() && state.session.cands_open() {
            // **看得到的那一段**：一般狀態是前 CHAR_PAGE 個，展開時是
            // 目前捲到的十欄——候選可以有幾百個，全部攤開會橫向長出螢幕
            let view = state.session.cand_visible_range();
            let mut all = state.session.char_candidates();
            all.truncate(view.end);
            all.drain(..view.start);
            all.into_iter()
                .map(|text| Candidate {
                    text,
                    label: "char",
                })
                .collect()
        } else if state.cutting_menu {
            state
                .session
                .cutting_menu(state.cutting_shown)
                .into_iter()
                .map(|text| Candidate { text, label: "cut" })
                .collect()
        } else {
            Vec::new()
        };

    // **指令做成提示而不是候選項**。
    //
    // 放進候選清單的話，打 `config` 這個英文單字時每次都會多一個
    // 不想選的東西。提示只是告訴你有這條路，不干擾選字。
    // 底部提示：指令優先，其次顯示全半形模式。
    //
    // 只在**不是自動模式**時顯示全半形——自動是預設，一直掛著
    // 反而是雜訊；使用者手動切過才需要知道自己在哪個模式。
    // 鎖定狀態要一直看得見——使用者得知道「現在打什麼都算注音」。
    // 自動模式是預設，不顯示（一直掛著反而是雜訊，跟全半形同理）。
    let lock_hint = state.session.lock().map(|l| format!("鎖定：{}", l.short()));
    // **關掉的引擎要一直看得見**——那是持久的設定，而且會讓某些字
    // 突然打不出來。不提示的話使用者只會覺得「怎麼壞了」。
    //
    // 全開是預設，不顯示（一直掛著反而是雜訊，跟鎖定、全半形同理）。
    let off_hint = {
        let e = state.config.behavior.engines;
        let off: Vec<&str> = [(e.bopomofo, "注音"), (e.romaji, "日文")]
            .iter()
            .filter(|(on, _)| !on)
            .map(|(_, name)| *name)
            .collect();
        (!off.is_empty()).then(|| format!("{}已停用", off.join("、")))
    };
    let hint = match ime_core::command::match_keys(state.session.keys()) {
        Some(cmd) => format!("↑↑↓↓ {}", cmd.label(state.config.behavior.engines)),
        // 兩種狀態可能同時成立（鎖定注音、又關掉日文），都要講
        None => [lock_hint, off_hint]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("　"),
    };
    // 展開全部時分欄，每欄 CHAR_COLUMN 個；其餘是一直排
    let per_column = if state.session.cand_expanded() {
        ime_core::session::CHAR_COLUMN
    } else {
        0
    };
    state.candidates = candidates.clone();

    let anchor = caret_anchor(context, state);

    // **把現有視窗交出去沿用**，不要銷毀重建。
    //
    // 每按一鍵砍掉重建的話，中間那一瞬間畫面上什麼都沒有，
    // 使用者看到的就是閃爍。
    let existing = state.candidate_window.take();
    state.candidate_window = Some(CandidateWindow::show(
        existing,
        &candidates,
        &preview,
        preview_box,
        &hint,
        selected,
        per_column,
        state.session.cand_scroll(),
        anchor,
    )?);
    Ok(())
}

/// 叫出候選視窗，位置跟著組字文字跑。
///
/// 位置要向 TSF 問（`ITfContextView::GetTextExt`），而那需要 edit cookie，
/// 所以得包在 edit session 裡。不能靠 Win32 API 去猜滑鼠或前景視窗的
/// caret——文字區可能在子視窗、另一個行程（瀏覽器）或自繪畫布（Electron）。
/// 顯示全半形切換的提示視窗（含滑動動畫）。
///
/// 它是**獨立於候選視窗**的另一個視窗，疊在候選上方。分開的理由
/// 見 `crate::width_window`。
pub(crate) fn show_width_window(
    context: &ITfContext,
    state: &mut State,
    from: ime_core::width::Width,
    to: ime_core::width::Width,
) -> Result<()> {
    // 錨點：組字中就用候選視窗的位置，沒組字就用退路
    let anchor = caret_anchor(context, state);
    // 提示視窗只需要一個點——用組字文字的左下角，跟以前一致
    let anchor_pt = POINT {
        x: anchor.left,
        y: anchor.bottom,
    };
    let theme = crate::theme::Theme::from_config(&state.config);
    let existing = state.width_window.take();
    state.width_window = Some(crate::width_window::WidthWindow::show(
        existing,
        &theme,
        from,
        to,
        anchor_pt,
        crate::candidate_window::dpi_at(anchor_pt),
    )?);
    Ok(())
}

/// 語言模式的提示視窗（自／注／日／英）。
///
/// **跟全半形共用同一個視窗實作**——動畫、版面、淡出都一樣，
/// 只有格數與標籤不同。
pub(crate) fn show_lang_window(
    context: &ITfContext,
    state: &mut State,
    from: Option<ime_core::language::Language>,
    to: Option<ime_core::language::Language>,
) -> Result<()> {
    use crate::width_bar::{lang_index_in, lang_options, lang_symbol};
    // **只畫還開著的語言**——設定裡關掉的引擎輪替時會跳過，
    // 提示列上留著那格會變成永遠輪不到的死格子。
    let options = lang_options(&state.session.engines());
    let labels: Vec<&'static str> = options.iter().map(|&o| lang_symbol(o)).collect();
    let anchor = caret_anchor(context, state);
    // 提示視窗只需要一個點——用組字文字的左下角，跟以前一致
    let anchor_pt = POINT {
        x: anchor.left,
        y: anchor.bottom,
    };
    let theme = crate::theme::Theme::from_config(&state.config);
    let existing = state.width_window.take();
    state.width_window = Some(crate::width_window::WidthWindow::show_bar(
        existing,
        &theme,
        labels,
        lang_index_in(&options, from),
        lang_index_in(&options, to),
        anchor_pt,
        crate::candidate_window::dpi_at(anchor_pt),
    )?);
    Ok(())
}

/// 使用者把捲軸拖到「可見的第一欄是第 `first` 欄」。
///
/// **只換可見範圍，不動反白也不選字**——拖捲軸是「我看看別欄有什麼」。
pub(crate) fn on_candidate_scrolled(
    shared: &std::sync::Arc<std::sync::Mutex<State>>,
    first: usize,
) {
    let mut state = super::lock_state(shared);
    let Some(context) = focused_context(&state) else {
        return;
    };
    state.session.set_cand_col_first(first);
    let _ = show_candidates(&context, &mut state);
}

/// 使用者用滑鼠點了第 `i` 個候選。
///
/// 行為跟鍵盤一致：切法選單就選那一種切法並關閉選單，選字模式就選那個
/// 字並確認——等同把反白移過去再按 Enter。
pub(crate) fn on_candidate_picked(shared: &std::sync::Arc<std::sync::Mutex<State>>, i: usize) {
    use ime_core::config::EnterInSelect;
    let mut state = super::lock_state(shared);
    let Some(context) = focused_context(&state) else {
        return;
    };
    match state.mode() {
        Mode::CuttingMenu => {
            state.session.set_cutting_index(i);
            state.cutting_menu = false;
            // 同鍵盤那條路：關掉選單就要清掉雙擊判定，
            // 不然下一次按 TAB 會被誤判成「雙擊展開全部」
            state.last_tab = None;
            let _ = rewrite_composition(&context, &mut state);
            let _ = show_candidates(&context, &mut state);
        }
        Mode::Selecting | Mode::SelectingExpanded => {
            state.session.set_cand_index(i);
            let advance = state.config.behavior.enter_in_select == EnterInSelect::Next;
            let left_select = state.session.confirm_cand_with(advance);
            if left_select && state.config.behavior.commit_on_last {
                let text = state.session.text();
                let _ = end_composition(&context, &mut state, EndKind::Commit(&text));
            } else {
                let _ = rewrite_composition(&context, &mut state);
                let _ = show_candidates(&context, &mut state);
            }
        }
        // 打字中或閒置時視窗上沒有可點的候選，點到也不該有事
        _ => {}
    }
}
