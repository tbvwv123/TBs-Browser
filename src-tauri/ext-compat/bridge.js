// TB's Browser 확장 호환 (spec 6.6): 보이지 않는 웹뷰에 띄우는 확장 페이지.
// 확장이 만든 오른쪽 클릭 메뉴 항목을 TB's Browser에 알리고, 누른 항목을 확장에 전한다.
(() => {
  const c = globalThis.chrome;
  const KEY = "__tbs_menus";
  const send = (msg) => c.webview.postMessage(JSON.stringify(msg));
  const report = () => c.storage.local.get(KEY).then((r) => send({ tbs: "menus", items: (r && r[KEY]) || [] }));
  c.storage.onChanged.addListener((changes, area) => {
    if (area === "local" && changes[KEY]) send({ tbs: "menus", items: changes[KEY].newValue || [] });
  });
  // 서비스 워커를 깨워 확장이 처음 메뉴를 만들게 한다 (만든 뒤에는 저장소 변화로 알게 된다)
  c.runtime.sendMessage({ __tbs: "wake" }).catch(() => {});
  report();
  // 서비스 워커가 맡긴 부탁(새 탭 열기 등)을 호스트에 전한다. 이 확장 자신이 보낸 것만
  c.runtime.onMessage.addListener((m, sender) => {
    if (m && m.__tbs === "relay" && sender && sender.id === c.runtime.id && !sender.tab && m.msg && m.msg.tbs === "openTab") send(m.msg);
  });
  c.webview.addEventListener("message", async (e) => {
    const d = e.data;
    if (!d || typeof d !== "object" || !d.click) return;
    const { info, pageUrl, pageTitle } = d.click;
    let tab;
    try {
      const all = await c.tabs.query({});
      const same = all.filter((t) => t.url === pageUrl);
      tab = same.find((t) => t.title === pageTitle) || same[0];
    } catch (err) {
      tab = undefined;
    }
    c.runtime.sendMessage({ __tbs: "click", info, tab }).catch(() => {});
  });
})();
