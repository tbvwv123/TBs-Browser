<script lang="ts">
  // 탭의 파비콘 자리: 내부 페이지 아이콘, 불러오는 중이면 회전하는 고리, 파비콘, 없으면 빈 사각형 (spec 4.5)
  import type { Tab } from "./api";
  import { ICONS } from "./store.svelte";

  let { tab }: { tab: Tab } = $props();

  const internalIcon = $derived(
    tab.url === "tbs://history" ? ICONS.history : tab.url === "tbs://downloads" ? ICONS.downloads : tab.url === "tbs://settings" || tab.url.startsWith("tbs://scripts") || tab.url === "tbs://extensions" ? ICONS.settings : null,
  );
</script>

{#if internalIcon}
  <svg class="favicon" viewBox="0 0 24 24" fill="none" stroke="var(--fg-muted)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#each internalIcon as d}<path {d}></path>{/each}
  </svg>
{:else if tab.loading}
  <svg class="favicon spin" viewBox="0 0 16 16" fill="none" role="img" aria-label="불러오는 중"><circle cx="8" cy="8" r="5.5" stroke="var(--spinner-track)" stroke-width="2"></circle><path d="M8 2.5a5.5 5.5 0 0 1 5.5 5.5" stroke="var(--spinner-head)" stroke-width="2" stroke-linecap="round"></path></svg>
{:else if tab.favicon}
  <img class="favicon" src={tab.favicon} alt="" />
{:else}
  <svg class="favicon" viewBox="0 0 16 16" aria-hidden="true"><rect x="0.75" y="0.75" width="14.5" height="14.5" rx="3.5" fill="none" stroke="var(--favicon-empty)" stroke-width="1.5"></rect></svg>
{/if}

<style>
  .favicon {
    flex: none;
    width: var(--favicon-size);
    height: var(--favicon-size);
    object-fit: contain;
  }
  .spin {
    animation: spin 0.9s linear infinite;
    transform-origin: 50% 50%;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
</style>
