// 마우스 제스처 (spec 6.9 2번). 오른쪽 버튼을 누른 채 끌면 방향을 읽어 탭 동작을 한다.
// 맨 위 문서에서만 돈다 (틀 안에서는 브라우저에 부탁할 수 없다).
if (window.top !== window) return;

const NAMES = {
  back: "뒤로", forward: "앞으로", top: "맨 위로", bottom: "맨 아래로", home: "홈", reload: "새로고침", newTab: "새 탭",
  close: "탭 닫기", reopen: "닫은 탭 되살리기", prev: "이전 탭", next: "다음 탭", fullscreen: "전체 화면", maximize: "창 최대화", minimize: "창 최소화",
};

// 제스처를 쓰지 않을 사이트: 그 사이트와 그 아래 주소 (chess.com이면 www.chess.com도)
function excluded() {
  const host = location.hostname.toLowerCase();
  return (tbs.settings.excluded || []).some((s) => {
    const h = String(s).trim().toLowerCase().replace(/^\w+:\/\//, "").split("/")[0].replace(/^\*\./, "");
    return h && (host === h || host.endsWith("." + h));
  });
}

// 페이지 안에서 하는 동작 (스크롤). 나머지는 브라우저에 부탁한다
function run(a) {
  if (a === "top" || a === "bottom") {
    const el = document.scrollingElement || document.documentElement;
    window.scrollTo({ top: a === "top" ? 0 : el.scrollHeight, behavior: "smooth" });
    return;
  }
  tbs.tabs(a).catch((err) => console.warn("[마우스 제스처]", err));
}
let g = null; // 지금 긋는 제스처
let eatMenu = false; // 제스처를 했으면 뒤따르는 오른쪽 클릭 메뉴를 막는다

function actionOf(dirs) {
  const a = tbs.settings["g_" + dirs.join("")];
  return a && a !== "none" && NAMES[a] ? a : null;
}

// ───── 그리기 (시안 P: 검은 선 4px + 흰 테 2px, 꼬리표 높이 30px) ─────
let host = null;
let canvas = null;
let tag = null;

function ensureOverlay() {
  if (host) return;
  host = document.createElement("tbs-gesture");
  const root = host.attachShadow({ mode: "closed" });
  // Trusted Types를 쓰는 사이트(유튜브 등)에서도 되게 innerHTML 없이 만든다
  const style = document.createElement("style");
  style.textContent = `
  canvas { position: fixed; inset: 0; z-index: 2147483646; pointer-events: none; }
  span {
    position: fixed; z-index: 2147483647; pointer-events: none; display: none; transform: translateX(-50%);
    height: 30px; padding: 0 12px; border-radius: 8px; background: rgba(17, 17, 17, 0.92);
    box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.5); color: #fff;
    font: 600 13px/30px "Inter", "Noto Sans KR", system-ui, sans-serif; white-space: nowrap;
  }`;
  root.append(style, document.createElement("canvas"), document.createElement("span"));
  canvas = root.querySelector("canvas");
  tag = root.querySelector("span");
}

function draw() {
  ensureOverlay();
  if (!host.isConnected) document.documentElement.appendChild(host);
  const dpr = window.devicePixelRatio || 1;
  const w = innerWidth, h = innerHeight;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    canvas.style.width = w + "px";
    canvas.style.height = h + "px";
  }
  const c = canvas.getContext("2d");
  c.setTransform(dpr, 0, 0, dpr, 0, 0);
  c.clearRect(0, 0, w, h);
  if (tbs.settings.trail !== false && g.pts.length > 1) {
    c.lineCap = "round";
    c.lineJoin = "round";
    for (const [color, width] of [["#FFFFFF", 8], ["#111111", 4]]) {
      c.beginPath();
      c.moveTo(g.pts[0][0], g.pts[0][1]);
      for (const [x, y] of g.pts) c.lineTo(x, y);
      c.strokeStyle = color;
      c.lineWidth = width;
      c.stroke();
    }
  }
  // 알아들은 동작 이름: 궤적 아래 가운데
  const a = tbs.settings.feedback !== false && actionOf(g.dirs);
  if (a) {
    const xs = g.pts.map((p) => p[0]), ys = g.pts.map((p) => p[1]);
    tag.textContent = NAMES[a];
    tag.style.left = Math.round((Math.min(...xs) + Math.max(...xs)) / 2) + "px";
    tag.style.top = Math.round(Math.min(Math.max(...ys) + 24, h - 40)) + "px";
    tag.style.display = "block";
  } else {
    tag.style.display = "none";
  }
}

function clear() {
  if (host) host.remove();
  g = null;
}

addEventListener(
  "mousedown",
  (e) => {
    if (e.button !== 2 || excluded()) return;
    g = { ax: e.clientX, ay: e.clientY, dirs: [], moved: false, pts: [[e.clientX, e.clientY]] };
  },
  true,
);

addEventListener(
  "mousemove",
  (e) => {
    if (!g) return;
    if (!(e.buttons & 2)) return clear();
    g.pts.push([e.clientX, e.clientY]);
    const min = Number(tbs.settings.minDistance) || 30;
    const dx = e.clientX - g.ax, dy = e.clientY - g.ay;
    if (Math.abs(dx) >= min || Math.abs(dy) >= min) {
      const d = Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? "R" : "L") : dy > 0 ? "D" : "U";
      if (g.dirs[g.dirs.length - 1] !== d && g.dirs.length < 4) g.dirs.push(d);
      g.ax = e.clientX;
      g.ay = e.clientY;
      g.moved = true;
    }
    if (g.moved) draw();
  },
  true,
);

addEventListener(
  "mouseup",
  (e) => {
    if (e.button !== 2 || !g) return;
    const done = g;
    clear();
    if (!done.moved) return; // 끌지 않았으면 원래 오른쪽 클릭 메뉴
    eatMenu = true;
    setTimeout(() => (eatMenu = false), 500);
    e.preventDefault();
    e.stopPropagation();
    const a = actionOf(done.dirs);
    if (a) run(a);
  },
  true,
);

addEventListener(
  "contextmenu",
  (e) => {
    if (!eatMenu && !(g && g.moved)) return;
    eatMenu = false;
    e.preventDefault();
    e.stopPropagation();
  },
  true,
);

addEventListener("blur", clear);
