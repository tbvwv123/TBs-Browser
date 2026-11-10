// 어떤 영상이든 PiP (spec 6.9 1번). 모든 문서(틀 포함)에서 돈다. 단축키와 "이 탭에서 실행"은 맨 위 문서로 온다.

const area = (v) => {
  const r = v.getBoundingClientRect();
  return r.width * r.height;
};

// 보이는 영상 가운데 재생 중인 가장 큰 것. 재생 중인 것이 없으면 가장 큰 것
function pick() {
  const all = [...document.querySelectorAll("video")].filter((v) => v.readyState > 0 && area(v) > 0);
  const playing = all.filter((v) => !v.paused && !v.ended);
  return (playing.length ? playing : all).sort((a, b) => area(b) - area(a))[0] || null;
}

async function toggle(video) {
  try {
    if (document.pictureInPictureElement) {
      await document.exitPictureInPicture();
      return;
    }
    const v = video || pick();
    if (!v) return;
    // PiP를 막아 둔 사이트: 막는 속성을 지우고 시도한다
    v.removeAttribute("disablepictureinpicture");
    v.disablePictureInPicture = false;
    await v.requestPictureInPicture();
  } catch (e) {
    console.warn("[PiP]", e);
  }
}

tbs.onShortcut("pip", () => toggle());
tbs.onAction("pip", () => toggle());

// ───── 영상 위 PiP 버튼 (시안 P: 높이 28px, 오른쪽 위에서 12px) ─────
const host = document.createElement("tbs-pip-button");
const root = host.attachShadow({ mode: "closed" });
// 유튜브처럼 Trusted Types를 쓰는 사이트에서는 innerHTML을 쓸 수 없으므로 요소를 직접 만든다
const style = document.createElement("style");
style.textContent = `
  button {
    position: fixed; z-index: 2147483647; display: none; align-items: center; gap: 6px;
    height: 28px; padding: 0 10px; border: 0; border-radius: 8px; margin: 0;
    background: rgba(17, 17, 17, 0.8); box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.4);
    color: #fff; font: 600 12px/1 "Inter", "Noto Sans KR", system-ui, sans-serif; cursor: pointer;
  }
  button.show { display: flex; }
  svg { flex: none; }`;
const button = document.createElement("button");
button.type = "button";
button.setAttribute("aria-label", "PiP로 보기");
const NS = "http://www.w3.org/2000/svg";
const icon = document.createElementNS(NS, "svg");
for (const [k, v] of Object.entries({ width: 14, height: 14, viewBox: "0 0 24 24", fill: "none", stroke: "#fff", "stroke-width": 1.8, "stroke-linecap": "round", "stroke-linejoin": "round", "aria-hidden": "true" })) icon.setAttribute(k, v);
for (const r of [{ x: 3, y: 5, width: 18, height: 14, rx: 2 }, { x: 12, y: 12, width: 6, height: 4, rx: 1 }]) {
  const el = document.createElementNS(NS, "rect");
  for (const [k, v] of Object.entries(r)) el.setAttribute(k, v);
  icon.appendChild(el);
}
button.append(icon, "PiP");
root.append(style, button);
let target = null;
let last = 0;

button.addEventListener("click", (e) => {
  e.preventDefault();
  e.stopPropagation();
  toggle(target);
});

function place(x, y) {
  if (!tbs.settings.hoverButton || document.fullscreenElement) return hide();
  // 가리키는 자리의 영상 (영상 위에 투명한 막이 덮여 있어도 찾는다)
  const v = [...document.querySelectorAll("video")].find((v) => {
    const r = v.getBoundingClientRect();
    return r.width >= 160 && r.height >= 90 && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  });
  if (!v) return hide();
  target = v;
  if (!host.isConnected) document.documentElement.appendChild(host);
  const r = v.getBoundingClientRect();
  button.classList.add("show");
  const w = button.offsetWidth;
  button.style.left = Math.round(r.right - 12 - w) + "px";
  button.style.top = Math.round(Math.max(r.top, 0) + 12) + "px";
}

function hide() {
  target = null;
  button.classList.remove("show");
}

document.addEventListener(
  "pointermove",
  (e) => {
    // 너무 자주 재지 않는다 (30ms에 한 번)
    if (e.timeStamp - last < 30) return;
    last = e.timeStamp;
    place(e.clientX, e.clientY);
  },
  { passive: true, capture: true },
);
document.documentElement.addEventListener("mouseleave", hide);
document.addEventListener("scroll", hide, { passive: true, capture: true });
tbs.onSettingsChanged((s) => {
  if (!s.hoverButton) hide();
});
