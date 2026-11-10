// TB's Browser 확장 호환 (spec 6.6): 확장 페이지(팝업, 설정)에 넣는다. 설치할 때 HTML의 맨 앞에 끼운다.
// WebView2는 웹뷰마다 따로 "창 하나에 탭 하나"로 보여서, 팝업이 "지금 창의 활성 탭"을 물으면 자기 자신이 나온다.
// 그래서 그런 물음에는 TB's Browser가 지금 보여 주는 탭(주소·제목으로 찾은 실제 탭)을 돌려준다.
(() => {
  const c = globalThis.chrome;
  if (!c || !c.tabs || !c.webview || c.tabs.__tbs) return;
  const origQuery = c.tabs.query.bind(c.tabs);
  let seq = 0;
  const waiting = new Map();
  c.webview.addEventListener("message", (e) => {
    const d = e.data;
    if (d && typeof d === "object" && d.tbsReply && waiting.has(d.tbsReply)) {
      waiting.get(d.tbsReply)(d.tab || null);
      waiting.delete(d.tbsReply);
    }
  });
  // 호스트에 지금 보는 탭의 주소와 제목을 묻는다 (1초 안에 답이 없으면 없는 것으로)
  function askActive() {
    return new Promise((resolve) => {
      const id = ++seq;
      waiting.set(id, resolve);
      c.webview.postMessage(JSON.stringify({ tbs: "activeTab", id }));
      setTimeout(() => {
        if (waiting.has(id)) {
          waiting.delete(id);
          resolve(null);
        }
      }, 1000);
    });
  }
  async function activeTab() {
    const want = await askActive();
    if (!want || !want.url) return null;
    const all = await origQuery({});
    const same = all.filter((t) => t.url === want.url);
    return same.find((t) => t.title === want.title) || same[0] || null;
  }
  const asksActive = (q) => q && (q.active || q.currentWindow || q.lastFocusedWindow || q.highlighted) && q.windowId === undefined && !q.url && !q.title;
  function wrap(run, args, cb) {
    const p = run();
    if (typeof cb === "function") {
      p.then(cb, () => cb(undefined));
      return undefined;
    }
    return p;
  }
  c.tabs.query = function (q, cb) {
    return wrap(async () => {
      if (asksActive(q)) {
        const t = await activeTab();
        if (t) return [t];
      }
      return origQuery(q);
    }, arguments, cb);
  };
  if (c.windows) {
    for (const name of ["getCurrent", "getLastFocused"]) {
      const orig = c.windows[name] && c.windows[name].bind(c.windows);
      if (!orig) continue;
      c.windows[name] = function (opts, cb) {
        if (typeof opts === "function") [cb, opts] = [opts, undefined];
        return wrap(async () => {
          const t = await activeTab();
          if (!t) return orig(opts || {});
          const w = await c.windows.get(t.windowId, opts || {});
          return { ...w, focused: true };
        }, arguments, cb);
      };
    }
  }
  // 새 탭 열기: WebView2에서는 아무 일도 일어나지 않으므로 TB's Browser에 연다
  const abs = (u) => new URL(u || "", c.runtime.getURL("/")).href;
  const open = (url) => c.webview.postMessage(JSON.stringify({ tbs: "openTab", url: abs(url) }));
  c.tabs.create = function (props, cb) {
    open(props && props.url);
    const tab = { id: -1, url: abs(props && props.url), active: true };
    if (typeof cb === "function") return void cb(tab);
    return Promise.resolve(tab);
  };
  if (c.runtime && c.runtime.openOptionsPage) {
    c.runtime.openOptionsPage = function (cb) {
      const m = c.runtime.getManifest();
      open((m.options_ui && m.options_ui.page) || m.options_page || "options.html");
      if (typeof cb === "function") return void cb();
      return Promise.resolve();
    };
  }
  c.tabs.__tbs = true;
})();
