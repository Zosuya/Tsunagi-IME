const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const rand = (a, b) => a + Math.random() * (b - a);

// ── 下載連結 ──
// HTML 裡先寫死目前版本的網址（查詢失敗也能下載），這裡再向 GitHub 問最新版蓋掉，
// 以後發新版不必改網站。
const REPO = "Zosuya/Tsunagi-IME";
const isMac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
const links = { win: null, mac: null };
const PLATFORM_LABEL = { win: "Windows 版", mac: "macOS 版" };

function applyDownloads() {
  const mine = isMac ? "mac" : "win";
  const other = isMac ? "win" : "mac";
  document.querySelectorAll("[data-dl]").forEach((a) => {
    const kind = a.dataset.dl;
    const target = kind === "auto" ? mine : kind === "other" ? other : kind;
    if (links[target]) a.href = links[target];
    else if (kind === "auto" || kind === "other") {
      const fallback = document.querySelector(`[data-dl="${target}"]`);
      if (fallback) a.href = fallback.href;
    }
  });
  document.querySelectorAll("[data-dl-label]").forEach((el) => (el.textContent = PLATFORM_LABEL[mine]));
  document.querySelectorAll("[data-dl-other-label]").forEach((el) => (el.textContent = PLATFORM_LABEL[other]));
}

applyDownloads();

fetch(`https://api.github.com/repos/${REPO}/releases/latest`)
  .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
  .then((release) => {
    for (const asset of release.assets || []) {
      if (asset.name.endsWith("-win-setup.exe")) links.win = asset.browser_download_url;
      else if (asset.name.endsWith(".pkg")) links.mac = asset.browser_download_url;
    }
    if (release.tag_name) {
      document.querySelectorAll("[data-dl-version]").forEach((el) => (el.textContent = release.tag_name));
    }
    applyDownloads();
  })
  .catch(() => {});

// ── 安裝分頁：Mac 訪客預設開 macOS 那頁 ──
function openInstallTab(name) {
  document.querySelectorAll(".tab-btn").forEach((b) => b.classList.toggle("active", b.dataset.tab === name));
  document.querySelectorAll(".tab-panel").forEach((p) => p.classList.toggle("active", p.id === "tab-" + name));
}
document.querySelectorAll(".tab-btn").forEach((btn) => {
  btn.addEventListener("click", () => openInstallTab(btn.dataset.tab));
});
if (isMac) openInstallTab("mac");

// ── 功能展示：左邊清單切換右邊畫面 ──
document.querySelectorAll(".show-tab").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".show-tab").forEach((b) => b.classList.toggle("active", b === btn));
    document.querySelectorAll(".show-panel").forEach((p) =>
      p.classList.toggle("active", p.id === "show-" + btn.dataset.show)
    );
  });
});

// ── 開頭的打字示範 ──
// demo.json 是 `gen_site_demo` 逐鍵跑真正的引擎產生的：每打一鍵，
// 畫面就換成引擎那一步的輸出，中途被後面的鍵改掉的字也照實播出來。
const keysEl = document.getElementById("demo-keys");
const outEl = document.getElementById("demo-out");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// 同語言相鄰的格子併成一段，底線才會連成一條
function renderStep(cells, prev) {
  outEl.innerHTML = "";
  let run = null;
  cells.forEach(([text, lang], i) => {
    const changed = !prev || !prev[i] || prev[i][0] !== text;
    const piece = document.createElement("span");
    piece.textContent = text;
    if (changed) piece.className = "flash";
    if (!run || run.dataset.lang !== lang) {
      run = document.createElement("span");
      run.className = "run " + lang;
      run.dataset.lang = lang;
      outEl.appendChild(run);
    }
    run.appendChild(piece);
  });
}

async function runDemo() {
  let demos;
  try {
    demos = await (await fetch("demo.json")).json();
  } catch {
    return;
  }
  if (reduceMotion) {
    const d = demos[1] || demos[0];
    keysEl.textContent = d.keys;
    renderStep(d.steps[d.steps.length - 1], d.steps[d.steps.length - 1]);
    return;
  }
  for (let i = 0; ; i = (i + 1) % demos.length) {
    const { keys, steps } = demos[i];
    keysEl.textContent = "";
    outEl.innerHTML = "";
    await sleep(500);
    let prev = null;
    const chars = [...keys];
    for (let k = 0; k < chars.length; k++) {
      keysEl.textContent += chars[k];
      renderStep(steps[k], prev);
      prev = steps[k];
      await sleep(chars[k] === " " ? 220 : rand(110, 190));
    }
    await sleep(2800);
  }
}
if (keysEl && outEl) runDemo();

// ── 背景飄字：英文、日文假名、注音各一組，對應三種輸入 ──
const GLYPHS = [
  "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
  "あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみむめもやゆよらりるれろわをん",
  "ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙㄧㄨㄩㄚㄛㄜㄝㄞㄟㄠㄡㄢㄣㄤㄥㄦ",
];
const drift = document.createElement("div");
drift.className = "drift";
drift.setAttribute("aria-hidden", "true");
const count = window.innerWidth < 760 ? 14 : 28;
for (let i = 0; i < count; i++) {
  const set = GLYPHS[i % GLYPHS.length];
  const span = document.createElement("span");
  span.textContent = set[Math.floor(Math.random() * set.length)];
  const duration = rand(28, 60);
  span.style.left = rand(0, 100) + "vw";
  span.style.fontSize = rand(18, 56) + "px";
  span.style.opacity = rand(0.05, 0.16).toFixed(2);
  span.style.animationDuration = duration + "s";
  // 負的延遲讓字一開始就散在整個畫面上，而不是全部從底部同時冒出來
  span.style.animationDelay = -rand(0, duration) + "s";
  span.style.setProperty("--r0", rand(-20, 20) + "deg");
  span.style.setProperty("--r1", rand(-40, 40) + "deg");
  drift.appendChild(span);
}
document.body.prepend(drift);

// ── GIF 延遲載入：捲到附近才載，功能展示切換到那頁時也會觸發 ──
const lazyImages = document.querySelectorAll("img[data-src]");
const load = (img) => {
  img.src = img.dataset.src;
  img.removeAttribute("data-src");
};
if ("IntersectionObserver" in window) {
  const observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          load(entry.target);
          observer.unobserve(entry.target);
        }
      });
    },
    { rootMargin: "300px" }
  );
  lazyImages.forEach((img) => observer.observe(img));
} else {
  lazyImages.forEach(load);
}
