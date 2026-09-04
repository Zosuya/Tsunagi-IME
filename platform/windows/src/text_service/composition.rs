//! 組字區的增刪改：把 session 的狀態寫進宿主的文件。
//!
//! # 兩個踩過的坑
//!
//! **`EndComposition` 不會刪掉文字**——它的語意是「結束組字狀態」，
//! 不是「丟棄內容」。取消時要先 `SetText` 成空字串再結束，見 `EndKind`。
//!
//! **`GetRange()` 的端點在 `SetText` 之後不會延展**，它還記著舊邊界。
//! 拿過時的座標去 `SetSelection`，HRESULT 回 `S_OK` 但位置是錯的。
//! 所以 `SetText` 之後一律重新 `GetRange()`。

use super::*;

/// 有既有 composition 就直接改內容；沒有就新開一個並掛上 composition sink。
pub(crate) fn update_composition(
    service: &TextService_Impl,
    context: &ITfContext,
    state: &mut State,
) -> Result<()> {
    let tid = state.client_id;
    // **組字區顯示什麼由 core 決定**——自動模式是原始按鍵，
    // 鎖定注音是「已完成的字＋正在打的注音符號」。見 `composition_text`。
    let text_utf16: Vec<u16> = state.session.composition_text().encode_utf16().collect();

    if state.composition.is_some() {
        rewrite_composition(context, state)?;
    } else {
        let sink: ITfCompositionSink =
            (*ComObjectInterface::<ITfCompositionSink>::as_interface_ref(service)).clone();
        let result: Rc<RefCell<Option<ITfComposition>>> = Rc::new(RefCell::new(None));
        let result_slot = result.clone();
        let context_owned = context.clone();

        let session = EditSession::new(move |ec| unsafe {
            let insert_at_sel: ITfInsertAtSelection = context_owned.cast()?;
            let range = insert_at_sel.InsertTextAtSelection(ec, Default::default(), &text_utf16)?;
            let composition_services: ITfContextComposition = context_owned.cast()?;
            let composition = composition_services.StartComposition(ec, &range, &sink)?;
            // 套上「組字中」樣式，宿主才會畫底線。
            apply_display_attribute(&context_owned, ec, &range)?;
            sync_caret_to_composition_end(&context_owned, &composition, ec)?;
            *result_slot.borrow_mut() = Some(composition);
            Ok(())
        });
        let session_interface: ITfEditSession = session.into();
        unsafe {
            let _ = context.RequestEditSession(
                tid,
                &session_interface,
                TF_ES_READWRITE | TF_ES_SYNC,
            )?;
        }

        let composition = result
            .borrow_mut()
            .take()
            .ok_or_else(|| Error::from(E_FAIL))?;
        state.composition = Some(composition);
    }
    Ok(())
}

/// 把**既有**組字區的文字換成目前該顯示的內容。
///
/// 從 `update_composition` 抽出來共用——滑鼠選字那條路也要改寫組字，
/// 但它沒有 `TextService_Impl` 可用（見 `on_candidate_picked`）。而
/// `service` 只有「還沒有組字、要新開一個」時才需要（拿 composition
/// sink），既有組字這條路根本用不到。
pub(crate) fn rewrite_composition(context: &ITfContext, state: &mut State) -> Result<()> {
    let tid = state.client_id;
    let Some(composition) = state.composition.clone() else {
        return Ok(());
    };
    let text_utf16: Vec<u16> = state.session.composition_text().encode_utf16().collect();
    let context_owned2 = context.clone();
    let session = EditSession::new(move |ec| unsafe {
        let range = composition.GetRange()?;
        range.SetText(ec, 0, &text_utf16)?;
        // 文字換過了，樣式要重套一次，不然新增的那段沒有底線。
        apply_display_attribute(&context_owned2, ec, &range)?;
        // 游標要跟著打到的位置跑。這裡不能只用 `range.Collapse()`，
        // 那只會調整 range 物件本身，宿主的游標不會動。
        sync_caret_to_composition_end(&context_owned2, &composition, ec)?;
        Ok(())
    });
    let session_interface: ITfEditSession = session.into();
    unsafe {
        let _ =
            context.RequestEditSession(tid, &session_interface, TF_ES_READWRITE | TF_ES_SYNC)?;
    }
    Ok(())
}

/// 直接把一個字送進文件，**不開組字**。
///
/// # 什麼時候走這條路
///
/// 鎖定英文＋非全形模式時。那個狀態的語意是「等同關掉輸入法」，
/// 打什麼就進去什麼——沒有候選、沒有底線、不必按 Enter。
///
/// 全形模式**不能**走這裡：`ａｂｃ` 那種轉換是在組字階段做的，
/// 不組字就沒機會轉。
///
/// 跟 `end_composition` 的差別是這裡從頭到尾沒有 composition——
/// `InsertTextAtSelection` 直接寫進插入點，宿主當成一般輸入。
pub(crate) fn insert_directly(context: &ITfContext, state: &mut State, ch: char) -> Result<()> {
    let mut buf = [0u16; 2];
    let text_utf16: Vec<u16> = ch.encode_utf16(&mut buf).to_vec();
    let tid = state.client_id;
    let context_owned = context.clone();
    let session = EditSession::new(move |ec| unsafe {
        let insert_at_sel: ITfInsertAtSelection = context_owned.cast()?;
        let range = insert_at_sel.InsertTextAtSelection(ec, Default::default(), &text_utf16)?;
        // 插入點要移到剛打的字後面，不然下一個字會蓋掉這個。
        //
        // `TF_SELECTION.range` 是 `ManuallyDrop`：`SetSelection` 只讀取、
        // 不接手所有權，用完要自己 drop，否則每打一個字就漏一個
        // `ITfRange` 的參考計數。
        let end = range.Clone()?;
        end.Collapse(ec, TF_ANCHOR_END)?;
        let mut selection = [TF_SELECTION {
            range: std::mem::ManuallyDrop::new(Some(end)),
            style: TF_SELECTIONSTYLE {
                ase: TF_AE_END,
                fInterimChar: false.into(),
            },
        }];
        let result = context_owned.SetSelection(ec, &selection);
        std::mem::ManuallyDrop::drop(&mut selection[0].range);
        result
    });
    let session_interface: ITfEditSession = session.into();
    unsafe {
        let _ =
            context.RequestEditSession(tid, &session_interface, TF_ES_READWRITE | TF_ES_SYNC)?;
    }
    Ok(())
}

/// 結束目前的 composition。
///
/// **`EndComposition` 只是「結束組字狀態」，不會刪掉已經顯示的文字。**
/// 可以想成組字中的文字是用鉛筆寫的草稿，`EndComposition` 只是「把鉛筆
/// 放下、不再標示這是草稿」，紙上的字還在。所以取消時必須先把那段範圍
/// 明確寫成空字串（等於擦掉），否則組字內容會以一般文字的身分留在文件裡。
///
/// 早期版本用 `Option<&str>`，`None` 分支直接跳過 `SetText` 就呼叫
/// `EndComposition`，導致按 Esc 之後底線消失、但文字還留著。
pub(crate) fn end_composition(
    context: &ITfContext,
    state: &mut State,
    kind: EndKind<'_>,
) -> Result<()> {
    state.candidate_window = None;
    state.session.clear();

    let Some(composition) = state.composition.take() else {
        return Ok(());
    };
    let tid = state.client_id;
    // 取消就是「換成空字串」，兩種意圖在這裡收斂成同一個寫入動作。
    let text_utf16: Vec<u16> = match kind {
        EndKind::Commit(s) => s.encode_utf16().collect(),
        EndKind::Cancel => Vec::new(),
    };

    let context_owned = context.clone();
    let session = EditSession::new(move |ec| unsafe {
        let range = composition.GetRange()?;
        range.SetText(ec, 0, &text_utf16)?;

        // 把宿主的插入點移到送出文字的後面。
        //
        // 順序很重要：必須在 `EndComposition` **之前**。組字一結束，
        // 這個 range 就不再指向有效的組字範圍，拿它去設 selection
        // 雖然不會報錯（HRESULT 仍是 S_OK），位置卻是錯的。
        //
        sync_caret_to_composition_end(&context_owned, &composition, ec)?;
        composition.EndComposition(ec)?;
        Ok(())
    });
    let session_interface: ITfEditSession = session.into();
    unsafe {
        let _ =
            context.RequestEditSession(tid, &session_interface, TF_ES_READWRITE | TF_ES_SYNC)?;
    }
    Ok(())
}
