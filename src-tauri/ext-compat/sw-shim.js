// TB's Browser 확장 호환 (spec 6.6): 확장의 백그라운드(서비스 워커)보다 먼저 돈다.
// WebView2는 chrome.contextMenus 항목을 그리지 않는다. 만든 항목을 chrome.storage.local에 적어 두면
// 브리지 페이지(tbs-compat/bridge.html)가 TB's Browser에 알리고, 우리가 페이지의 오른쪽 클릭 메뉴에 넣는다.
// 항목을 누르면 브리지가 {__tbs: "click"}을 보내고, 여기서 확장이 걸어 둔 처리기를 부른다.
(() => {
  const c = globalThis.chrome;
  const cm = c && c.contextMenus;
  if (!cm || cm.__tbs) return;
  const KEY = "__tbs_menus";
  const items = new Map();
  const onclicks = new Map();
  const listeners = [];
  let auto = 0;
  const keep = ["title", "contexts", "documentUrlPatterns", "targetUrlPatterns", "parentId", "type", "checked", "enabled", "visible"];
  const strip = (p) => Object.fromEntries(keep.filter((k) => p[k] !== undefined).map((k) => [k, k === "parentId" ? String(p[k]) : p[k]]));
  const ready = c.storage.local.get(KEY).then((r) => {
    for (const it of (r && r[KEY]) || []) if (!items.has(it.id)) items.set(it.id, it);
  });
  const save = () => ready.then(() => c.storage.local.set({ [KEY]: [...items.values()] })).catch(() => {});
  const call = (orig, args) => {
    try {
      return orig(...args);
    } catch (e) {
      return undefined;
    }
  };
  const create = cm.create.bind(cm);
  const update = cm.update.bind(cm);
  const remove = cm.remove.bind(cm);
  const removeAll = cm.removeAll.bind(cm);
  cm.create = function (props, cb) {
    const id = String(props.id ?? `tbs-auto-${++auto}`);
    items.set(id, { ...strip(props), id });
    if (typeof props.onclick === "function") onclicks.set(id, props.onclick);
    save();
    const r = call(create, [props, cb]);
    return r === undefined ? props.id ?? id : r;
  };
  cm.update = function (id, props, cb) {
    const it = items.get(String(id));
    if (it) Object.assign(it, strip(props));
    if (typeof props.onclick === "function") onclicks.set(String(id), props.onclick);
    save();
    return call(update, [id, props, cb]);
  };
  cm.remove = function (id, cb) {
    items.delete(String(id));
    for (const [k, v] of items) if (v.parentId === String(id)) items.delete(k);
    save();
    return call(remove, [id, cb]);
  };
  cm.removeAll = function (cb) {
    items.clear();
    save();
    return call(removeAll, [cb]);
  };
  const addListener = cm.onClicked.addListener.bind(cm.onClicked);
  cm.onClicked.addListener = function (f) {
    listeners.push(f);
    return addListener(f);
  };
  // 브리지를 거쳐 TB's Browser에 부탁한다 (서비스 워커는 호스트에 직접 말할 수 없다)
  const relay = (msg) => c.runtime.sendMessage({ __tbs: "relay", msg }).catch(() => {});
  const abs = (u) => new URL(u || "", c.runtime.getURL("/")).href;
  if (c.tabs) {
    c.tabs.create = function (props, cb) {
      relay({ tbs: "openTab", url: abs(props && props.url) });
      const tab = { id: -1, url: abs(props && props.url), active: true };
      if (typeof cb === "function") return void cb(tab);
      return Promise.resolve(tab);
    };
  }
  if (c.runtime.openOptionsPage) {
    c.runtime.openOptionsPage = function (cb) {
      const m = c.runtime.getManifest();
      relay({ tbs: "openTab", url: abs((m.options_ui && m.options_ui.page) || m.options_page || "options.html") });
      if (typeof cb === "function") return void cb();
      return Promise.resolve();
    };
  }
  c.runtime.onMessage.addListener((m, sender, reply) => {
    // 이 확장의 브리지 페이지가 보낸 것만 받는다
    if (!m || !m.__tbs || !sender || !String(sender.url || "").startsWith(c.runtime.getURL("tbs-compat/"))) return;
    if (m.__tbs === "click") {
      ready.then(() => {
        const it = items.get(String(m.info.menuItemId));
        if (it && it.type === "checkbox") it.checked = !it.checked;
        for (const f of listeners) {
          try {
            f(m.info, m.tab);
          } catch (e) {
            console.error(e);
          }
        }
        const oc = onclicks.get(String(m.info.menuItemId));
        if (oc) oc(m.info, m.tab);
        save();
        reply(true);
      });
      return true;
    }
  });
  cm.__tbs = true;
})();
