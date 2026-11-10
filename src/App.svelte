<script lang="ts">
  // 브라우저 화면: 사이드바, 상단 툴바, 페이지 카드 (spec 4.1, 시안 F).
  // 디자인 값은 styles/tokens.css에만 있다.
  import { onMount, tick } from "svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import CollapsedSidebar from "./lib/CollapsedSidebar.svelte";
  import NewTab from "./lib/NewTab.svelte";
  import PermissionBar from "./lib/PermissionBar.svelte";
  import Toolbar from "./lib/Toolbar.svelte";
  import ContextMenu from "./lib/ContextMenu.svelte";
  import HistoryPage from "./lib/pages/HistoryPage.svelte";
  import DownloadsPage from "./lib/pages/DownloadsPage.svelte";
  import SettingsPage from "./lib/pages/SettingsPage.svelte";
  import ScriptSettingsPage from "./lib/pages/ScriptSettingsPage.svelte";
  import StorePage from "./lib/pages/StorePage.svelte";
  import {
    host,
    host4,
    on,
    onTabsChanged,
    onTabUpdated,
    onBookmarksChanged,
    onMaxButtonHover,
    onWindowMaximized,
    shortcutName,
    host5,
    host7,
    host8,
    host9,
    type ExtInfo,
    type PermissionRequest,
    type ScriptInfo,
    type Download,
    type Settings,
  } from "./lib/api";
  import { ui, report, activeTab, setSidebarCollapsed, runPopupMenuItem, showInstall, cancelInstall } from "./lib/store.svelte";

  let maxHover = $state(false);
  let maximized = $state(false);
  let card: HTMLElement;
  let maxButton: HTMLButtonElement | null = null;
  // 전체 화면에서는 사라지므로 없을 수 있다
  let toolbar = $state<Toolbar>();
  let sidebar = $state<Sidebar>();
  let newTabPage = $state<NewTab>();

  const active = $derived(activeTab());
  // 지금 보는 탭이 기다리는 권한 질문 (spec 5.5)
  const permission = $derived(ui.permissions.find((p) => p.tab === ui.activeId) ?? null);

  // 새 탭은 새 탭 화면을 띄우고 검색 칸에 포커스를 준다 (spec 5.1)
  async function newTab() {
    await host.tabCreate().catch(report);
    focusNewTab();
  }

  async function focusNewTab() {
    await tick();
    if (newTabPage) newTabPage.focus();
    else toolbar?.focusAddress();
  }

  // 탭 검색: 접혀 있으면 펼친 뒤 검색 칸으로 (spec 4.6)
  async function focusTabSearch() {
    if (ui.sidebarCollapsed) await setSidebarCollapsed(false);
    await tick();
    sidebar?.focusSearch();
  }

  // ───── 테마 (spec 4.3, 5.4): 라이트, 다크, 시스템 따름 ─────
  const systemDark = window.matchMedia("(prefers-color-scheme: dark)");
  let osDark = $state(systemDark.matches);
  const themeSetting = $derived(ui.settings?.theme ?? "light");
  const dark = $derived(themeSetting === "dark" || (themeSetting === "system" && osDark));

  $effect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
    // 웹 페이지의 prefers-color-scheme과 웹뷰 빈 바탕을 같은 색으로 (흰 화면이 번쩍이지 않게)
    const bg = getComputedStyle(document.documentElement).getPropertyValue("--card-bg").trim();
    const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(bg);
    const rgb: [number, number, number] = m ? [parseInt(m[1], 16), parseInt(m[2], 16), parseInt(m[3], 16)] : [255, 255, 255];
    host7.applyTheme(dark, themeSetting === "system", rgb).catch(report);
  });

  // UI를 누르면 떠 있는 팝업(메뉴, 북마크 목록, 다운로드)을 닫는다. 주소창 자동완성은 주소창이 알아서 닫는다.
  // 설치·권한 확인은 바깥을 눌러도 닫지 않는다 (시안 M)
  // 마우스 옆 버튼: 새 탭 화면·내부 페이지·사이드바 위에서도 지금 탭을 뒤로·앞으로
  function onSideButton(e: MouseEvent) {
    if (e.button !== 3 && e.button !== 4) return;
    e.preventDefault();
    const tab = activeTab();
    if (!tab) return;
    (e.button === 3 ? host.goBack(tab.id) : host.goForward(tab.id)).catch(report);
  }

  function onPointerDown(e: PointerEvent) {
    if (ui.install) return;
    if (!(e.target as Element).closest?.(".address")) host4.popupHide().catch(() => {});
  }

  // 페이지 카드와 최대화 버튼 자리를 호스트에 알린다 (창 크기가 바뀔 때마다)
  function sendLayout() {
    if (card) {
      const radius = parseFloat(getComputedStyle(card).borderTopLeftRadius) || 0;
      host.setContentBounds(card.getBoundingClientRect(), radius).catch(report);
    }
    if (maxButton && !ui.fullscreen) host.setMaxButtonBounds(maxButton.getBoundingClientRect()).catch(report);
  }

  // UI에서 처리하는 단축키 (spec 5.6). 나머지는 호스트에 넘긴다.
  function local(name: string): boolean {
    if (name === "focus_address") toolbar?.focusAddress();
    else if (name === "focus_tab_search") focusTabSearch();
    else if (name === "focus_new_tab") focusNewTab();
    else if (name === "toggle_sidebar") setSidebarCollapsed(!ui.sidebarCollapsed);
    else return false;
    return true;
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape" && ui.install) {
      e.preventDefault();
      return cancelInstall();
    }
    const name = shortcutName(e);
    if (!name) return;
    e.preventDefault();
    if (!local(name)) host4.runShortcut(name).catch(report);
  }

  onMount(() => {
    const unlisten = [
      onTabsChanged((s) => {
        ui.tabs = s.tabs;
        ui.groups = s.groups;
        ui.activeId = s.active;
      }),
      onTabUpdated((t) => {
        ui.tabs = ui.tabs.map((x) => (x.id === t.id ? t : x));
      }),
      onBookmarksChanged((b) => (ui.bookmarks = b)),
      onMaxButtonHover((h) => (maxHover = h)),
      onWindowMaximized((m) => {
        maximized = m;
        requestAnimationFrame(sendLayout);
      }),
      on<Settings>("settings_changed", (s) => (ui.settings = s)),
      on<Download[]>("downloads_changed", (d) => (ui.downloads = d)),
      on<Download>("download_progress", (d) => (ui.downloads = ui.downloads.map((x) => (x.id === d.id ? d : x)))),
      on<string>("shortcut", (name) => local(name)),
      on<ScriptInfo[]>("scripts_changed", (l) => (ui.scripts = l)),
      on<boolean>("fullscreen", (f) => {
        ui.fullscreen = f;
        tick().then(sendLayout);
      }),
      on<boolean>("sidebar_collapsed", (c) => (ui.sidebarCollapsed = c)),
      on<PermissionRequest[]>("permission_requests", (r) => (ui.permissions = r)),
      on<number>("popup_menu_picked", (i) => runPopupMenuItem(i)),
      on<string>("update_available", (v) => (ui.update = v)),
      on<ExtInfo[]>("extensions_changed", (l) => (ui.extensions = l)),
      on<ExtInfo>("install_ready", (info) => showInstall(info)),
      on<string>("notice", (m) => (ui.error = m)),
      // 설치 확인 창이 답 없이 닫히면(창을 옮김 등) 취소로 본다
      on<null>("popup_closed", () => cancelInstall()),
      on<number>("update_progress", (f) => {
        if (f >= 0) ui.error = `새 버전 ${ui.update}을(를) 내려받는 중입니다 (${Math.round(f * 100)}%). 다 받으면 설치하고 다시 시작합니다.`;
      }),
    ];
    const onOsTheme = (e: MediaQueryListEvent) => (osDark = e.matches);
    systemDark.addEventListener("change", onOsTheme);
    host.getState().then((s) => {
      ui.tabs = s.tabs;
      ui.groups = s.groups;
      ui.activeId = s.active;
      if (!activeTab()?.url) focusNewTab();
    }, report);
    host.bookmarksGet().then((b) => (ui.bookmarks = b), report);
    host4.settingsGet().then((s) => (ui.settings = s), report);
    host4.downloadsGet().then((d) => (ui.downloads = d), report);
    host4.fullscreenState().then((f) => (ui.fullscreen = f), report);
    host5.scriptsList().then((l) => (ui.scripts = l), report);
    host7.sidebarGet().then((c) => (ui.sidebarCollapsed = c), report);
    host7.permissionRequests().then((r) => (ui.permissions = r), report);
    host8.updateGet().then((v) => (ui.update = v), report);
    host9.extList().then((l) => (ui.extensions = l), report);
    host.takeNotices().then((n) => n.length && (ui.error = n.join("\n")), report);
    const ro = new ResizeObserver(sendLayout);
    ro.observe(card);
    ro.observe(document.body);
    sendLayout();
    return () => {
      systemDark.removeEventListener("change", onOsTheme);
      ro.disconnect();
      unlisten.forEach((p) => p.then((f) => f()));
    };
  });
</script>

<svelte:window onkeydown={onKeydown} onpointerdowncapture={onPointerDown} onmousedown={(e) => (e.button === 3 || e.button === 4) && e.preventDefault()} onmouseup={onSideButton} />

<div class="window" class:dragging={ui.drag} class:fullscreen={ui.fullscreen}>
  {#if ui.fullscreen}
    <!-- 전체 화면에서는 사이드바와 툴바를 숨긴다 (spec 5.5) -->
  {:else if ui.sidebarCollapsed}
    <CollapsedSidebar onNewTab={newTab} onSearch={focusTabSearch} />
  {:else}
    <Sidebar bind:this={sidebar} onNewTab={newTab} />
  {/if}

  <main class="main">
    {#if !ui.fullscreen}
      <Toolbar
        bind:this={toolbar}
        {maxHover}
        {maximized}
        onMaxButton={(el) => {
          maxButton = el;
          sendLayout();
        }}
      />
    {/if}
    {#if permission}
      <PermissionBar request={permission} />
    {/if}
    {#if ui.error && (ui.sidebarCollapsed || ui.fullscreen)}
      <!-- 알림은 평소 사이드바에 뜬다. 사이드바가 좁거나 숨었을 때는 여기에 -->
      <p class="notice" role="alert">
        <span>{ui.error}</span>
        <button class="notice-close" aria-label="알림 닫기" onclick={() => (ui.error = "")}>×</button>
      </p>
    {/if}
    <!-- 웹 페이지(콘텐츠 웹뷰)가 이 카드 자리에 놓인다. 내부 페이지와 새 탭 화면은 UI가 직접 그린다 -->
    <div class="card" bind:this={card}>
      {#if active?.url === "tbs://history"}
        <HistoryPage />
      {:else if active?.url === "tbs://downloads"}
        <DownloadsPage />
      {:else if active?.url === "tbs://settings"}
        <SettingsPage />
      {:else if active?.url === "tbs://scripts"}
        <StorePage />
      {:else if active?.url.startsWith("tbs://scripts/")}
        <ScriptSettingsPage id={decodeURIComponent(active.url.slice("tbs://scripts/".length))} />
      {:else if active && !active.url}
        {#key active.id}
          <NewTab bind:this={newTabPage} />
        {/key}
      {/if}
    </div>
  </main>
</div>

<ContextMenu />

{#if ui.drag && !ui.sidebarCollapsed}
  <!-- 접힌 상태에서는 이름표가 페이지 뒤에 가려지므로 그리지 않는다 (끄는 파비콘이 흐려지는 것으로 보인다) -->
  <div class="drag-label" style:left="{ui.drag.x + 12}px" style:top="{ui.drag.y + 8}px">{ui.drag.label}</div>
{/if}

<style>
  .window {
    display: flex;
    height: 100%;
    padding: var(--window-padding);
  }
  .window.dragging {
    cursor: grabbing;
  }
  /* F11 전체 화면: 페이지가 창 전체를 채운다 */
  .window.fullscreen {
    padding: 0;
  }
  .window.fullscreen .card {
    border-radius: 0;
    box-shadow: none;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: var(--main-gap);
  }
  .card {
    position: relative;
    flex: 1;
    min-width: 0;
    background: var(--card-bg);
    border-radius: var(--card-radius);
    box-shadow: var(--card-shadow);
    overflow: hidden;
  }
  .notice {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--prompt-gap);
    min-height: var(--prompt-height);
    margin: 0;
    padding: var(--prompt-padding);
    background: var(--menu-bg);
    border-radius: var(--menu-radius);
    box-shadow: var(--menu-shadow);
    user-select: text;
    white-space: pre-line;
  }
  .notice span {
    flex: 1;
  }
  .notice-close {
    flex: none;
    width: var(--prompt-button-height);
    height: var(--prompt-button-height);
    text-align: center;
    color: var(--fg-muted);
  }
  .drag-label {
    position: fixed;
    z-index: 30;
    max-width: 200px;
    padding: 4px 8px;
    border-radius: var(--menu-item-radius);
    background: var(--menu-bg);
    box-shadow: var(--menu-shadow);
    font-size: var(--search-font-size);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    pointer-events: none;
  }
</style>
