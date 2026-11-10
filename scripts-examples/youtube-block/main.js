// 유튜브 사용자 차단 (spec 6.9 4번). 차단한 채널은 설정 칸 "channels"(글자 목록)에 둔다.
// 이름과 @핸들 둘 다로 맞춘다. 유튜브는 화면만 바꾸므로 변화를 지켜보다가 다시 훑는다.

const norm = (s) => (s || "").trim().replace(/^@/, "").toLowerCase();
let blocked = new Set();
const load = () => (blocked = new Set((tbs.settings.channels || []).map(norm).filter(Boolean)));
load();

// 영상 카드: 새 모양(yt-lockup-view-model)과 옛 모양
const CARDS = "yt-lockup-view-model, ytd-video-renderer, ytd-compact-video-renderer, ytd-grid-video-renderer";
const COMMENTS = "ytd-comment-view-model, ytd-comment-renderer";

function channelOfCard(card) {
  const link = card.querySelector('a[href^="/@"], a[href^="/channel/"]');
  const nameEl =
    card.querySelector("ytd-channel-name #text, #channel-name #text") ||
    card.querySelector("yt-content-metadata-view-model .ytContentMetadataViewModelMetadataRow .ytContentMetadataViewModelMetadataText") ||
    link;
  const name = nameEl ? nameEl.textContent.trim() : "";
  const href = link ? link.getAttribute("href") : "";
  const handle = href.startsWith("/@") ? decodeURIComponent(href.slice(1).split("/")[0]) : "";
  return { name, handle, nameEl };
}

function channelOfComment(c) {
  const a = c.querySelector("#author-text");
  if (!a) return { name: "", handle: "", nameEl: null };
  const href = a.getAttribute("href") || "";
  const handle = href.startsWith("/@") ? decodeURIComponent(href.slice(1).split("/")[0]) : "";
  return { name: a.textContent.trim(), handle, nameEl: a };
}

const isBlocked = (ch) => (ch.handle && blocked.has(norm(ch.handle))) || (ch.name && blocked.has(norm(ch.name)));

function hide(el) {
  const note = (tbs.settings.note || "").trim();
  el.classList.remove("tbs-yb-gone", "tbs-yb-noted");
  if (note) {
    el.classList.add("tbs-yb-noted");
    let n = el.querySelector(":scope > .tbs-yb-note");
    if (!n) {
      n = document.createElement("div");
      n.className = "tbs-yb-note";
      el.appendChild(n);
    }
    n.textContent = note;
  } else {
    el.classList.add("tbs-yb-gone");
  }
}

function show(el) {
  el.classList.remove("tbs-yb-gone", "tbs-yb-noted");
  el.querySelector(":scope > .tbs-yb-note")?.remove();
}

function addButton(card, ch) {
  if (!ch.nameEl || !(ch.handle || ch.name)) return;
  card.classList.add("tbs-yb-card");
  const parent = ch.nameEl.parentElement;
  if (!parent || parent.querySelector(":scope > .tbs-yb-btn")) return;
  const b = document.createElement("button");
  b.type = "button";
  b.className = "tbs-yb-btn";
  b.textContent = "차단";
  b.title = `${ch.name || ch.handle} 채널을 차단합니다`;
  b.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
    const entry = ch.handle ? "@" + ch.handle : ch.name;
    const list = [...(tbs.settings.channels || [])];
    if (!list.some((x) => norm(x) === norm(entry))) list.push(entry);
    tbs.setSetting("channels", list).catch((err) => console.warn("[유튜브 사용자 차단]", err));
  });
  ch.nameEl.after(b);
}

function scan() {
  timer = 0;
  for (const card of document.querySelectorAll(CARDS)) {
    const ch = channelOfCard(card);
    // 홈 화면은 카드를 감싼 칸째 숨겨야 빈자리가 남지 않는다
    const box = card.closest("ytd-rich-item-renderer") || card;
    if (isBlocked(ch)) hide(box);
    else {
      show(box);
      addButton(card, ch);
    }
  }
  for (const c of document.querySelectorAll(COMMENTS)) {
    const ch = channelOfComment(c);
    const box = (!c.closest("#replies") && c.closest("ytd-comment-thread-renderer")) || c;
    // 댓글에는 차단 버튼을 달지 않는다. 차단한 채널의 댓글만 숨긴다
    if (tbs.settings.comments && isBlocked(ch)) hide(box);
    else show(box);
  }
}

let timer = 0;
const later = () => {
  if (!timer) timer = setTimeout(scan, 250);
};
new MutationObserver(later).observe(document.documentElement, { childList: true, subtree: true });
document.addEventListener("yt-navigate-finish", later);
tbs.onSettingsChanged(() => {
  load();
  scan();
});
scan();
