// 체스닷컴 기물 바꾸기 (spec 6.5). 기물마다 고른 그림으로 바꾼다. 고르지 않은 기물은 원래 그림 그대로.
// 스타일 덮어쓰기 방식: 체스닷컴은 기물을 .piece.wp 같은 요소의 배경 그림으로 그린다.

const KEYS = ["wk", "wq", "wr", "wb", "wn", "wp", "bk", "bq", "br", "bb", "bn", "bp"];
const style = document.createElement("style");
style.dataset.tbsChessPieces = "";

function apply(s) {
  style.textContent = KEYS.filter((k) => s[k])
    .map((k) => `.piece.${k}, .promotion-piece.${k} { background-image: url("${s[k]}") !important; }`)
    .join("\n");
  if (!style.isConnected) (document.head || document.documentElement).appendChild(style);
}

apply(tbs.settings);
tbs.onSettingsChanged(apply);
