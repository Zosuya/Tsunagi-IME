---
原編號: "2.52.25"
date: 2026-09-08
status: done
一句話: "每個宿主連線各有一個控制器，全域緩衝會讓兩個 App 共用組字內容；set_ivars 一定要在 super init 之前"
pitfall: true
相關:
  - "[[spike 2：IMK Echo 輸入法]]"
---

# 組字緩衝從 thread_local 換成 ivar

spike 階段刻意留的兩個簡化之一（[[spike 2：IMK Echo 輸入法]]），接 core 之前要還的債。

**為什麼非改不可**：**每個宿主連線各有一個 `IMKInputController`**。全域的
`thread_local` 緩衝區在單一宿主下看不出問題——今天整天的驗收都沒碰到
——但同時在兩個 App 打字就會**共用同一份組字內容**：在 TextEdit 打
`abc` 不送出、切到 Safari 打 `xyz`，會變成 `abcxyz`。

### 寫法：覆寫 init，順序不能反

```rust
#[ivars = Composition]
struct EchoController;

#[unsafe(method_id(initWithServer:delegate:client:))]
fn init_with_server(this: Allocated<Self>, …) -> Option<Retained<Self>> {
    let this = this.set_ivars(Composition::default());   // ← 先
    unsafe { msg_send![super(this), initWithServer: …] } // ← 後
}
```

`define_class!` 的 ivars **不會自己生出來**，沒 `set_ivars` 就去碰是未定義
行為。而 **`set_ivars` 一定要在呼叫 super 的 init 之前**——super 的 init
可能回頭呼叫我們覆寫的方法，那時 ivars 必須已經在了。

`initWithServer:delegate:client:` 是 `IMKInputController` 的指定初始化子，
也就是 IMK 造控制器實際走的那一支。

連帶把 `on_key` 與 `commit` 從自由函式改成方法，所有存取都經過
`self.buf()`。`marked_attributed`／`set_marked`／`fake_candidates` 維持自由
函式——它們不碰狀態。

### 驗收

在 TextEdit 組字中途切到 Safari 再打字，兩邊的組字區各自獨立。基本功能
（打字、Backspace、Enter、Esc、方向鍵）一併確認沒被弄壞。

### 另一件

[[spike 2：IMK Echo 輸入法]] 記的兩件事，另一件（候選面板要能點選）**同日也做了**，
見 [[候選面板接上 core 的繪製決策，並且點得動]]。
