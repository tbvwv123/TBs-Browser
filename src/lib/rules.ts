// 호스트에도 같은 것이 있는 규칙. 둘 다 tests/rules-vectors.json으로 시험한다 (npm run test:ui, npm run test:host).
// Node가 바로 읽을 수 있게 import를 두지 않는다.

/** 브라우저 단축키 이름 (spec 5.6). 호스트는 shortcuts::name (`shortcuts`의 host·ui 칸) */
export function shortcutName(e: { key: string; ctrlKey: boolean; shiftKey: boolean; altKey: boolean }): string | null {
  const k = e.key.length === 1 ? e.key.toUpperCase() : e.key;
  const c = e.ctrlKey, s = e.shiftKey, a = e.altKey;
  if (c && !s && !a && k === "T") return "new_tab";
  if (c && !s && !a && k === "W") return "close_tab";
  if (c && s && !a && k === "T") return "reopen_tab";
  if (c && !a && k === "Tab") return s ? "prev_tab" : "next_tab";
  if (c && !s && !a && k === "L") return "focus_address";
  if ((c && !s && !a && k === "R") || (!c && !s && !a && k === "F5")) return "reload";
  if (!c && !s && a && k === "ArrowLeft") return "back";
  if (!c && !s && a && k === "ArrowRight") return "forward";
  if (c && !s && !a && k === "D") return "bookmark";
  if (c && !s && !a && k === "H") return "history";
  if (c && !s && !a && k === "J") return "downloads";
  if (c && !s && !a && k === "B") return "toggle_sidebar";
  if (c && s && !a && k === "A") return "focus_tab_search";
  if (c && !s && !a && k === "F") return "find";
  if (!c && !s && !a && k === "F11") return "fullscreen";
  if (!c && !s && !a && k === "F12") return "devtools";
  return null;
}

/** `*`는 아무 글자나 (spec 6.1). 호스트는 scripts::pattern_matches (`patterns`) */
export function patternMatches(pattern: string, text: string): boolean {
  const re = new RegExp("^" + pattern.split("*").map((x) => x.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*") + "$");
  return re.test(text);
}

/** http(s) 주소면 URL, 아니면 null */
function webUrl(url: string): URL | null {
  try {
    const u = new URL(url);
    return u.protocol === "http:" || u.protocol === "https:" ? u : null;
  } catch {
    return null;
  }
}

/** 크롬 확장의 주소 패턴 ("https://*.dcinside.com/*", "<all_urls>")이 주소에 맞는지. 호스트는 ext_compat::chrome_matches (`chromePatterns`) */
export function chromeMatches(pattern: string, url: string): boolean {
  const u = webUrl(url);
  if (!u) return false;
  if (pattern === "<all_urls>") return true;
  const m = /^(\*|https?):\/\/([^/]*)(\/.*)?$/.exec(pattern);
  if (!m) return false;
  const [, scheme, host, path = "/*"] = m;
  if (scheme !== "*" && scheme + ":" !== u.protocol) return false;
  if (host !== "*") {
    if (host.startsWith("*.")) {
      const base = host.slice(2);
      if (u.hostname !== base && !u.hostname.endsWith("." + base)) return false;
    } else if (u.hostname !== host) return false;
  }
  // 빈 쿼리("/?")는 search가 비어 있어도 물음표를 남긴다 (호스트와 같게)
  const query = u.search || (/^[^#]*\?/.test(u.href) ? "?" : "");
  return patternMatches(path, u.pathname + query);
}

/** 광고 차단에서 쓰는 사이트 이름 ("www."를 뺀 호스트). 웹 페이지가 아니면 null. 호스트는 adblock::host_of (`adblockSites`) */
export function adblockSite(url: string): string | null {
  return webUrl(url)?.hostname.replace(/^(www\.)+/, "") ?? null;
}
