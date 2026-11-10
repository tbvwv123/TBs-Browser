<script lang="ts">
  // 방문 기록 페이지 (spec 5.4): 검색, 개별 삭제, 전체 삭제
  import "./page.css";
  import { onMount } from "svelte";
  import { host, host4, readableUrl, type Visit } from "../api";
  import { ui, report } from "../store.svelte";

  let query = $state("");
  let visits = $state<Visit[]>([]);
  let more = $state(false);
  let confirmClear = $state(false);
  let timer = 0;

  async function load(append = false) {
    const before = append && visits.length ? visits[visits.length - 1].time : null;
    const page = await host4.historySearch(query.trim(), before).catch((e) => {
      report(e);
      return [] as Visit[];
    });
    visits = append ? [...visits, ...page] : page;
    more = page.length === 100;
  }

  function onQuery() {
    clearTimeout(timer);
    timer = window.setTimeout(() => load(), 150);
  }

  async function remove(id: number) {
    await host4.historyDelete(id).catch(report);
    visits = visits.filter((v) => v.id !== id);
  }

  async function clearAll() {
    confirmClear = false;
    await host4.historyClear().catch(report);
    visits = [];
    more = false;
  }

  function open(e: MouseEvent, url: string) {
    if (e.ctrlKey || e.button === 1) host.tabOpenUrl(url).catch(report);
    else if (ui.activeId !== null) host.navigate(ui.activeId, url).catch(report);
  }

  function day(t: number): string {
    const d = new Date(t);
    const today = new Date();
    const y = new Date(today);
    y.setDate(today.getDate() - 1);
    const same = (a: Date, b: Date) => a.toDateString() === b.toDateString();
    if (same(d, today)) return "오늘";
    if (same(d, y)) return "어제";
    return d.toLocaleDateString("ko-KR", { year: "numeric", month: "long", day: "numeric", weekday: "short" });
  }

  function time(t: number): string {
    return new Date(t).toLocaleTimeString("ko-KR", { hour: "2-digit", minute: "2-digit", hour12: false });
  }

  onMount(() => {
    load();
  });
</script>

<div class="page">
  <div class="page-inner">
    <div class="page-head">
      <h1 class="page-title">방문 기록</h1>
      <input class="page-field" bind:value={query} oninput={onQuery} placeholder="방문 기록 검색" aria-label="방문 기록 검색" />
      {#if confirmClear}
        <span class="page-muted">방문 기록을 모두 지울까요?</span>
        <button class="page-button" onclick={clearAll}>모두 지우기</button>
        <button class="page-button" onclick={() => (confirmClear = false)}>취소</button>
      {:else}
        <button class="page-button" disabled={!visits.length} onclick={() => (confirmClear = true)}>전체 삭제</button>
      {/if}
    </div>

    {#if !visits.length}
      <p class="page-empty">{query ? `"${query}"이(가) 들어간 기록이 없습니다.` : "방문 기록이 없습니다."}</p>
    {/if}

    <div class="page-list">
      {#each visits as v, i (v.id)}
        {#if i === 0 || day(visits[i - 1].time) !== day(v.time)}
          <h2 class="page-section-title">{day(v.time)}</h2>
        {/if}
        <div class="page-row">
          <span class="page-muted when">{time(v.time)}</span>
          <button class="entry" title={readableUrl(v.url)} onclick={(e) => open(e, v.url)} onauxclick={(e) => open(e, v.url)}>
            <span class="page-ellipsis title">{v.title || readableUrl(v.url)}</span>
            <span class="page-ellipsis page-muted">{readableUrl(v.url)}</span>
          </button>
          <button class="remove" aria-label="이 기록 지우기" onclick={() => remove(v.id)}>×</button>
        </div>
      {/each}
    </div>
    {#if more}<button class="page-button" onclick={() => load(true)}>더 보기</button>{/if}
  </div>
</div>

<style>
  .when {
    flex: none;
    width: 44px;
    white-space: nowrap;
  }
  .entry {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .title {
    flex: 0 1 auto;
  }
  .entry .page-muted {
    flex: 1 1 0;
  }
  .remove {
    flex: none;
    width: 28px;
    height: 28px;
    border-radius: var(--icon-button-radius);
    color: var(--fg-muted);
    visibility: hidden;
    text-align: center;
  }
  .page-row:hover .remove,
  .remove:focus-visible {
    visibility: visible;
  }
  .remove:hover {
    background: var(--hover-bg);
  }
</style>
