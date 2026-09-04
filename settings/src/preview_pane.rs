//! 設定頁右半邊那個**候選視窗的預覽**。
//!
//! 這裡是用 egui 「重畫一次」實際輸入法的候選視窗，好讓使用者調色時
//! 看得到結果。它跟設定項本身無關——設定頁那邊只管把值改掉，這邊
//! 只管照著值畫。
//!
//! **凡是「該畫成什麼樣」的判斷，一律問 `ime_core::render`**，不要在
//! 這裡自己算。兩邊各算一份而走鐘的 bug 出現過三次。

use crate::color::*;
use crate::{image_load, project_data_dir, PREVIEW_FAMILY};
use eframe::egui;
use ime_core::config::Config;
use ime_core::render::fixed;

pub(crate) const BASE_CORNER_RADIUS: f32 = fixed::CORNER_RADIUS as f32;

pub(crate) const BASE_PADDING: f32 = fixed::PADDING as f32;

pub(crate) const BASE_INDEX_GAP: f32 = fixed::INDEX_GAP as f32;

pub(crate) const BASE_FONT_SIZE: f32 = fixed::FONT_SIZE_PT as f32;

/// egui 的列高大約是字級的這個倍數。
///
/// 換算圓角比例要用（見 `highlight_radius`）——預覽的列高由 egui 的
/// 版面決定，不是實際繪製的 `fixed::LINE_HEIGHT`。
pub(crate) const EGUI_ROW_FACTOR: f32 = 1.4;

/// 反白塊的圓角半徑。
///
/// **實際繪製時是視窗圓角的一半**（`candidate_window.rs` 的
/// `fill_highlight(..., radius / 2.0, ...)`）——外框圓潤、內部反白
/// 收斂，兩層才不會看起來一樣圓。預覽這裡沿用完整半徑的話，
/// 反白塊在矮列上會變成膠囊形，跟實際差很多。
pub(crate) fn highlight_radius(cfg: &Config) -> f32 {
    // 換算本身在 `core`——這裡只負責算出「這一列有多高」
    ime_core::render::highlight_radius_for_row(BASE_FONT_SIZE * shrink(cfg) * EGUI_ROW_FACTOR)
}

/// 反白狀態下的底色與文字色。
///
/// **候選列與預覽列共用這一份**——這兩處各寫一次的話會走鐘，
/// 而且已經走鐘過：預覽列原本固定鋪 `highlight_bg`（近白）配
/// `highlight_text`（純白），在「只有高光」樣式下白底白字整個看不見。
///
/// `base_fg` 是沒反白時該用的字色（候選列是 `text`，預覽列是
/// `preview_text`）——「只有高光」沒有深底，字要維持原色才看得見。
pub(crate) fn highlight_colors(
    cfg: &Config,
    base_fg: egui::Color32,
) -> (egui::Color32, egui::Color32) {
    let p = paint_of(cfg, base_fg);
    let fill = match p.fill {
        Some(bg) => from_rgb(bg),
        // egui 沒有「不畫」，用全透明表示
        None => egui::Color32::TRANSPARENT,
    };
    (fill, from_rgb(p.text))
}

/// 問 `core`：這個樣式下反白該畫成什麼樣。
pub(crate) fn paint_of(cfg: &Config, base_fg: egui::Color32) -> ime_core::render::HighlightPaint {
    ime_core::render::highlight_paint(
        cfg.metrics.highlight_style,
        to_rgb(&cfg.colors.highlight_bg),
        to_rgb(&cfg.colors.highlight_text),
        ime_core::render::Rgb::new(base_fg.r(), base_fg.g(), base_fg.b()),
    )
}

/// 反白的上緣高光帶與亮邊。
///
/// 玻璃有厚度，上半部會反光——比例（42%）與不透明度跟實際繪製一致。
/// 「只有高光」還要加亮邊：沒有底色的話，那條邊是唯一能標出範圍的東西。
/// 畫反白的**底色 + 高光帶 + 亮邊**（不含文字）。
///
/// # 為什麼填色也要畫在這裡
///
/// 順序必須是「填色 → 高光 → 亮邊 → 文字」，跟實際繪製一致
/// （`d2d.rs` 的 `fill_highlight`）。
///
/// egui 的 `Frame` 會先鋪自己的底色再畫內容，而這裡的高光是**先預留
/// 位置、之後才填進去**（要等 `Frame` 畫完才知道大小）。預留的位置
/// 在圖層順序上比 `Frame` 更早——**高光會被 `Frame` 的底色蓋掉**。
///
/// 「只有高光」樣式看不出來（它本來就不鋪底），但「高光帶」樣式的底色
/// 是不透明的，高光整個消失，看起來就跟「實心」一模一樣。
///
/// 所以底色改由這裡畫，`Frame` 保持透明、只負責排版與文字。
/// `with_sheen` 為 `false` 時**只畫底色與亮邊，不畫上緣的光**——
/// 跟實際繪製一致（見 `d2d::fill_highlight`）。預覽列那個框住文字的
/// 反白就是這種：格子很矮，加了光會糊成一塊。
pub(crate) fn paint_highlight(
    ui: &egui::Ui,
    cfg: &Config,
    base_fg: egui::Color32,
    slot: egui::layers::ShapeIdx,
    r: egui::Rect,
    with_sheen: bool,
) {
    use ime_core::render as rd;
    let p = paint_of(cfg, base_fg);
    let radius = highlight_radius(cfg);
    let mut shapes: Vec<egui::Shape> = Vec::new();

    // 1. 底色
    if let Some(bg) = p.fill {
        shapes.push(egui::Shape::rect_filled(r, radius, from_rgb(bg)));
    }
    // 2. 上緣高光帶——比例與不透明度從 `core` 拿，跟實際繪製同一批數值
    if p.sheen && with_sheen {
        let band = egui::Rect::from_min_max(
            r.min,
            egui::pos2(r.max.x, r.min.y + r.height() * rd::SHEEN_BAND_RATIO),
        );
        let alpha = (rd::SHEEN_ALPHA * 255.0).round() as u8;
        shapes.push(gradient_mesh(
            band,
            egui::Color32::from_white_alpha(alpha),
            egui::Color32::TRANSPARENT,
        ));
    }
    // 3. 亮邊
    if p.outline {
        let c = to_color(&cfg.colors.highlight_bg).gamma_multiply(rd::OUTLINE_DIM);
        shapes.push(egui::Shape::rect_stroke(
            r,
            radius,
            egui::Stroke::new(rd::OUTLINE_WIDTH, c),
            egui::StrokeKind::Inside,
        ));
    }
    ui.painter().set(slot, egui::Shape::Vec(shapes));
}

/// 展示區的縮放係數。
///
/// 實際縮放到 200% 時展示卡會撐爆設定視窗，所以這裡再打個折——
/// 看的是**比例**對不對，不是絕對大小。
pub(crate) fn shrink(cfg: &Config) -> f32 {
    cfg.metrics.scale_percent as f32 / 100.0
}

/// 在一塊矩形上畫垂直漸層（上→下）。
///
/// egui 的 `Frame::fill` 只吃單色，漸層要自己疊一個 `Mesh`——
/// 四個頂點各自帶顏色，上面兩個用起始色、下面兩個用結束色，
/// GPU 自己內插。
///
/// 這是為了讓**展示區跟實際的候選視窗一致**：那邊用 GDI 的
/// `GradientFill`，這邊用 egui 的 mesh，兩者的視覺結果相同。
/// 描邊的顏色。固定黑色，理由同實際繪製：用主題色的話遇到同色系
/// 背景就失效了。
pub(crate) const OUTLINE_COLOR: egui::Color32 = egui::Color32::BLACK;

/// 畫一段文字，依設定決定要不要描邊。**預覽區的文字都走這裡**，
/// 不然「描邊」這個開關會漏掉其中幾處，預覽就跟實際對不上。
///
/// egui 沒有現成的描邊，做法跟實際繪製一樣：把同一段字先往八個方向
/// 各畫一次深色，再把正常的字畫在上面。
pub(crate) fn label_outlined(
    ui: &mut egui::Ui,
    cfg: &Config,
    color: egui::Color32,
    text: egui::RichText,
) {
    // **預覽區一律用使用者選的字型**，介面自己維持原本的。
    // 這裡是預覽文字唯一的出入口，套在這裡就不會漏掉哪一處。
    let text = text.family(egui::FontFamily::Name(PREVIEW_FAMILY.into()));
    let bg = &cfg.background;
    if !bg.outlined() {
        // **這裡一定要是 `ui.colored_label`，不能是 `label_outlined`**
        // ——呼叫自己就是無限遞迴，畫面直接卡死。同一個錯誤在
        // `candidate_window::draw_label` 也犯過一次，成因相同：
        // 加函式時用全域取代換掉呼叫端，連新函式自己的內部呼叫
        // 都被掃到。
        ui.colored_label(color, text);
        return;
    }
    // 自己排版才畫得了偏移的複本。`Extend` = 不折行
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let (rect, _) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    let w = (BASE_FONT_SIZE * shrink(cfg) / 12.0).max(1.0);
    let oc = OUTLINE_COLOR.gamma_multiply(bg.outline_alpha());
    const DIRS: [(f32, f32); 8] = [
        (-1.0, 0.0),
        (1.0, 0.0),
        (0.0, -1.0),
        (0.0, 1.0),
        (-0.7, -0.7),
        (0.7, -0.7),
        (-0.7, 0.7),
        (0.7, 0.7),
    ];
    let painter = ui.painter();
    for (dx, dy) in DIRS {
        painter.galley(rect.min + egui::vec2(dx * w, dy * w), galley.clone(), oc);
    }
    painter.galley(rect.min, galley, color);
}

/// 背景圖的貼圖。**載入要讀檔＋解碼，用 egui 的暫存區依路徑快取**，
/// 路徑沒變就重用，不會每幀重讀。
///
/// 存的是 `Option`：載入失敗也記下來，才不會每幀重試一次讀檔。
pub(crate) fn background_texture(ctx: &egui::Context, cfg: &Config) -> Option<egui::TextureHandle> {
    let want = cfg.background.image.trim().to_string();
    if want.is_empty() {
        return None;
    }
    let key = egui::Id::new("bg_texture");
    // 快取裡放 (路徑, 結果)，路徑不同就重載
    let cached: Option<(String, Option<egui::TextureHandle>)> = ctx.data(|d| d.get_temp(key));
    if let Some((k, v)) = &cached {
        if k == &want {
            return v.clone();
        }
    }
    let loaded = ime_core::config::resolve_image_path(&want, project_data_dir().as_deref())
        .and_then(|p| image_load::load_rgba(&p))
        .map(|(w, h, rgba)| {
            let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
            ctx.load_texture("ime_bg", img, egui::TextureOptions::LINEAR)
        });
    ctx.data_mut(|d| d.insert_temp(key, (want, loaded.clone())));
    loaded
}

/// 等比填滿（超出裁掉）時，該取貼圖的哪一塊。
///
/// 回傳的是 uv 座標（0～1）。**倍率跟實際繪製共用同一份**
/// （`ime_core::render::cover_scale`）——這裡只負責把它換算成 egui 要的
/// uv，兩邊各算一次的話預覽會跟實際畫出來的不一樣。
pub(crate) fn cover_uv(dst: egui::Vec2, img: egui::Vec2) -> egui::Rect {
    if dst.x <= 0.0 || dst.y <= 0.0 || img.x <= 0.0 || img.y <= 0.0 {
        return egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    }
    let scale = ime_core::render::cover_scale(dst.x, dst.y, img.x, img.y);
    // 縮放後圖有多大，換算回「用了原圖的幾成」
    let used = egui::vec2(dst.x / (img.x * scale), dst.y / (img.y * scale));
    let off = (egui::vec2(1.0, 1.0) - used) / 2.0;
    egui::Rect::from_min_size(egui::pos2(off.x, off.y), used)
}

pub(crate) fn gradient_mesh(
    rect: egui::Rect,
    top: egui::Color32,
    bottom: egui::Color32,
) -> egui::Shape {
    let mut mesh = egui::epaint::Mesh::default();
    // 頂點順序：左上、右上、左下、右下
    for (pos, color) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.left_bottom(), bottom),
        (rect.right_bottom(), bottom),
    ] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    egui::Shape::mesh(mesh)
}

/// 漸層的下緣色；沒設定就跟上緣同色（純色）。
pub(crate) fn gradient_bottom(base: &str, second: &str) -> egui::Color32 {
    if second.is_empty() {
        to_color(base)
    } else {
        to_color(second)
    }
}

/// 元件展示：把候選視窗的三種狀態並排畫出來。
///
/// # 為什麼要三種而不是一種
///
/// 每個顏色角色都要**在真正用到它的地方**看得見，不然改了不知道
/// 影響什麼。三種狀態合起來剛好把 11 個角色都用上：
///
/// | 狀態 | 用到的角色 |
/// |---|---|
/// | 打字中 | 預覽列文字／底色、標記符號 |
/// | 選字 | 候選字、編號、反白三色、分隔線 |
/// | 切法選單 | 長內容下的視窗底色與外框 |
pub(crate) fn preview(ui: &mut egui::Ui, cfg: &Config) {
    ui.horizontal_top(|ui| {
        state_card(ui, cfg, "打字中", |ui, cfg| {
            preview_row(ui, cfg, "su3cl3");
        });
        ui.add_space(10.0);
        state_card(ui, cfg, "選字", |ui, cfg| {
            preview_row_boxed(ui, cfg, "你好", Some("你"));
            separator(ui, cfg);
            for (i, ch) in ["你", "擬", "妳", "泥"].iter().enumerate() {
                cand_row(ui, cfg, i + 1, ch, i == 1);
            }
        });
        ui.add_space(10.0);
        state_card(ui, cfg, "切法選單", |ui, cfg| {
            preview_row(ui, cfg, "你好");
            separator(ui, cfg);
            for (i, t) in ["你好", "ni3好", "你cl3"].iter().enumerate() {
                cand_row(ui, cfg, i + 1, t, i == 0);
            }
        });
    });
}

/// 一張狀態卡：標題 + 照設定畫出來的視窗。
///
/// 外框改成陰影（實際繪製用 `CS_DROPSHADOW`），這裡用淡灰陰影模擬。
pub(crate) fn state_card(
    ui: &mut egui::Ui,
    cfg: &Config,
    title: &str,
    body: impl FnOnce(&mut egui::Ui, &Config),
) {
    let k = shrink(cfg);
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(title).weak());
        ui.add_space(2.0);
        // **漸層畫在內容底下**：用 `painter().add()` 先佔一個位子，
        // 等 Frame 畫完知道實際大小之後，再把那個位子換成漸層 mesh。
        //
        // egui 的繪製順序就是加入順序，所以先佔位＝畫在底下。
        // 直接在 Frame 之後畫會蓋住文字。
        let bg2 = gradient_bottom(&cfg.colors.window_bg, &cfg.colors.window_bg2);
        let slot = ui.painter().add(egui::Shape::Noop);
        // 有漸層時 Frame 不填色（漸層自己畫），沒有才填
        let flat = to_color(&cfg.colors.window_bg) == bg2;
        let resp = egui::Frame::NONE
            .fill(if flat {
                to_color(&cfg.colors.window_bg)
            } else {
                egui::Color32::TRANSPARENT
            })
            .shadow(egui::epaint::Shadow {
                offset: [0, 2],
                blur: 8,
                spread: 0,
                color: egui::Color32::from_black_alpha(40),
            })
            .corner_radius(BASE_CORNER_RADIUS * k)
            .inner_margin(BASE_PADDING * k)
            .show(ui, |ui| {
                ui.set_width(140.0 * k);
                body(ui, cfg);
            });
        // 把剛才佔的位子換成「背景圖 + 漸層」。
        //
        // 順序跟實際繪製一致：先圖、再把漸層半透明蓋上去。濃度由設定
        // 決定（`overlay_alpha`），拉到 1 就是純圖片、0 就是純色。
        let r = resp.response.rect;
        let tex = background_texture(ui.ctx(), cfg);
        let mut shapes: Vec<egui::Shape> = Vec::new();
        if let Some(tex) = &tex {
            let uv = cover_uv(r.size(), tex.size_vec2());
            shapes.push(egui::Shape::Rect(egui::epaint::RectShape {
                rect: r,
                corner_radius: (BASE_CORNER_RADIUS * k).into(),
                fill: egui::Color32::WHITE, // 貼圖的調色，白＝原色
                stroke: egui::Stroke::NONE,
                stroke_kind: egui::StrokeKind::Inside,
                round_to_pixels: None,
                blur_width: 0.0,
                brush: Some(std::sync::Arc::new(egui::epaint::Brush {
                    fill_texture_id: tex.id(),
                    uv,
                })),
            }));
        }
        let alpha = if tex.is_some() {
            cfg.background.overlay_alpha()
        } else {
            1.0
        };
        if tex.is_some() || to_color(&cfg.colors.window_bg) != bg2 {
            let a = |c: egui::Color32| c.gamma_multiply(alpha);
            shapes.push(gradient_mesh(r, a(to_color(&cfg.colors.window_bg)), a(bg2)));
        }
        if !shapes.is_empty() {
            ui.painter().set(slot, egui::Shape::Vec(shapes));
        }
    });
}

/// 預覽列：組字當下的第一名切法。
///
/// **不套圓角**——使用者要求圓角只影響候選清單那一塊。
pub(crate) fn preview_row(ui: &mut egui::Ui, cfg: &Config, text: &str) {
    preview_row_boxed(ui, cfg, text, None)
}

/// 預覽列，其中 `boxed` 那一段反白（選字時標出正在選哪一格）。
///
/// 反白用的是**跟候選清單同一組顏色**——「選中」這件事在整個視窗裡
/// 只有一種長相。原本畫的是細外框，小字級下不夠顯眼。
///
/// 實際的候選視窗是量字寬算出反白塊的位置；這裡是預覽，用 egui 的
/// 逐段排版達到同樣的視覺效果就夠了。
pub(crate) fn preview_row_boxed(ui: &mut egui::Ui, cfg: &Config, text: &str, boxed: Option<&str>) {
    let size = BASE_FONT_SIZE * shrink(cfg);
    let fg = to_color(&cfg.colors.preview_text);
    // 漸層的作法同 `state_card`：先佔位、畫完才知道大小、再替換
    let bg2 = gradient_bottom(&cfg.colors.preview_bg, &cfg.colors.preview_bg2);
    let flat = to_color(&cfg.colors.preview_bg) == bg2;
    let slot = ui.painter().add(egui::Shape::Noop);
    let resp = egui::Frame::NONE
        .fill(if flat {
            to_color(&cfg.colors.preview_bg)
        } else {
            egui::Color32::TRANSPARENT
        })
        .inner_margin(egui::Margin::symmetric(2, 1))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // 逐段排版才反白得了其中一段。間距歸零，看起來才是連續的一句話
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(|ui| {
                label_outlined(ui, cfg, fg, egui::RichText::new("▶ ").size(size));
                match boxed {
                    // 反白那一段：**要跟著反白樣式走**，跟候選列同一套。
                    Some(seg) => {
                        let (_, seg_fg) = highlight_colors(cfg, fg);
                        let (before, rest) = text.split_once(seg).unwrap_or((text, ""));
                        label_outlined(ui, cfg, fg, egui::RichText::new(before).size(size));
                        // **底色、高光、亮邊全部畫在這個預留位置**——`Frame`
                        // 保持透明只負責排版，順序才會是「底色→高光→文字」
                        let seg_slot = ui.painter().add(egui::Shape::Noop);
                        let seg_resp =
                            egui::Frame::NONE
                                .corner_radius(highlight_radius(cfg))
                                .show(ui, |ui| {
                                    label_outlined(
                                        ui,
                                        cfg,
                                        seg_fg,
                                        egui::RichText::new(seg).size(size),
                                    );
                                });
                        // 這一格很矮，不畫上緣的光，只留外框
                        paint_highlight(ui, cfg, fg, seg_slot, seg_resp.response.rect, false);
                        label_outlined(ui, cfg, fg, egui::RichText::new(rest).size(size));
                    }
                    None => {
                        label_outlined(ui, cfg, fg, egui::RichText::new(text).size(size));
                    }
                }
            });
        });
    if !flat {
        let r = resp.response.rect;
        ui.painter().set(
            slot,
            gradient_mesh(r, to_color(&cfg.colors.preview_bg), bg2),
        );
    }
}

/// 預覽列與候選清單之間的分隔線。
pub(crate) fn separator(ui: &mut egui::Ui, cfg: &Config) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 3.0), egui::Sense::hover());
    let y = rect.center().y;
    ui.painter().hline(
        rect.x_range(),
        y,
        egui::Stroke::new(1.0_f32, to_color(&cfg.colors.separator)),
    );
}

/// 一列候選：編號 + 文字，選中的鋪反白底。
///
/// 反白時編號與文字**同色**（兩個角色已合併）。
pub(crate) fn cand_row(ui: &mut egui::Ui, cfg: &Config, num: usize, text: &str, hot: bool) {
    // **編號與候選字一律同色**——跟實際繪製一致（見
    // `candidate_window.rs` 的 `row_color`）。編號原本用淡灰、反白時
    // 才跟文字同色，同一個東西在兩種狀態下換顏色看起來像兩種資訊。
    let row_c = if hot {
        // 底色由 `paint_highlight` 畫，這裡只要文字色
        let (_, t) = highlight_colors(cfg, to_color(&cfg.colors.text));
        t
    } else {
        to_color(&cfg.colors.text)
    };
    let (num_c, txt_c) = (row_c, row_c);
    // **底色、高光、亮邊全部畫在這個預留位置**——`Frame` 保持透明
    // 只負責排版，順序才會是「底色→高光→亮邊→文字」，跟實際繪製一致
    let slot = hot.then(|| ui.painter().add(egui::Shape::Noop));
    let resp = egui::Frame::NONE
        .corner_radius(highlight_radius(cfg))
        .inner_margin(egui::Margin::symmetric(3, 1))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                label_outlined(
                    ui,
                    cfg,
                    num_c,
                    egui::RichText::new(format!("{num}")).size(BASE_FONT_SIZE * shrink(cfg)),
                );
                ui.add_space(BASE_INDEX_GAP * shrink(cfg));
                label_outlined(
                    ui,
                    cfg,
                    txt_c,
                    egui::RichText::new(text).size(BASE_FONT_SIZE * shrink(cfg)),
                );
            });
        });
    if let Some(slot) = slot {
        paint_highlight(
            ui,
            cfg,
            to_color(&cfg.colors.text),
            slot,
            resp.response.rect,
            true,
        );
    }
}
