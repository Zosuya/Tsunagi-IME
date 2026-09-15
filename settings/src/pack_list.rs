//! 擴充包清單：**上半部的那張表**。
//!
//! 跟 `pack_editor` 合在同一個分頁（使用者定的）——上面選一個包，
//! 下面就是它的內容。這裡只負責「列出來、勾選、選中誰」。
//!
//! # 勾選與選中是兩件事
//!
//! - **勾選框**管「這個包啟不啟用」，寫進 `config.toml`
//! - **點那一列**管「現在在看哪一個」，只是畫面狀態
//!
//! 分開的理由：使用者常常要看一個沒啟用的包裡有什麼（決定要不要
//! 開它），把兩件事綁在一起的話他得先啟用才看得到。
//!
//! # 為什麼順序不能調
//!
//! 清單依檔名排序，衝突時誰贏就照這個順序。使用者定的（2026-09-01）：
//! 先不做上下移動，包不多的時候夠用，真的撞到衝突再說。

use eframe::egui;
use ime_core::config::Config;

/// 名稱欄的上限。檔名可以很長（「我的遊戲專有名詞包」），超過就截
const NAME_MAX: f32 = 200.0;
/// 作者欄的上限。這欄是身分資訊，不是每次都要看的，給少一點
const AUTHOR_MAX: f32 = 120.0;

/// 放一格**最多** `max_w` 寬的文字，超過就截斷（尾端顯示 …）。
///
/// 跟 `pack_editor::cell` 不同：那個是「這塊就是我的」（固定寬，欄才
/// 撐得開），這個是「最多這麼寬」——短的名字仍然只占它需要的寬度，
/// 省下來的給說明欄。回傳實際占的範圍（整列的感應區要併它）。
fn capped(ui: &mut egui::Ui, max_w: f32, text: egui::RichText) -> egui::Rect {
    ui.scope(|ui| {
        ui.set_max_width(max_w);
        ui.add(egui::Label::new(text).truncate().selectable(false))
    })
    .inner
    .rect
}

// 這四個跟舊的擴充包分頁共用——**不要各寫一份**，
// 兩邊漂掉的話同一個包在清單與提示裡會顯示不同的內容
use crate::{breakdown, details, fill, kind_tag};

/// 畫清單。回傳使用者這一幀點中了哪個包（沒點就是 `None`）。
pub fn show(
    ui: &mut egui::Ui,
    cfg: &mut Config,
    cache: &mut Option<Vec<ime_core::pack::Info>>,
    current: Option<&str>,
) -> Option<String> {
    // 掃一次存起來——egui 每一幀都重畫，不快取等於每秒讀幾十次磁碟
    let list = cache.get_or_insert_with(|| {
        let mut v: Vec<ime_core::pack::Info> = ime_core::pack::available(&cfg.behavior.packs_dir)
            .into_iter()
            .map(|f| ime_core::pack::info(&cfg.behavior.packs_dir, &f))
            .collect();
        // **依顯示名排序，不是檔名**——使用者看到的是包名，照檔名排
        // 會看起來像沒排序（`devterms.txt` 顯示成「程式術語」）。
        v.sort_by(|a, b| a.title().cmp(b.title()));
        v
    });

    // 設定裡啟用了、但檔案不見的包。也排進清單裡（標紅），
    // 不然使用者會困惑「明明開了卻沒作用」
    let missing: Vec<String> = cfg
        .behavior
        .packs
        .iter()
        .filter(|n| !list.iter().any(|i| &i.file == *n))
        .cloned()
        .collect();

    if list.is_empty() && missing.is_empty() {
        ui.label("這個資料夾中沒有任何擴充包。");
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "請按上方的「＋新增包」建立，或將 .txt 檔放入資料夾後按「重新整理」。",
            )
            .weak(),
        );
        return None;
    }

    let mut pick = None;

    // **整張表的文字不給選取**（使用者定的）。
    //
    // egui 的 `Label` 預設可以拖曳選字，那會**吃掉整列的點擊**——
    // 滑鼠壓在文字上時它以為你要選字，點擊事件傳不到底下的感應區，
    // 所以「選中」怎麼點都沒反應（使用者實測回報）。
    // 這是一張清單不是一篇文章，選字沒有用處。
    ui.style_mut().interaction.selectable_labels = false;

    // **固定 10 列高，超過就捲、不足就空著**（使用者定的）。
    //
    // 高度固定的好處是**下半部不會跳來跳去**——包的數量一變，
    // 或者切換到別的資料夾，下面的基本資料與內容清單如果跟著上下
    // 移動，剛才在看的東西就不見了。
    //
    // 一列大約 28px（文字 20 ＋ 列距 8）。
    let list_h = 10.0 * 28.0;

    // **整個捲動區往左退一個捲軸的寬度**，理由同 `pack_editor` 的
    // 內容清單：不退的話它撐滿到最右邊，自己的捲軸就跟整頁那支疊在
    // 一起（使用者實測回報）。
    let bar = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin;
    let inner_w = (ui.available_width() - bar).max(200.0);

    // ★ **自由文字的欄一定要有上限**，不然整張表比視窗寬 ★
    //
    // Grid 的一欄有多寬，由那一欄最寬的那一格決定，而 `ui.label` 在
    // Grid 裡**不換行**——說明、作者、名稱都是包作者隨便打的字串，
    // 台語包那句「用注音打華語詞、候選出台語漢字。資料：台文華文線頂
    // 辭典」一放進去整張表就超過視窗。接下來是連鎖反應：這個捲動區
    // 橫向不能捲，只好**長大去配合內容**；整頁那支也一樣；下一幀所有
    // 東西拿到的「可用寬度」都是灌水過的，下面內容表的「往左退一個
    // 捲軸寬」就退在一個比視窗還寬的父容器裡——畫面上就是**兩支捲軸
    // 疊在一起**。這個症狀 2026-09-11 追了三輪才追到這裡：無視窗的
    // 測試包資料夾是空的，怎麼量都重現不了，最後是在真的 app 裡逐段
    // 印寬度才看到「清單畫完之後頁面變寬了」。
    //
    // 截掉不會丟資料：整列滑過去有 `details(info)` 顯示完整內容。
    //
    // 說明欄吃**剩下的**：其餘各欄都有上限，加總（含 7 個欄距與捲軸）
    // 就是它的起點。名稱與作者的上限是固定值，見那兩個常數。
    let desc_max = (inner_w
        - (24.0 + 48.0 + NAME_MAX + 56.0 + AUTHOR_MAX + 40.0 + 7.0 * 16.0 + bar))
        .max(140.0);

    // **用 Grid 而不是一行一個 horizontal**：欄位要對齊，
    // 名字長短不一的時候「詞彙數」那欄才不會參差不齊。
    // `striped` 讓相鄰的列有淡淡的底色，行數多也掃得下去。
    egui::ScrollArea::vertical()
        .id_salt("pack_list_scroll")
        .min_scrolled_height(list_h)
        .max_height(list_h)
        .max_width(inner_w)
        // 兩軸都不收縮：列不足時也維持這個高度，下面留白
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("pack_list")
                .num_columns(8)
                .striped(true)
                .spacing([16.0, 8.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("啟用").weak());
                    // **類別自成一欄**（使用者定的）：原本擠在名字前面當小標籤，
                    // 名字長的時候看不清楚，也沒辦法照類別掃過去
                    ui.label(egui::RichText::new("類別").weak());
                    ui.label(egui::RichText::new("名稱").weak());
                    ui.label(egui::RichText::new("詞彙數").weak());
                    ui.label(egui::RichText::new("內容").weak());
                    // 作者與版本擺最右——它們是「誰做的、哪一版」，
                    // 屬於身分資訊，不是每次都要看的東西
                    ui.label(egui::RichText::new("作者").weak());
                    ui.label(egui::RichText::new("版本").weak());
                    // 補一格空的撐滿寬度——隔行底色只塗到最後一格為止，
                    // 不撐滿的話色帶會斷在半路，看起來不像一整列
                    fill(ui);
                    ui.end_row();

                    let mut toggled = false;
                    for info in list.iter() {
                        let mut on = cfg.behavior.packs.iter().any(|p| p == &info.file);
                        let total = info.total();
                        let selected = current == Some(info.file.as_str());

                        // **先占一格畫布**：底色得畫在文字底下，但範圍要等
                        // 整列畫完才知道。egui 的做法是先留一個位置、記下編號，
                        // 等一下再回填——後畫的東西不會蓋住先畫的文字。
                        let bg = ui.painter().add(egui::Shape::Noop);
                        // **整列的範圍要把每一格的實際範圍併起來**。
                        //
                        // 不能用 `ui.cursor()`：在 Grid 裡它指的是下一格要畫
                        // 在哪，不是這一列的底部；`ui.min_rect()` 又是整個
                        // Grid 的範圍。兩個都拿來框這一列的話框出來的是錯的，
                        // 滑鼠根本碰不到（使用者實測回報：完全沒有反饋）。
                        let mut row_rect = egui::Rect::NOTHING;

                        // 空包不給勾——勾了也沒有任何作用，讓它可勾只會讓人
                        // 以為壞掉。停在這裡比讓使用者去猜好。
                        let cb = ui.add_enabled_ui(total > 0, |ui| {
                            if ui.checkbox(&mut on, "").changed() {
                                if on {
                                    cfg.behavior.packs.push(info.file.clone());
                                } else {
                                    cfg.behavior.packs.retain(|p| p != &info.file);
                                }
                                toggled = true;
                            }
                        });
                        // **勾選框不併進感應區**。整列的感應區是後畫的，在 egui 裡
                        // 後畫的在上層——併進去的話它會把勾選框整個蓋住，
                        // 勾選框就點不到了（使用者實測回報）。底色仍然要塗到它，
                        // 所以另外記著它的範圍
                        let cb_rect = cb.response.rect;

                        // 類別
                        row_rect = row_rect
                            .union(ui.label(egui::RichText::new(kind_tag(info)).weak()).rect);

                        // 名稱。顯示名來自檔頭的 `# name:`，沒寫就是檔名
                        let mut t = egui::RichText::new(info.title());
                        if selected {
                            t = t.strong();
                        }
                        let name = ui.horizontal(|ui| {
                            capped(ui, NAME_MAX, t);
                            // 唯讀的包標出來——它不給編，使用者要知道為什麼
                            if info.meta.readonly {
                                ui.label(egui::RichText::new("唯讀").weak().small())
                                    .on_hover_text("隨程式一起裝的包，改了下次更新會被覆蓋");
                            }
                        });
                        row_rect = row_rect.union(name.response.rect);

                        if total > 0 {
                            row_rect = row_rect.union(ui.label(format!("{total}")).rect);
                            // 有寫說明就顯示說明——那比語言分佈更有用；
                            // 分佈滑到名字上就看得到
                            let text = info
                                .meta
                                .description
                                .clone()
                                .unwrap_or_else(|| breakdown(info));
                            row_rect = row_rect.union(capped(
                                ui,
                                desc_max,
                                egui::RichText::new(text).weak(),
                            ));
                        } else if info.error == Some(ime_core::pack::PackReadError::NotUtf8) {
                            // **編碼不對跟「沒有詞」是兩回事**（§2.49.3）。
                            //
                            // Big5（記事本的「ANSI」）存的包格式完全正確，
                            // 使用者照「格式不對」那句話去檢查格式**永遠查
                            // 不出來**。要直接講編碼，還要講怎麼修。
                            row_rect =
                                row_rect.union(ui.label(egui::RichText::new("—").weak()).rect);
                            row_rect = row_rect.union(
                                ui.label(
                                    egui::RichText::new("編碼不是 UTF-8（用記事本另存為 UTF-8）")
                                        .color(egui::Color32::from_rgb(200, 80, 60))
                                        .italics(),
                                )
                                .rect,
                            );
                        } else {
                            row_rect =
                                row_rect.union(ui.label(egui::RichText::new("0").weak()).rect);
                            row_rect = row_rect.union(
                                ui.label(
                                    egui::RichText::new("尚未收錄詞彙（內容為空，或格式有誤）")
                                        .weak()
                                        .italics(),
                                )
                                .rect,
                            );
                        }

                        // 作者與版本。沒填的用短破折號佔位，欄位才對得齊
                        let dash = |s: Option<&String>| {
                            s.map(|x| x.trim().to_string())
                                .filter(|x| !x.is_empty())
                                .unwrap_or_else(|| "—".into())
                        };
                        row_rect = row_rect.union(capped(
                            ui,
                            AUTHOR_MAX,
                            egui::RichText::new(dash(info.meta.author.as_ref())).weak(),
                        ));
                        row_rect = row_rect.union(
                            ui.label(egui::RichText::new(dash(info.meta.version.as_ref())).weak())
                                .rect,
                        );
                        fill(ui);

                        // **整列都點得到，而且整列反白**（使用者實測回報：
                        // 只有名字那幾個字可以點，很難點中）。
                        //
                        // 做法是等這一列的東西都畫完，用「這一列的上緣」到
                        // 「現在的下緣」框出整列的範圍，再蓋一層透明的感應區。
                        // Grid 的每一格是獨立的 widget，沒有「一整列」這個東西
                        // ——範圍得自己量。
                        // 往左右各撐開一點，滑到列與列的縫隙也算數
                        let rect = row_rect.expand2(egui::vec2(4.0, 3.0));
                        // 底色與框線塗整列（含勾選框），感應區才避開它
                        let paint_rect = rect.union(cb_rect.expand2(egui::vec2(4.0, 3.0)));
                        let r = ui.interact(
                            rect,
                            egui::Id::new(("pack_row", &info.file)),
                            egui::Sense::click(),
                        );

                        // 回填上面留的那格畫布——底色因此落在文字底下
                        if selected || r.hovered() {
                            let v = ui.visuals();
                            let c = if selected {
                                v.selection.bg_fill.gamma_multiply(0.55)
                            } else {
                                // 滑鼠碰到：白色 10% 的淡底
                                egui::Color32::from_white_alpha(26)
                            };
                            ui.painter()
                                .set(bg, egui::epaint::RectShape::filled(paint_rect, 3.0, c));
                        }
                        // **滑鼠碰到再加一圈框線**，一眼看得出判定到哪
                        if r.hovered() {
                            ui.painter().rect_stroke(
                                paint_rect,
                                3.0,
                                egui::Stroke::new(1.0_f32, egui::Color32::from_white_alpha(70)),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if r.clicked() {
                            pick = Some(info.file.clone());
                        }
                        r.on_hover_text(details(info));

                        ui.end_row();
                    }

                    // **設定裡的順序就是衝突時的優先序**，所以要跟畫面上
                    // 看到的順序一致。不排的話順序等於「勾選的先後」，
                    // 那是使用者完全看不見的東西。
                    if toggled {
                        let order: std::collections::HashMap<&str, usize> = list
                            .iter()
                            .enumerate()
                            .map(|(i, x)| (x.file.as_str(), i))
                            .collect();
                        cfg.behavior
                            .packs
                            .sort_by_key(|p| order.get(p.as_str()).copied().unwrap_or(usize::MAX));
                    }

                    for name in &missing {
                        ui.label(""); // 啟用
                        ui.label(""); // 類別
                        ui.label(egui::RichText::new(name).strikethrough().weak());
                        ui.label(egui::RichText::new("—").weak()); // 詞彙數
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(0xC6, 0x28, 0x28),
                                "找不到檔案",
                            );
                            if ui.small_button("從清單移除").clicked() {
                                cfg.behavior.packs.retain(|n| n != name);
                            }
                        });
                        ui.label(""); // 作者
                        ui.label(""); // 版本
                        fill(ui);
                        ui.end_row();
                    }
                });
        });

    ui.add_space(8.0);
    let on = cfg.behavior.packs.len() - missing.len();
    let words: usize = list
        .iter()
        .filter(|i| cfg.behavior.packs.iter().any(|p| p == &i.file))
        .map(|i| i.total())
        .sum();
    ui.label(egui::RichText::new(format!("已啟用 {on} 個包，共 {words} 個詞")).weak());

    pick
}
