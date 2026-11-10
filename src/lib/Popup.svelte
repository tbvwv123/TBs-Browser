<script lang="ts">
  // 콘텐츠 위에 뜨는 팝업 창의 화면 (spec 3장 배치): 주소창 자동완성, 진행 중인 다운로드,
  // 접힌 사이드바의 메뉴와 북마크 목록 (spec 4.6).
  // 창은 투명하고, 그림자를 그릴 여백(왼쪽·오른쪽·아래 16px)을 둔다.
  import { onMount, tick } from "svelte";
  import { host4, host5, host9, on, readableUrl, fileName, formatBytes, type Suggestion, type Download, type BookmarkNode, type ExtInfo, type ScriptInfo } from "./api";
  import ExtensionsPanel from "./ExtensionsPanel.svelte";
  import InstallConfirm from "./InstallConfirm.svelte";

  type MenuEntry = { label: string; danger: boolean; icon: string[] | null };
  type Theme = { theme: string };
  type Data =
    | { kind: "suggest"; data: Theme & { items: Suggestion[]; selected: number } }
    | { kind: "downloads"; data: Theme & { items: Download[] } }
    | { kind: "menu"; data: Theme & { items: MenuEntry[] } }
    | { kind: "bookmarks"; data: Theme & { items: BookmarkNode[] } }
    | { kind: "extensions"; data: Theme & { url: string; anchor: { x: number; bottom: number } } }
    | { kind: "install"; data: Theme & { info: ExtInfo } };
  let view = $state<Data | null>(null);
  let error = $state("");
  // 접힌 사이드바의 확장 패널 (시안 K): 목록은 팝업이 직접 받아 온다
  let scripts = $state<ScriptInfo[]>([]);
  let exts = $state<ExtInfo[]>([]);
  // 확장 패널·설치 확인은 내용 높이에 창을 맞춘다
  let fitted = $state<HTMLElement>();
  // 내용 높이를 재서 창을 맞춘다. 다시 띄울 때 내용 높이가 그대로면 ResizeObserver가 부르지 않으므로
  // 띄울 때마다 직접도 부른다 (창 크기는 띄울 때 다시 정해진다)
  function refit() {
    requestAnimationFrame(() => {
      if (fitted) host9.popupFit(Math.ceil(fitted.getBoundingClientRect().height)).catch(() => {});
    });
  }
  $effect(() => {
    if (!fitted) return;
    const ro = new ResizeObserver(refit);
    ro.observe(fitted);
    return () => ro.disconnect();
  });

  function loadLists() {
    Promise.all([
      host5.scriptsList().then((l) => (scripts = l)),
      host9.extList().then((l) => (exts = l)),
    ]).then(() => tick().then(refit), (e) => (error = String(e)));
  }

  // 북마크 목록에서 펼친 폴더 (이 목록 안에서만. 사이드바의 접힘 상태는 바꾸지 않는다)
  let open = $state<Record<string, boolean>>({});

  onMount(() => {
    // 창 바탕을 투명하게 (base.css의 창 그라디언트를 지운다)
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    const off = on<Data>("popup_data", (d) => {
      // 메인 창과 같은 테마로 그린다 (UI가 data.theme에 넣어 보낸다)
      document.documentElement.dataset.theme = d.data.theme === "dark" ? "dark" : "light";
      if (d.kind === "bookmarks" && view?.kind !== "bookmarks") open = {};
      if (d.kind === "extensions") loadLists();
      view = d;
      error = "";
      if (d.kind === "extensions" || d.kind === "install") tick().then(refit);
    });
    const offs = [
      on<ScriptInfo[]>("scripts_changed", (l) => (scripts = l)),
      on<ExtInfo[]>("extensions_changed", (l) => (exts = l)),
    ];
    return () => [off, ...offs].forEach((p) => p.then((f) => f()));
  });

  function act(p: Promise<void>) {
    p.catch((e) => (error = String(e)));
  }
</script>

{#snippet bookmarkRows(items: BookmarkNode[], depth: number)}
  {#each items as node (node.id)}
    {#if node.type === "folder"}
      {@const isOpen = open[node.id] ?? node.open}
      <button class="bm" style:padding-left="calc(10px + {depth} * var(--nested-indent) - {depth} * 10px)" aria-expanded={isOpen} onclick={() => (open[node.id] = !isOpen)}>
        <svg class="bm-icon" viewBox="0 0 24 24" fill="none" stroke="var(--fg-muted)" stroke-width="1.9" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"></path></svg>
        <span class="title grow">{node.title}</span>
        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="var(--fg-muted)" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          {#if isOpen}<path d="M6 9l6 6 6-6"></path>{:else}<path d="M9 6l6 6-6 6"></path>{/if}
        </svg>
      </button>
      {#if isOpen}{@render bookmarkRows(node.items, depth + 1)}{/if}
    {:else}
      <!-- 누르면 지금 탭에서, Ctrl+클릭이나 가운데 클릭은 새 탭에서 (spec 5.3) -->
      <button
        class="bm"
        style:padding-left="calc(10px + {depth} * var(--nested-indent) - {depth} * 10px)"
        title={readableUrl(node.url)}
        onclick={(e) => act(host4.popupPick(node.url, e.ctrlKey))}
        onauxclick={(e) => e.button === 1 && act(host4.popupPick(node.url, true))}
      >
        {#if node.icon}
          <img class="bm-icon" src={node.icon} alt="" />
        {:else}
          <svg class="bm-icon" viewBox="0 0 16 16" aria-hidden="true"><rect x="0.75" y="0.75" width="14.5" height="14.5" rx="3.5" fill="none" stroke="var(--favicon-empty)" stroke-width="1.5"></rect></svg>
        {/if}
        <span class="title grow">{node.title || readableUrl(node.url)}</span>
      </button>
    {/if}
  {/each}
{/snippet}

{#if view?.kind === "extensions"}
  <!-- 확장 패널은 버튼에 아래 끝을 맞춘다: 창 높이를 맞추기 전에도 버튼 옆에 보이게 아래에 붙여 그린다 -->
  <div class="fit bottom" bind:this={fitted}>
    <ExtensionsPanel scripts={scripts} extensions={exts} url={view.data.url} onError={(e) => (error = String(e))} popupAnchor={() => (view as any).data.anchor} />
    {#if error}<p class="error fit-error">{error}</p>{/if}
  </div>
{:else if view?.kind === "install"}
  <div class="fit" bind:this={fitted}>
    <InstallConfirm info={view.data.info} onAnswer={(a) => act(host9.extInstallConfirm(a))} />
  </div>
{:else if view}
  <div
    class="box"
    class:scroll={view.kind === "bookmarks"}
    role={view.kind === "suggest" ? "listbox" : view.kind === "menu" ? "menu" : "dialog"}
    aria-label={view.kind === "suggest" ? "자동완성" : view.kind === "downloads" ? "진행 중인 다운로드" : view.kind === "bookmarks" ? "북마크" : "메뉴"}
  >
    {#if view.kind === "menu"}
      {#each view.data.items as m, i}
        <button class="item" class:danger={m.danger} role="menuitem" onclick={() => act(host4.popupMenuPick(i))}>
          {#if m.icon}
            <svg class="kind" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              {#each m.icon as d}<path {d}></path>{/each}
            </svg>
          {/if}
          <span>{m.label}</span>
        </button>
      {/each}
    {:else if view.kind === "bookmarks"}
      {#if view.data.items.length}
        {@render bookmarkRows(view.data.items, 0)}
      {:else}
        <p class="empty">북마크가 없습니다. 주소창의 별 버튼으로 추가하세요.</p>
      {/if}
    {:else if view.kind === "suggest"}
      {#each view.data.items as s, i}
        <!-- 누르는 순간 고른다 (주소창이 포커스를 잃기 전에) -->
        <div class="item" class:selected={i === view.data.selected} role="option" aria-selected={i === view.data.selected} tabindex="-1" onpointerdown={(e) => {
            e.preventDefault();
            act(host4.popupPick(s.url));
          }}>
          {#if s.kind === "bookmark"}
            <svg class="kind" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-label="북마크"><path d="M12 4l2.5 5 5.5.8-4 3.9.9 5.5-4.9-2.6-4.9 2.6.9-5.5-4-3.9 5.5-.8z"></path></svg>
          {:else}
            <svg class="kind" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-label="방문 기록"><circle cx="12" cy="12" r="8"></circle><path d="M12 8v4l3 2"></path></svg>
          {/if}
          <span class="title">{s.title || readableUrl(s.url)}</span>
          <span class="url">{readableUrl(s.url)}</span>
        </div>
      {/each}
    {:else}
      {#each view.data.items as d (d.id)}
        <div class="download">
          <div class="dl-top">
            <span class="title">{fileName(d.path)}</span>
            <button class="link" onclick={() => act(host4.downloadShow(d.id))}>폴더 열기</button>
            <button class="link" onclick={() => act(host4.downloadCancel(d.id))}>취소</button>
          </div>
          <div class="bar" aria-hidden="true"><span style:width="{d.total > 0 ? Math.min(100, (d.received / d.total) * 100) : 0}%"></span></div>
          <span class="size">{formatBytes(d.received)} / {d.total > 0 ? formatBytes(d.total) : "크기 모름"}</span>
        </div>
      {/each}
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
  </div>
{/if}

<style>
  /* 내용 높이에 창을 맞추는 팝업: 위에 붙이고 그림자 여백(16px)을 둔다 */
  .fit {
    position: absolute;
    left: 16px;
    right: 16px;
    top: 0;
  }
  .fit.bottom {
    top: auto;
    bottom: 16px;
  }
  .fit-error {
    margin: 6px 0 0;
    padding: 6px 10px;
    border-radius: var(--menu-radius);
    background: var(--menu-bg);
    box-shadow: var(--menu-shadow);
  }
  .box {
    position: absolute;
    left: 16px;
    right: 16px;
    top: 0;
    bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px;
    background: var(--menu-bg);
    border-radius: var(--menu-radius);
    box-shadow: var(--menu-shadow);
    overflow: hidden;
    color: var(--fg);
  }
  .box.scroll {
    overflow-y: auto;
    scrollbar-width: thin;
  }
  .item {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--menu-item-height);
    padding: 0 10px;
    border-radius: var(--menu-item-radius);
    cursor: pointer;
    flex: none;
  }
  .item:hover,
  .item.selected {
    background: var(--hover-bg);
  }
  .kind {
    flex: none;
    color: var(--fg-muted);
  }
  .item.danger {
    color: var(--fg);
  }
  .bm {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--bookmark-icon-gap);
    width: 100%;
    height: var(--bookmark-row-height);
    padding: 0 10px;
    border-radius: var(--bookmark-row-radius);
    font-size: var(--bookmark-font-size);
    color: var(--fg-bookmark);
  }
  .bm:hover {
    background: var(--hover-bg);
  }
  .bm-icon {
    flex: none;
    width: var(--bookmark-icon-size);
    height: var(--bookmark-icon-size);
    object-fit: contain;
  }
  .title.grow {
    flex: 1 1 0;
  }
  .empty {
    margin: 6px 10px;
    font-size: var(--search-font-size);
    color: var(--fg-muted);
  }
  .title {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .url {
    flex: 1 1 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--fg-muted);
    font-size: var(--search-font-size);
  }
  .size {
    color: var(--fg-muted);
    font-size: var(--search-font-size);
  }
  .download {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 10px;
    border-radius: var(--menu-item-radius);
  }
  .dl-top {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .dl-top .title {
    flex: 1;
  }
  .link {
    flex: none;
    font-size: var(--search-font-size);
    color: var(--fg-muted);
  }
  .link:hover {
    color: var(--fg);
  }
  .bar {
    height: 4px;
    border-radius: 2px;
    background: var(--field-bg);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--fg);
  }
  .error {
    margin: 4px 10px;
    font-size: var(--search-font-size);
  }
</style>
