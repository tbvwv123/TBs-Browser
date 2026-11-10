<script lang="ts">
  // 접힌 사이드바 (spec 4.6, 시안 H): 44px 폭의 아이콘 한 줄.
  // 위에서부터 펼치기, 탭 검색, 북마크, 구분선, 새 탭, 탭 파비콘들. 맨 아래에 확장 프로그램과 점 3개.
  // 44px 밖으로 나가는 것(메뉴, 북마크 목록)은 콘텐츠 위에 뜨는 팝업 창으로 띄운다.
  import { tick } from "svelte";
  import TabIcon from "./TabIcon.svelte";
  import { host, host4, type BookmarkNode, type Group, type Tab } from "./api";
  import { ui, report, activeTab, moreMenuItems, openMenu, openMenuAbove, tabLabel, tabMenu, setSidebarCollapsed, token } from "./store.svelte";
  import { startDrag, consumeDragClick, dropMark } from "./drag";

  let { onNewTab, onSearch }: { onNewTab: () => void; onSearch: () => void } = $props();

  let list: HTMLDivElement;
  let scrolled = $state(false);
  let bookmarksButton: HTMLButtonElement;

  type Block = { kind: "tab"; tab: Tab } | { kind: "group"; group: Group; tabs: Tab[] };

  // 탭 검색 글자가 남아 있으면 접힌 상태에서도 그 탭만 보인다 (spec 5.1)
  const query = $derived(ui.search.trim().toLowerCase());
  const blocks = $derived.by(() => {
    const tabs = query ? ui.tabs.filter((t) => t.title.toLowerCase().includes(query) || t.url.toLowerCase().includes(query)) : ui.tabs;
    const out: Block[] = [];
    for (const t of tabs) {
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

  // 탭을 바꾸면 그 탭이 보이도록 목록이 따라간다 (spec 5.1)
  $effect(() => {
    const id = ui.activeId;
    if (id === null) return;
    tick().then(() => list?.querySelector(`[data-tab-id="${id}"]`)?.scrollIntoView({ block: "nearest" }));
  });

  // 끌어 옮기는 동안 목록 위·아래 끝에 가까이 가면 그쪽으로 스크롤한다 (접힌 목록은 짧아서 자주 필요하다)
  $effect(() => {
    if (!ui.drag || ui.drag.source.kind !== "tab") return;
    const timer = setInterval(() => {
      const d = ui.drag;
      if (!d || !list) return;
      const r = list.getBoundingClientRect();
      const edge = token("--collapsed-button-height");
      if (d.y < r.top + edge && d.y > r.top - edge) list.scrollTop -= 8;
      else if (d.y > r.bottom - edge && d.y < r.bottom + edge) list.scrollTop += 8;
    }, 16);
    return () => clearInterval(timer);
  });

  // 이름 바꾸기는 펼친 사이드바의 그룹 이름 줄에서 한다
  function groupMenu(e: MouseEvent, g: Group) {
    e.stopPropagation();
    openMenu(e, [
      {
        label: "이름 바꾸기",
        run: async () => {
          await setSidebarCollapsed(false);
          ui.renaming = { kind: "group", id: g.id };
        },
      },
      { label: g.collapsed ? "펼치기" : "접기", run: () => host.groupSetCollapsed(g.id, !g.collapsed).catch(report) },
      { label: "그룹 풀기", run: () => host.groupUngroup(g.id).catch(report) },
    ]);
  }

  /** 지금 펼쳐 보일 북마크 줄 수 (팝업 높이를 정한다) */
  function visibleRows(items: BookmarkNode[]): number {
    let n = 0;
    for (const it of items) {
      n++;
      if (it.type === "folder" && it.open) n += visibleRows(it.items);
    }
    return n;
  }

  // 북마크 버튼: 목록을 버튼 오른쪽에 띄운다 (spec 4.6)
  function showBookmarks() {
    const r = bookmarksButton.getBoundingClientRect();
    const rows = Math.max(1, visibleRows(ui.bookmarks.items));
    const rowH = token("--bookmark-row-height") + token("--tab-row-gap");
    const h = Math.min(window.innerHeight - r.top - 24, 8 + rows * rowH);
    host4.popupShow("bookmarks", { x: r.right + 4, y: r.top, w: token("--bookmarks-popup-width"), h }, { items: ui.bookmarks.items }).catch(report);
  }

  // 확장 프로그램 버튼: 고정 아이콘은 보이지 않고 이 안으로 들어간다 (spec 4.6, 시안 K).
  // 패널은 페이지 위 팝업 창으로, 버튼 오른쪽에 아래 끝을 맞춰 띄운다. 높이는 팝업이 내용에 맞춘다.
  function showExtensions(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const w = token("--ext-panel-collapsed-width");
    const h = 420;
    const x = r.right + 4;
    const anchor = { x: x + w + 8, bottom: r.bottom };
    host4.popupShow("extensions", { x, y: Math.max(8, r.bottom - h), w, h }, { url: activeTab()?.url ?? "", anchor }).catch(report);
  }

  function moreMenu(e: MouseEvent) {
    openMenuAbove(e.currentTarget as HTMLElement, moreMenuItems());
  }
</script>

{#snippet tabButton(tab: Tab)}
  {@const active = tab.id === ui.activeId}
  {@const mark = dropMark("tab", tab.id)}
  <!-- 끌어서 순서와 그룹을 바꾼다 (spec 5.1). 펼친 사이드바와 같은 규칙 (drag.ts) -->
  <button
    class="cb tab"
    class:active
    class:drop-before={mark === "before"}
    class:drop-after={mark === "after"}
    class:dragging={ui.drag?.source.kind === "tab" && ui.drag.source.id === tab.id}
    data-tab-id={tab.id}
    data-drop="tab"
    data-id={tab.id}
    aria-current={active ? "page" : undefined}
    aria-label={tabLabel(tab) + (tab.audible ? ", 재생 중" : "") + (tab.loading ? ", 불러오는 중" : "")}
    title={tabLabel(tab)}
    onpointerdown={(e) => startDrag(e, { kind: "tab", id: tab.id }, tabLabel(tab))}
    onclick={() => {
      if (!consumeDragClick()) host.tabActivate(tab.id).catch(report);
    }}
    oncontextmenu={(e) => {
      e.stopPropagation();
      tabMenu(e, tab);
    }}
  >
    <TabIcon {tab} />
    {#if tab.audible}<span class="audio-dot" aria-hidden="true"></span>{/if}
  </button>
{/snippet}

<nav class="collapsed" aria-label="접힌 사이드바">
  <button class="cb" aria-label="사이드바 펼치기" title="사이드바 펼치기 (Ctrl+B)" onclick={() => setSidebarCollapsed(false)}>
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="3"></rect><path d="M9 4v16"></path></svg>
  </button>
  <button class="cb" aria-label="탭 검색" title="탭 검색 (Ctrl+Shift+A)" onclick={onSearch}>
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6"></circle><path d="M20 20l-4.5-4.5"></path></svg>
  </button>
  <!-- 탭을 여기에 놓으면 북마크로 추가한다 (spec 5.3) -->
  <button
    class="cb"
    class:drop-target={dropMark("bookmarks-head", "")}
    data-drop="bookmarks-head"
    bind:this={bookmarksButton}
    aria-label="북마크"
    title="북마크"
    aria-haspopup="menu"
    onclick={showBookmarks}
  >
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linejoin="round" aria-hidden="true"><path d="M6 4h12v16l-6-4-6 4z"></path></svg>
  </button>

  <div class="short-divider" aria-hidden="true"></div>

  <!-- 탭을 여기에 놓으면 목록 맨 위로 (펼친 사이드바의 "새 탭" 줄과 같다) -->
  <button class="cb" class:drop-target={dropMark("tabs-head", "")} data-drop="tabs-head" aria-label="새 탭" title="새 탭 (Ctrl+T)" onclick={onNewTab}>
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14"></path></svg>
  </button>

  <!-- 목록을 내리면 위에 얇은 선이 생긴다 (시안 H) -->
  <div class="tabs-line" class:on={scrolled} aria-hidden="true"></div>

  <!-- 탭 파비콘 목록만 스크롤되고 스크롤 막대는 보이지 않는다 (spec 4.6) -->
  <div class="tabs" bind:this={list} onscroll={() => (scrolled = list.scrollTop > 0)}>
    {#each blocks as block (block.kind === "tab" ? `t${block.tab.id}` : `g${block.group.id}`)}
      {#if block.kind === "tab"}
        {@render tabButton(block.tab)}
      {:else}
        {@const g = block.group}
        <!-- 탭 그룹은 옅은 둥근 바탕 하나로 묶는다 (spec 4.6) -->
        {@const gm = dropMark("group", g.id)}
        <!-- 그룹 바탕 위(탭과 탭 사이)에 놓으면 그룹 안으로, 위쪽 끝에 놓으면 그룹 앞으로 -->
        <div
          class="group"
          class:drop-before={gm === "before"}
          class:drop-target={gm === "inside"}
          data-drop="group"
          data-id={g.id}
          data-edge="6"
          role="group"
          aria-label="{g.title} 그룹"
          oncontextmenu={(e) => groupMenu(e, g)}
        >
          <!-- 그룹 맨 위의 화살표: 눌러서 접고 펼친다 (펼친 사이드바의 그룹 이름 줄 화살표와 같은 일) -->
          {#if !query}
            <button
              class="group-toggle"
              aria-expanded={!g.collapsed}
              aria-label="{g.title} 그룹 {g.collapsed ? '펼치기' : '접기'}"
              title="{g.title} ({g.collapsed ? '펼치기' : '접기'})"
              onclick={() => host.groupSetCollapsed(g.id, !g.collapsed).catch(report)}
            >
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                {#if g.collapsed}<path d="M9 6l6 6-6 6"></path>{:else}<path d="M6 9l6 6 6-6"></path>{/if}
              </svg>
            </button>
          {/if}
          {#if g.collapsed && !query}
            <!-- 접은 그룹: 탭 수를 보여 주고, 누르면 펼친다. 보고 있는 탭이 안에 있으면 선택된 모양 -->
            <button
              class="cb count"
              class:active={block.tabs.some((t) => t.id === ui.activeId)}
              aria-label="{g.title} 그룹 펼치기, 탭 {block.tabs.length}개"
              title="{g.title} (누르면 펼침)"
              onclick={() => host.groupSetCollapsed(g.id, false).catch(report)}
            >
              {block.tabs.length}
            </button>
          {:else}
            {#each block.tabs as tab (tab.id)}
              {@render tabButton(tab)}
            {/each}
          {/if}
        </div>
      {/if}
    {/each}
  </div>

  <div class="short-divider bottom" aria-hidden="true"></div>

  <button class="cb strong" aria-label="확장 프로그램" title="확장 프로그램" onclick={showExtensions}>
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><path d="M4 8h4V7a2 2 0 1 1 4 0v1h4v4h1a2 2 0 1 1 0 4h-1v4h-4v-1a2 2 0 1 0-4 0v1H4z"></path></svg>
  </button>
  <button class="cb strong" aria-label={ui.update ? "더 보기, 새 버전 있음" : "더 보기"} title="더 보기" aria-haspopup="menu" onclick={moreMenu}>
    {#if ui.update}<span class="update-dot" aria-hidden="true"></span>{/if}
    <svg width="17" height="17" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="12" cy="5" r="1.7"></circle><circle cx="12" cy="12" r="1.7"></circle><circle cx="12" cy="19" r="1.7"></circle></svg>
  </button>
</nav>

<style>
  .collapsed {
    flex: none;
    width: var(--sidebar-collapsed-width);
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--collapsed-gap);
    margin-right: var(--collapsed-margin-right);
    padding-bottom: 4px;
  }
  .cb {
    flex: none;
    position: relative;
    width: var(--collapsed-button-width);
    height: var(--collapsed-button-height);
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--icon-button-radius);
    color: var(--fg-muted);
  }
  .cb:hover {
    background: var(--hover-bg);
  }
  .cb.strong {
    color: var(--fg-sidebar-button);
  }
  .cb.tab.active,
  .cb.count.active {
    background: var(--tab-active-bg);
    box-shadow: var(--tab-active-shadow);
  }
  .group-toggle {
    flex: none;
    width: var(--collapsed-button-width);
    height: var(--collapsed-group-toggle-height);
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--icon-button-radius);
    color: var(--fg-muted);
  }
  .group-toggle:hover {
    background: var(--hover-bg);
  }
  .count {
    font-size: var(--search-font-size);
    font-weight: 600;
  }
  .short-divider {
    flex: none;
    width: var(--collapsed-divider-width);
    height: 1px;
    margin: 4px 0;
    background: var(--divider);
  }
  .short-divider.bottom {
    margin: 2px 0;
  }
  .tabs-line {
    flex: none;
    width: var(--collapsed-button-width);
    height: 1px;
    background: transparent;
  }
  .tabs-line.on {
    background: var(--divider);
  }
  .tabs {
    flex: 1 1 0;
    min-height: 0;
    width: var(--sidebar-collapsed-width);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--collapsed-gap);
    padding: var(--collapsed-list-padding);
    overflow-y: auto;
    scrollbar-width: none;
  }
  .tabs::-webkit-scrollbar {
    display: none;
  }
  .group {
    flex: none;
    width: var(--collapsed-group-width);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--collapsed-gap);
    padding: var(--collapsed-group-padding);
    background: var(--collapsed-group-bg);
    border-radius: var(--collapsed-group-radius);
  }
  .dragging {
    opacity: 0.4;
  }
  .drop-target {
    box-shadow: inset 0 0 0 1px var(--fg-muted);
  }
  /* 놓을 자리 선: 버튼 사이 간격(4px) 가운데에 그린다 */
  .drop-before::before,
  .drop-after::after {
    content: "";
    position: absolute;
    left: 4px;
    right: 4px;
    height: 2px;
    border-radius: 1px;
    background: var(--drop-line);
    pointer-events: none;
  }
  .drop-before::before {
    top: -3px;
  }
  .drop-after::after {
    bottom: -3px;
  }
  .group {
    position: relative;
  }
  .update-dot {
    position: absolute;
    top: var(--update-dot-offset);
    right: var(--update-dot-offset);
    width: var(--update-dot-size);
    height: var(--update-dot-size);
    border-radius: 50%;
    background: var(--fg);
  }
  .audio-dot {
    position: absolute;
    right: var(--collapsed-audio-dot-offset);
    bottom: var(--collapsed-audio-dot-offset);
    width: var(--collapsed-audio-dot);
    height: var(--collapsed-audio-dot);
    border-radius: 50%;
    background: var(--fg);
    border: var(--collapsed-audio-dot-border);
  }
</style>
