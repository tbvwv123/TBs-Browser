<script lang="ts">
  // 탭 한 줄: 파비콘, 제목, 닫기 (spec 4.5). 탭 줄 34px.
  import { host, readableUrl, type Tab } from "./api";
  import { ui, report, tabLabel, tabMenu } from "./store.svelte";
  import { startDrag, consumeDragClick, dropMark } from "./drag";
  import TabIcon from "./TabIcon.svelte";

  let { tab, nested }: { tab: Tab; nested: boolean } = $props();

  const active = $derived(tab.id === ui.activeId);
  const mark = $derived(dropMark("tab", tab.id));
  const label = $derived(tabLabel(tab));

  function activate() {
    if (consumeDragClick()) return;
    host.tabActivate(tab.id).catch(report);
  }
</script>

<div
  class="tab"
  class:active
  class:nested
  class:drop-before={mark === "before"}
  class:drop-after={mark === "after"}
  class:dragging={ui.drag?.source.kind === "tab" && ui.drag.source.id === tab.id}
  data-drop="tab"
  data-id={tab.id}
  data-tab-id={tab.id}
  role="presentation"
  oncontextmenu={(e) => tabMenu(e, tab)}
  onpointerdown={(e) => startDrag(e, { kind: "tab", id: tab.id }, label)}
>
  <button class="tab-main" aria-current={active ? "page" : undefined} title={readableUrl(tab.url) || undefined} onclick={activate}>
    <TabIcon {tab} />
    <span class="tab-title">{label}</span>
    {#if tab.audible}
      <!-- 소리가 나면 제목 오른쪽에 재생 표시 (spec 4.5, 시안 F) -->
      <svg class="audio" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" role="img" aria-label="재생 중"><path d="M6 10v4M10 6v12M14 9v6M18 7v10"></path></svg>
    {/if}
  </button>
  <button class="tab-close" aria-label="탭 닫기" onclick={() => host.tabClose(tab.id).catch(report)} onpointerdown={(e) => e.stopPropagation()}>
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"></path></svg>
  </button>
</div>

<style>
  .tab {
    flex: none;
    position: relative;
    display: flex;
    align-items: stretch;
    height: var(--tab-row-height);
    border-radius: var(--tab-row-radius);
    /* 탭을 바꾸면 목록이 따라갈 때, 붙는 머리줄 아래로 숨지 않게 한다 */
    scroll-margin-top: var(--tabs-head-height);
  }
  .tab.nested {
    scroll-margin-top: calc(var(--tabs-head-height) + var(--tab-row-height));
  }
  .tab:hover {
    background: var(--hover-bg);
  }
  .tab.active {
    background: var(--tab-active-bg);
    box-shadow: var(--tab-active-shadow);
  }
  .tab-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--tab-row-icon-gap);
    padding-left: 10px;
  }
  .nested .tab-main {
    padding-left: var(--nested-indent);
  }
  .active .tab-main {
    font-weight: var(--tab-active-weight);
  }
  .tab-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .audio {
    flex: none;
    color: var(--fg);
  }
  .tab-close {
    flex: none;
    width: var(--tab-close-width);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--fg-muted);
    visibility: hidden;
  }
  /* 선택된 탭은 늘, 다른 탭은 마우스를 올렸을 때 닫기 버튼이 보인다 (spec 4.5) */
  .active .tab-close,
  .tab:hover .tab-close,
  .tab-close:focus-visible {
    visibility: visible;
  }
  .dragging {
    opacity: 0.4;
  }
  .drop-before::before,
  .drop-after::after {
    content: "";
    position: absolute;
    left: 6px;
    right: 6px;
    height: 2px;
    border-radius: 1px;
    background: var(--drop-line);
  }
  .drop-before::before {
    top: -2px;
  }
  .drop-after::after {
    bottom: -2px;
  }
</style>
