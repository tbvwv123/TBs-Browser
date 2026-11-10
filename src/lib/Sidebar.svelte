<script lang="ts">
  // 사이드바 (spec 4.5). 맨 위 줄과 맨 아래 줄은 고정, 그 사이(북마크 + 탭)는 하나의 스크롤 영역이다.
  import { tick } from "svelte";
  import BookmarkNode from "./BookmarkNode.svelte";
  import TabRow from "./TabRow.svelte";
  import RenameInput from "./RenameInput.svelte";
  import ExtensionsPanel from "./ExtensionsPanel.svelte";
  import { host, host9, type Group, type Tab } from "./api";
  import { ui, report, moreMenuItems, openMenu, openMenuAbove, activeTab, setSidebarCollapsed, pinnedItems, token, type Pinned } from "./store.svelte";
  import { dropMark, consumeDragClick } from "./drag";

  let { onNewTab }: { onNewTab: () => void } = $props();

  let list: HTMLDivElement;
  let tabsHead: HTMLDivElement;
  let searchInput: HTMLInputElement;
  let stuck = $state(false);
  let thumb = $state({ top: 0, height: 100, on: false });

  type Block = { kind: "tab"; tab: Tab } | { kind: "group"; group: Group; tabs: Tab[] };

  // 탭 검색: 제목과 주소에 그 글자가 들어간 탭만 남긴다 (spec 5.1)
  const query = $derived(ui.search.trim().toLowerCase());
  const visibleTabs = $derived(
    query ? ui.tabs.filter((t) => t.title.toLowerCase().includes(query) || t.url.toLowerCase().includes(query)) : ui.tabs,
  );
  const blocks = $derived.by(() => {
    const out: Block[] = [];
    for (const t of visibleTabs) {
      const g = t.group === null ? null : ui.groups.find((x) => x.id === t.group);
      if (!g) out.push({ kind: "tab", tab: t });
      else {
        const last = out[out.length - 1];
        if (last?.kind === "group" && last.group.id === g.id) last.tabs.push(t);
        else out.push({ kind: "group", group: g, tabs: [t] });
      }
    }
    return out;
  });

  // 스크롤 막대와 붙는 머리줄 아래 선 (시안 F의 measure)
  function measure() {
    if (!list || !list.scrollHeight) return;
    const h = (list.clientHeight / list.scrollHeight) * 100;
    thumb = { top: (list.scrollTop / list.scrollHeight) * 100, height: h, on: h < 99.5 };
    stuck = list.scrollTop > 0 && tabsHead.getBoundingClientRect().top - list.getBoundingClientRect().top < 1;
  }

  $effect(() => {
    // 목록이 바뀌면 다시 잰다
    void [ui.tabs.length, ui.groups, ui.bookmarks, ui.search];
    tick().then(measure);
  });

  // 탭을 바꾸면 그 탭이 보이도록 목록이 따라간다 (spec 5.1)
  $effect(() => {
    const id = ui.activeId;
    if (id === null) return;
    tick().then(() => list?.querySelector(`[data-tab-id="${id}"]`)?.scrollIntoView({ block: "nearest" }));
  });

  $effect(() => {
    const ro = new ResizeObserver(measure);
    ro.observe(list);
    return () => ro.disconnect();
  });

  function toggleSection() {
    if (consumeDragClick()) return;
    host.bookmarksSetCollapsed(!ui.bookmarks.collapsed).catch(report);
  }

  // 북마크 머리줄의 추가 버튼: 지금 보는 페이지를 북마크 맨 위 목록 끝에 넣는다 (spec 5.3)
  function addCurrent() {
    const t = activeTab();
    if (!t?.url) return;
    host.bookmarkAdd(null, null, t.title, t.url, t.favicon).catch(report);
  }

  async function newRootFolder() {
    const id = await host.bookmarkAddFolder(null, null, "새 폴더");
    if (ui.bookmarks.collapsed) await host.bookmarksSetCollapsed(false);
    ui.renaming = { kind: "bookmark", id };
  }

  function groupMenu(e: MouseEvent, g: Group) {
    openMenu(e, [
      { label: "이름 바꾸기", run: () => (ui.renaming = { kind: "group", id: g.id }) },
      { label: g.collapsed ? "펼치기" : "접기", run: () => host.groupSetCollapsed(g.id, !g.collapsed).catch(report) },
      { label: "그룹 풀기", run: () => host.groupUngroup(g.id).catch(report) },
    ]);
  }

  function toggleGroup(g: Group) {
    if (consumeDragClick()) return;
    host.groupSetCollapsed(g.id, !g.collapsed).catch(report);
  }

  function onSearchKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      ui.search = "";
      searchInput.blur();
    }
    // Enter: 남은 탭 중 첫 탭으로 간다
    if (e.key === "Enter" && visibleTabs.length) {
      host.tabActivate(visibleTabs[0].id).catch(report);
      ui.search = "";
      searchInput.blur();
    }
  }

  export function focusSearch() {
    searchInput.focus();
    searchInput.select();
  }

  // 점 3개 메뉴: 다운로드, 방문 기록, 설정 (spec 4.5)
  function moreMenu(e: MouseEvent) {
    openMenuAbove(e.currentTarget as HTMLElement, moreMenuItems());
  }

  // 고정 아이콘: 4개까지 보이고 넘는 것은 확장 버튼이 +N으로 보인다 (시안 K)
  const pinned = $derived(pinnedItems());
  const pinnedMax = $derived(token("--ext-pinned-max") || 4);
  const shownPinned = $derived(pinned.slice(0, pinnedMax));
  const hiddenPinned = $derived(pinned.length - shownPinned.length);
  let panelEl = $state<HTMLElement>();

  // 고정 아이콘을 누르면: 팝업이 있는 크롬 확장은 그 팝업, 나머지는 확장 패널
  function openPinned(e: MouseEvent, p: Pinned) {
    if (p.kind === "ext" && p.popup && p.enabled) {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      host9.extOpenPopup(p.id, r.left, r.top - 6).catch(report);
    } else ui.scriptsOpen = !ui.scriptsOpen;
  }

  // 확장 패널에서 연 확장 팝업은 패널 오른쪽, 페이지 위에 띄운다
  function panelAnchor() {
    const r = panelEl?.getBoundingClientRect();
    return r ? { x: r.right + 8, bottom: r.bottom } : { x: 270, bottom: window.innerHeight - 48 };
  }

  const bmMark = $derived(dropMark("bookmarks-head", ""));
  const tabsMark = $derived(dropMark("tabs-head", ""));
</script>

<nav class="sidebar" aria-label="사이드바">
  <div class="top-row">
    <button class="ib top-button" aria-label="사이드바 접기" title="사이드바 접기 (Ctrl+B)" onclick={() => setSidebarCollapsed(true)}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="3"></rect><path d="M9 4v16"></path></svg>
    </button>
    <label class="search">
      <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6"></circle><path d="M20 20l-4.5-4.5"></path></svg>
      <input
        class="search-input"
        bind:this={searchInput}
        bind:value={ui.search}
        onkeydown={onSearchKey}
        placeholder="탭 검색"
        aria-label="탭 검색"
        spellcheck="false"
        autocomplete="off"
      />
    </label>
  </div>

  <div class="list-wrap">
    <div class="list" bind:this={list} onscroll={measure}>
      {#if !query}
        <!-- 북마크 구역 (spec 4.5): 머리줄을 누르면 구역 전체가 접힌다 -->
        <div
          class="section-head"
          class:drop-target={bmMark}
          data-drop="bookmarks-head"
          role="presentation"
          oncontextmenu={(e) => openMenu(e, [{ label: "새 폴더", run: () => newRootFolder().catch(report) }])}
        >
          <button class="section-toggle" aria-label="북마크 접기 또는 펼치기" aria-expanded={!ui.bookmarks.collapsed} onclick={toggleSection}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" style:transform="rotate({ui.bookmarks.collapsed ? -90 : 0}deg)"><path d="M6 9l6 6 6-6"></path></svg>
            <span>북마크</span>
          </button>
          <button class="ib section-button" aria-label="북마크 추가" disabled={!activeTab()?.url} onclick={addCurrent}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"></path></svg>
          </button>
        </div>
        {#if !ui.bookmarks.collapsed}
          <div class="bookmarks" role="tree" aria-label="북마크">
            {#each ui.bookmarks.items as node (node.id)}
              <BookmarkNode {node} parent={null} depth={0} />
            {/each}
            {#if ui.bookmarks.items.length === 0}
              <p class="hint">별 버튼이나 + 버튼으로 북마크를 추가하세요.</p>
            {/if}
          </div>
        {/if}
        <div class="divider in-list" aria-hidden="true"></div>
      {/if}

      <!-- "탭" 제목과 "새 탭" 줄은 스크롤 영역 맨 위에 붙는다 (spec 4.5) -->
      <div class="tabs-head" bind:this={tabsHead}>
        <span class="section-title">탭</span>
        <button class="row new-tab" class:drop-target={tabsMark} data-drop="tabs-head" onclick={onNewTab}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"></path></svg>
          <span>새 탭</span>
        </button>
        <div class="head-line" class:on={stuck} aria-hidden="true"></div>
      </div>

      {#each blocks as block (block.kind === "tab" ? `t${block.tab.id}` : `g${block.group.id}`)}
        {#if block.kind === "tab"}
          <TabRow tab={block.tab} nested={false} />
        {:else}
          {@const g = block.group}
          {@const gm = dropMark("group", g.id)}
          <div class="group">
            <!-- 그룹을 지나는 동안 그룹 이름 줄이 위에 붙는다 (spec 4.5) -->
            <div
              class="group-head"
              class:drop-before={gm === "before"}
              class:drop-target={gm === "inside"}
              data-drop="group"
              data-id={g.id}
              role="presentation"
              oncontextmenu={(e) => groupMenu(e, g)}
            >
              {#if ui.renaming?.kind === "group" && ui.renaming.id === g.id}
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6"></path></svg>
                <RenameInput
                  value={g.title}
                  onCommit={(v) => {
                    ui.renaming = null;
                    host.groupRename(g.id, v).catch(report);
                  }}
                  onCancel={() => (ui.renaming = null)}
                />
              {:else}
                <button class="group-toggle" aria-expanded={!g.collapsed || !!query} onclick={() => toggleGroup(g)}>
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                    {#if g.collapsed && !query}<path d="M9 6l6 6-6 6"></path>{:else}<path d="M6 9l6 6 6-6"></path>{/if}
                  </svg>
                  <span class="group-title">{g.title}</span>
                  {#if g.collapsed && !query}<span class="group-count">{block.tabs.length}</span>{/if}
                </button>
              {/if}
            </div>
            {#if !g.collapsed || query}
              {#each block.tabs as tab (tab.id)}
                <TabRow {tab} nested={true} />
              {/each}
            {/if}
          </div>
        {/if}
      {/each}

      {#if query && visibleTabs.length === 0}
        <p class="hint">"{ui.search}"이(가) 들어간 탭이 없습니다.</p>
      {/if}
    </div>
    <span
      class="thumb"
      aria-hidden="true"
      style:top="{thumb.top}%"
      style:height="{thumb.height}%"
      style:opacity={thumb.on ? 1 : 0}
    ></span>
  </div>

  {#if ui.error}
    <p class="error" role="alert">
      {ui.error}
      <button class="error-close" aria-label="알림 닫기" onclick={() => (ui.error = "")}>×</button>
    </p>
  {/if}

  {#if ui.scriptsOpen}
    <div class="panel-backdrop" role="presentation" onpointerdown={() => (ui.scriptsOpen = false)}></div>
    <div class="panel" bind:this={panelEl}>
      <ExtensionsPanel scripts={ui.scripts} extensions={ui.extensions} url={activeTab()?.url ?? ""} onError={report} popupAnchor={panelAnchor} />
    </div>
  {/if}

  <div class="divider" aria-hidden="true"></div>
  <div class="bottom-row">
    <!-- 고정한 사이트 스크립트 아이콘들, 그다음 확장 프로그램 버튼 (spec 4.5) -->
    {#each shownPinned as p (p.kind + p.id)}
      <button class="ib bottom-button pinned" class:off={!p.enabled} title={p.name} aria-label="{p.name} (고정됨)" onclick={(e) => openPinned(e, p)}>
        {#if p.icon}
          <img class="pin-icon" src={p.icon} alt="" />
        {:else}
          <span class="pin-badge">{p.name.slice(0, 1)}</span>
        {/if}
      </button>
    {/each}
    <button
      class="ib bottom-button"
      class:with-count={hiddenPinned > 0}
      class:hover={ui.scriptsOpen}
      aria-label={hiddenPinned > 0 ? `확장 프로그램, 고정한 것 ${hiddenPinned}개 더` : "확장 프로그램"}
      aria-expanded={ui.scriptsOpen}
      onclick={() => (ui.scriptsOpen = !ui.scriptsOpen)}
    >
      <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><path d="M4 8h4V7a2 2 0 1 1 4 0v1h4v4h1a2 2 0 1 1 0 4h-1v4h-4v-1a2 2 0 1 0-4 0v1H4z"></path></svg>
      {#if hiddenPinned > 0}<span class="more-count">+{hiddenPinned}</span>{/if}
    </button>
    <span class="spacer"></span>
    <button class="ib bottom-button" aria-label={ui.update ? "더 보기, 새 버전 있음" : "더 보기"} aria-haspopup="menu" aria-expanded={!!ui.menu?.above} onclick={moreMenu}>
      {#if ui.update}<span class="update-dot" aria-hidden="true"></span>{/if}
      <svg width="17" height="17" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="12" cy="5" r="1.7"></circle><circle cx="12" cy="12" r="1.7"></circle><circle cx="12" cy="19" r="1.7"></circle></svg>
    </button>
  </div>
</nav>

<style>
  .sidebar {
    position: relative;
    flex: none;
    width: var(--sidebar-width);
    display: flex;
    flex-direction: column;
    gap: var(--sidebar-gap);
    padding: var(--sidebar-padding);
    min-height: 0;
  }
  .top-row {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    height: var(--top-row-height);
  }
  .ib {
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--icon-button-radius);
    color: var(--fg);
  }
  .ib:hover:not(:disabled) {
    background: var(--hover-bg);
  }
  .ib:disabled {
    color: var(--fg-disabled);
    cursor: default;
  }
  .top-button {
    flex: none;
    width: var(--icon-button-size);
    height: var(--icon-button-size);
    color: var(--fg-muted);
  }
  .search {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: var(--search-height);
    padding: 0 10px;
    background: var(--field-bg);
    border-radius: var(--search-radius);
    color: var(--fg-muted);
    font-size: var(--search-font-size);
    cursor: text;
  }
  .search-input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: transparent;
    padding: 0;
    color: var(--fg);
    user-select: text;
  }
  .search-input::placeholder {
    color: var(--fg-muted);
  }

  .list-wrap {
    position: relative;
    flex: 1 1 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .list {
    flex: 1 1 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--tab-row-gap);
    margin: 0 -2px;
    padding: 0 12px 4px 2px;
    overflow-x: hidden;
    overflow-y: auto;
    scrollbar-width: none; /* 기본 스크롤바는 숨기고 얇은 막대를 그린다 (spec 4.2) */
  }
  .thumb {
    position: absolute;
    right: 0;
    width: var(--scrollbar-width);
    border-radius: var(--scrollbar-radius);
    background: var(--scrollbar);
    pointer-events: none;
  }

  .section-head {
    flex: none;
    display: flex;
    align-items: center;
    height: var(--section-title-height);
    padding-left: 4px;
    color: var(--fg-muted);
    border-radius: var(--bookmark-row-radius);
  }
  .section-toggle {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 6px;
    height: var(--section-title-height);
    font-size: var(--section-title-font-size);
    font-weight: var(--section-title-weight);
  }
  .section-button {
    width: var(--section-button-size);
    height: var(--section-button-size);
    border-radius: var(--bookmark-row-radius);
    color: inherit;
  }
  .bookmarks {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: var(--tab-row-gap);
    font-size: var(--bookmark-font-size);
    color: var(--fg-bookmark);
  }
  .hint {
    margin: 4px 10px;
    font-size: var(--search-font-size);
    color: var(--fg-muted);
  }
  .divider {
    flex: none;
    height: 1px;
    margin: 0 var(--divider-margin);
    background: var(--divider);
  }
  .divider.in-list {
    margin: var(--divider-margin);
  }

  .tabs-head {
    flex: none;
    position: sticky;
    top: 0;
    z-index: 2;
    display: flex;
    flex-direction: column;
    /* 창 바탕과 같은 그라디언트를 화면 기준으로 깔아 이음매가 보이지 않게 한다 */
    background-image: var(--window-bg);
    background-attachment: fixed;
  }
  .section-title {
    display: flex;
    align-items: center;
    height: var(--section-title-height);
    padding-left: 4px;
    font-size: var(--section-title-font-size);
    font-weight: var(--section-title-weight);
    color: var(--fg-muted);
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--tab-row-icon-gap);
    width: 100%;
    height: var(--tab-row-height);
    padding: var(--tab-row-padding);
    border-radius: var(--tab-row-radius);
  }
  .row:hover {
    background: var(--hover-bg);
  }
  .new-tab {
    color: var(--fg-muted);
  }
  .head-line {
    height: 1px;
    background: transparent;
  }
  .head-line.on {
    background: var(--divider);
  }

  .group {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: var(--tab-row-gap);
  }
  .group-head {
    position: sticky;
    top: var(--tabs-head-height);
    z-index: 1;
    display: flex;
    align-items: center;
    gap: var(--tab-row-icon-gap);
    height: var(--tab-row-height);
    padding: var(--tab-row-padding);
    background-image: var(--window-bg);
    background-attachment: fixed;
    color: var(--fg-muted);
    font-weight: 500;
    border-radius: var(--tab-row-radius);
  }
  .group-toggle {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--tab-row-icon-gap);
    height: 100%;
  }
  .group-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .group-count {
    font-size: var(--search-font-size);
  }

  .drop-target {
    box-shadow: inset 0 0 0 1px var(--fg-muted);
  }
  .group-head.drop-before::before {
    content: "";
    position: absolute;
    left: 6px;
    right: 6px;
    top: -2px;
    height: 2px;
    border-radius: 1px;
    background: var(--drop-line);
  }

  .error {
    flex: none;
    display: flex;
    gap: 6px;
    margin: 0 6px;
    padding: 6px 8px;
    border-radius: var(--tab-row-radius);
    background: var(--field-bg);
    font-size: var(--search-font-size);
    user-select: text;
  }
  .error-close {
    flex: none;
    margin-left: auto;
    color: var(--fg-muted);
  }
  .bottom-row {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    height: var(--bottom-row-height);
  }
  .bottom-button {
    width: var(--bottom-button-size);
    height: var(--bottom-button-size);
    color: var(--fg-sidebar-button);
  }
  .bottom-button {
    position: relative;
  }
  /* 새 버전이 있다는 작은 점 (spec 5.7) */
  .update-dot {
    position: absolute;
    top: var(--update-dot-offset);
    right: var(--update-dot-offset);
    width: var(--update-dot-size);
    height: var(--update-dot-size);
    border-radius: 50%;
    background: var(--fg);
  }
  .spacer {
    flex: 1;
  }
  /* 아이콘이 없는 것은 이름 첫 글자 (시안 K의 글자 아이콘) */
  .pin-badge {
    width: var(--ext-icon);
    height: var(--ext-icon);
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: calc(var(--ext-icon) / 4);
    background: var(--letter-icon-bg);
    color: var(--prompt-primary-fg);
    font-size: calc(var(--ext-icon) * 0.62);
    font-weight: 700;
    line-height: 1;
  }
  .pin-icon {
    width: var(--ext-icon);
    height: var(--ext-icon);
    object-fit: contain;
  }
  .pinned.off .pin-badge,
  .pinned.off .pin-icon {
    opacity: 0.45;
  }
  .ib.hover {
    background: var(--hover-bg);
  }
  .with-count {
    width: auto;
    gap: 4px;
    padding: 0 8px;
  }
  .more-count {
    font-size: var(--tag-font-size);
    font-weight: 600;
  }
  /* 확장 패널 (시안 K): 페이지에 가리지 않게 사이드바 안에서 위로 펼친다 */
  .panel-backdrop {
    position: fixed;
    inset: 0;
    z-index: 15;
  }
  .panel {
    position: absolute;
    z-index: 16;
    left: 4px;
    width: var(--ext-panel-width);
    bottom: calc(var(--bottom-row-height) + var(--sidebar-gap) + 5px);
  }
</style>
