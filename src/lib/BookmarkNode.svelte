<script lang="ts">
  // 북마크 한 줄 (폴더면 그 안의 줄까지). 북마크 줄 28px, 글자 12px, 아이콘 14px (spec 4.2)
  import BookmarkNode from "./BookmarkNode.svelte";
  import RenameInput from "./RenameInput.svelte";
  import { host, countLinks, childrenOf, type BookmarkNode as Node } from "./api";
  import { ui, report, openMenu } from "./store.svelte";
  import { startDrag, consumeDragClick, dropMark } from "./drag";

  let { node, parent, depth }: { node: Node; parent: string | null; depth: number } = $props();

  const mark = $derived(dropMark("bookmark", node.id));
  const renaming = $derived(ui.renaming?.kind === "bookmark" && ui.renaming.id === node.id);
  // 폴더 안쪽 줄은 26px 들여 쓴다. 더 깊으면 16px씩 더 (spec 4.2)
  const indent = $derived(depth === 0 ? 10 : 26 + (depth - 1) * 16);

  function open(e: MouseEvent) {
    if (consumeDragClick() || renaming) return;
    if (node.type === "folder") {
      host.bookmarkSetOpen(node.id, !node.open).catch(report);
      return;
    }
    // Ctrl+클릭은 새 탭, 그냥 클릭은 현재 탭 (spec 5.3)
    if (e.ctrlKey) host.tabOpenUrl(node.url).catch(report);
    else if (ui.activeId !== null) host.navigate(ui.activeId, node.url).catch(report);
  }

  function aux(e: MouseEvent) {
    if (e.button === 1 && node.type === "link") host.tabOpenUrl(node.url).catch(report);
  }

  function nextSibling(): string | null {
    const list = childrenOf(ui.bookmarks.items, parent) ?? [];
    const i = list.findIndex((n) => n.id === node.id);
    return i >= 0 && i + 1 < list.length ? list[i + 1].id : null;
  }

  async function newFolder() {
    // 폴더에서 고르면 그 안에, 북마크에서 고르면 그 바로 아래에 만든다
    const inside = node.type === "folder";
    const id = await host.bookmarkAddFolder(inside ? node.id : parent, inside ? null : nextSibling(), "새 폴더");
    if (inside && node.type === "folder" && !node.open) await host.bookmarkSetOpen(node.id, true);
    ui.renaming = { kind: "bookmark", id };
  }

  function menu(e: MouseEvent) {
    const n = node.type === "folder" ? countLinks(node.items) : 0;
    openMenu(e, [
      { label: "이름 바꾸기", run: () => (ui.renaming = { kind: "bookmark", id: node.id }) },
      { label: "새 폴더", run: () => newFolder().catch(report) },
      {
        label: node.type === "folder" && n > 0 ? `삭제 (안의 북마크 ${n}개도)` : "삭제",
        danger: true,
        run: () => host.bookmarkRemove(node.id).catch(report),
      },
    ]);
  }
</script>

<div
  class="row"
  class:drop-before={mark === "before"}
  class:drop-after={mark === "after"}
  class:drop-inside={mark === "inside"}
  class:dragging={ui.drag?.source.kind === "bookmark" && ui.drag.source.id === node.id}
  style:padding-left="{indent}px"
  role="treeitem"
  aria-selected="false"
  aria-expanded={node.type === "folder" ? node.open : undefined}
  tabindex="0"
  title={node.type === "link" ? node.url : undefined}
  data-drop="bookmark"
  data-id={node.id}
  data-parent={parent ?? ""}
  data-folder={node.type === "folder" ? "1" : "0"}
  onclick={open}
  onauxclick={aux}
  onmousedown={(e) => e.button === 1 && e.preventDefault()}
  onkeydown={(e) => e.key === "Enter" && open(e as unknown as MouseEvent)}
  oncontextmenu={menu}
  onpointerdown={(e) => !renaming && startDrag(e, { kind: "bookmark", id: node.id }, node.title)}
>
  {#if node.type === "folder"}
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="var(--fg-muted)" stroke-width="1.9" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"></path></svg>
  {:else if node.icon}
    <img class="icon" src={node.icon} alt="" />
  {:else}
    <svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true"><rect x="0.75" y="0.75" width="14.5" height="14.5" rx="3.5" fill="none" stroke="var(--fg-muted)" stroke-width="1.5"></rect></svg>
  {/if}
  {#if renaming}
    <RenameInput
      value={node.title}
      onCommit={(v) => {
        ui.renaming = null;
        host.bookmarkRename(node.id, v).catch(report);
      }}
      onCancel={() => (ui.renaming = null)}
    />
  {:else}
    <span class="title">{node.title}</span>
  {/if}
  {#if node.type === "folder"}
    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="var(--fg-muted)" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      {#if node.open}<path d="M6 9l6 6 6-6"></path>{:else}<path d="M9 6l6 6-6 6"></path>{/if}
    </svg>
  {/if}
</div>

{#if node.type === "folder" && node.open}
  {#each node.items as child (child.id)}
    <BookmarkNode node={child} parent={node.id} depth={depth + 1} />
  {/each}
{/if}

<style>
  .row {
    flex: none;
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--bookmark-icon-gap);
    width: 100%;
    height: var(--bookmark-row-height);
    padding-right: 10px;
    border-radius: var(--bookmark-row-radius);
    cursor: pointer;
    outline: none;
  }
  .row:hover,
  .row:focus-visible {
    background: var(--hover-bg);
  }
  .icon {
    flex: none;
    width: var(--bookmark-icon-size);
    height: var(--bookmark-icon-size);
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dragging {
    opacity: 0.4;
  }
  .drop-inside {
    background: var(--hover-bg);
    box-shadow: inset 0 0 0 1px var(--fg-muted);
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
