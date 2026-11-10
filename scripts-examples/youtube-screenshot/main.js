// 유튜브 플레이어 아래에 "현재 화면 저장" 버튼을 단다 (spec 6.5).
// 누르면 지금 영상 화면을 PNG로 다운로드 폴더에 저장한다. 다른 영상으로 넘어가도 버튼을 다시 단다.
// tbs.request(...)는 브라우저가 이 스크립트에 넘겨주는 부탁 통로다 (spec 6.3).
const BAR_ID = "tbs-yt-shot";
const LABEL = "현재 화면 PNG로 저장";

function video() {
  return document.querySelector("#movie_player video.html5-main-video") || document.querySelector("video");
}

function title() {
  const h = document.querySelector("ytd-watch-metadata h1") || document.querySelector("h1.title");
  return ((h && h.textContent) || document.title).trim().replace(/\s*-\s*YouTube$/, "");
}

function at(v) {
  const t = Math.floor(v.currentTime || 0);
  const h = Math.floor(t / 3600), m = Math.floor((t % 3600) / 60), s = t % 60;
  const two = (n) => String(n).padStart(2, "0");
  return (h ? h + "-" : "") + two(m) + "-" + two(s);
}

function say(btn, text) {
  btn.textContent = text;
  clearTimeout(btn._tbsTimer);
  btn._tbsTimer = setTimeout(() => (btn.textContent = LABEL), 2500);
}

async function shoot(btn) {
  const v = video();
  if (!v || !v.videoWidth) return say(btn, "아직 재생 중인 영상이 없습니다");
  const name = `${title()} ${at(v)}.png`;
  try {
    // 영상 한 장면을 그대로 그린다 (플레이어 단추나 자막 없이)
    const c = document.createElement("canvas");
    c.width = v.videoWidth;
    c.height = v.videoHeight;
    c.getContext("2d").drawImage(v, 0, 0, c.width, c.height);
    await tbs.request("saveFile", { name, base64: c.toDataURL("image/png") });
    say(btn, "저장했습니다 (다운로드 폴더)");
  } catch (e) {
    // 영상을 그릴 수 없으면 탭 화면을 통째로 찍는다
    try {
      await tbs.request("screenshot", {});
      say(btn, "탭 화면을 저장했습니다");
    } catch (e2) {
      say(btn, "저장하지 못했습니다: " + e2.message);
    }
  }
}

function ensure() {
  if (location.pathname !== "/watch") return;
  if (document.getElementById(BAR_ID)) return;
  const below = document.querySelector("ytd-watch-flexy #below");
  if (!below) return;
  const bar = document.createElement("div");
  bar.id = BAR_ID;
  const btn = document.createElement("button");
  btn.type = "button";
  btn.textContent = LABEL;
  btn.addEventListener("click", () => shoot(btn));
  bar.appendChild(btn);
  below.prepend(bar);
}

// 유튜브는 페이지를 새로 불러오지 않고 화면만 바꾼다. 바뀔 때마다 버튼이 있는지 본다.
document.addEventListener("yt-navigate-finish", ensure);
setInterval(ensure, 1000);
ensure();
