<script lang="ts">
  // 오른쪽 클릭 메뉴. 콘텐츠 웹뷰가 UI를 가리므로 사이드바 안에 들어가게 자리를 맞춘다 (spec 3장).
  // 메뉴 팝업: 폭 200px, 모서리 10px, 항목 높이 32px (spec 4.2)
  import { tick } from "svelte";
  import { ui } from "./store.svelte";

  let box: HTMLDivElement | undefined = $state();
  let pos = $state({ left: 0, top: 0 });

  $effect(() => {
    const m = ui.menu;
    if (!m) return;
    tick().then(() => {
      if (!box) return;
      const side = document.querySelector(".sidebar")?.getBoundingClientRect();
      const w = box.offsetWidth;
      const h = box.offsetHeight;
      const maxX = (side ? side.right : window.innerWidth) - w - 4;
      // 위로 펼치는 메뉴는 버튼 오른쪽 끝에 맞춘다
      const left = m.above ? Math.max(4, Math.min(m.x - w, maxX)) : Math.max(4, Math.min(m.x, maxX));
      const top = m.above ? Math.max(8, m.y - h) : m.y + h > window.innerHeight - 8 ? Math.max(8, m.y - h) : m.y;
      pos = { left, top };
      box.querySelector<HTMLButtonElement>("button")?.focus();
    });
  });

  function close() {
    ui.menu = null;
  }

  function onKey(e: KeyboardEvent) {
    if (!ui.menu) return;
    if (e.key === "Escape") close();
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const items = [...(box?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
      const i = items.indexOf(document.activeElement as HTMLButtonElement);
      const next = e.key === "ArrowDown" ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
      items[next]?.focus();
      e.preventDefault();
    }
  }
</script>

<svelte:window onkeydown={onKey} onblur={close} />

{#if ui.menu}
  <div class="backdrop" role="presentation" onpointerdown={close} oncontextmenu={(e) => { e.preventDefault(); close(); }}></div>
  <div class="menu" role="menu" bind:this={box} style:left="{pos.left}px" style:top="{pos.top}px">
    {#each ui.menu.items as item}
      <button
        class="item"
        class:danger={item.danger}
        role="menuitem"
        onclick={() => {
          close();
          item.run();
        }}
      >
        {#if item.icon}
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            {#each item.icon as d}<path {d}></path>{/each}
          </svg>
        {/if}
        <span>{item.label}</span>
      </button>
    {/each}
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
  }
  .menu {
    position: fixed;
    z-index: 21;
    width: var(--menu-width);
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px;
    background: var(--menu-bg);
    border-radius: var(--menu-radius);
    box-shadow: var(--menu-shadow);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--menu-item-height);
    padding: 0 10px;
    border-radius: var(--menu-item-radius);
    outline: none;
  }
  .item:hover,
  .item:focus-visible {
    background: var(--hover-bg);
  }
</style>
