<script lang="ts">
  // 확장 프로그램 관리 (spec 6.4·6.8). 두 탭:
  // - 설치됨 (tbs://extensions): 사이트 스크립트와 크롬 확장 관리 (InstalledList)
  // - 찾아보기 (tbs://scripts, 시안 O): 저장소 목록. 설치한 스크립트와 견줘 설치 / 설치됨 / 업데이트를 정한다.
  import "./page.css";
  import { onMount } from "svelte";
  import InstalledList from "./InstalledList.svelte";
  import { host, host5, host9, host12, newerVersion, on, type StoreEntry } from "../api";
  import { ui, report, activeTab } from "../store.svelte";

  let { tab }: { tab: "installed" | "store" } = $props();
  let installChoice = $state(false);

  function go(to: "installed" | "store") {
    const t = activeTab();
    if (t) host.navigate(t.id, to === "installed" ? "tbs://extensions" : "tbs://scripts").catch(report);
  }

  function pick(folder: boolean) {
    installChoice = false;
    host9.extInstallPick(folder).catch(report);
  }

  let entries = $state<StoreEntry[] | null>(null);
  let failed = $state(false);
  let query = $state("");
  let busy = $state<string | null>(null);

  async function load(refresh: boolean) {
    failed = false;
    try {
      entries = await host12.storeList(refresh);
    } catch {
      failed = true;
    }
  }

  onMount(() => {
    load(tab === "store");
    const off = on<null>("store_done", () => (busy = null));
    return () => off.then((f) => f());
  });

  function status(e: StoreEntry): "install" | "installed" | "update" {
    const s = ui.scripts.find((x) => x.id === e.id);
    if (!s) return "install";
    return newerVersion(e.version, s.version) ? "update" : "installed";
  }

  function sites(matches: string[]): string {
    if (matches.some((m) => m === "*" || /^(\*|https?):\/\/\*\/?\*?$/.test(m) || m === "<all_urls>")) return "모든 사이트";
    const hosts = [...new Set(matches.map((m) => m.replace(/^\w+:\/\//, "").split("/")[0].replace(/^\*\./, "").replace(/^www\./, "")))].filter((h) => h && h !== "*");
    return hosts.length > 1 ? `${hosts[0]} 외 ${hosts.length - 1}곳` : (hosts[0] ?? "");
  }

  const shown = $derived.by(() => {
    const q = query.trim().toLowerCase();
    const list = entries ?? [];
    return q ? list.filter((e) => `${e.name} ${e.description} ${e.author}`.toLowerCase().includes(q)) : list;
  });

  function install(e: StoreEntry) {
    busy = e.id;
    host12.storeInstall(e.id).catch((err) => {
      busy = null;
      report(err);
    });
  }

  // 확인 창을 취소하면 busy를 푼다
  $effect(() => {
    if (busy && !ui.install) {
      const id = busy;
      const t = setTimeout(() => {
        if (busy === id && !ui.install) busy = null;
      }, 30000);
      return () => clearTimeout(t);
    }
  });
</script>

<div class="page">
  <div class="page-inner">
    <div class="head">
      <h1 class="title"><button class="tab" class:on={tab === "installed"} onclick={() => go("installed")}>설치됨</button></h1>
      <h1 class="title"><button class="tab" class:on={tab === "store"} onclick={() => go("store")}>찾아보기</button></h1>
      {#if tab === "store"}<span class="sub">스크립트 저장소</span>{/if}
      <span class="spacer"></span>
      {#if tab === "installed"}
        {#if installChoice}
          <button class="btn" onclick={() => pick(false)}>zip 파일</button>
          <button class="btn" onclick={() => pick(true)}>압축 푼 폴더</button>
          <button class="btn" onclick={() => (installChoice = false)}>취소</button>
        {:else}
          <button class="btn" onclick={() => (installChoice = true)}>크롬 확장 설치…</button>
          <button class="btn" onclick={() => host5.scriptsOpenFolder().catch(report)}>스크립트 폴더 열기</button>
        {/if}
      {/if}
    </div>
    {#if tab === "installed"}
      <InstalledList />
    {:else}
    <label class="search">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6.5"></circle><path d="M20 20l-4.2-4.2"></path></svg>
      <input aria-label="스크립트 검색" placeholder="스크립트 검색" bind:value={query} />
    </label>

    {#if failed}
      <div class="state">
        <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M2 8.5a15 15 0 0 1 20 0M5 12a10 10 0 0 1 14 0M8.5 15.5a5 5 0 0 1 7 0"></path><path d="M12 19h.01"></path><path d="M3 3l18 18"></path></svg>
        <span class="state-title">저장소에 연결할 수 없습니다</span>
        <span class="state-sub">인터넷 연결을 확인한 뒤 다시 시도하세요.</span>
        <button class="btn" onclick={() => load(true)}>다시 시도</button>
      </div>
    {:else if entries && !shown.length}
      <div class="state">
        <svg width="28" height="28" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6.5"></circle><path d="M20 20l-4.2-4.2"></path></svg>
        <span class="state-title">{query.trim() ? `"${query.trim()}"에 맞는 스크립트가 없습니다` : "저장소가 비어 있습니다"}</span>
        {#if query.trim()}<span class="state-sub">다른 낱말로 찾아보세요.</span>{/if}
      </div>
    {:else if entries}
      <div class="list">
        {#each shown as e (e.id)}
          {@const st = status(e)}
          <div class="row">
            {#if e.icon}
              <img class="icon" src={e.icon} alt="" />
            {:else}
              <span class="icon letter" aria-hidden="true">{e.name.slice(0, 1)}</span>
            {/if}
            <div class="text">
              <div class="name-line">
                <span class="name">{e.name}</span>
                <span class="meta">버전 {e.version}</span>
              </div>
              {#if e.description}<span class="desc">{e.description}</span>{/if}
              <span class="meta">{[e.author && `만든 사람 ${e.author}`, sites(e.matches)].filter(Boolean).join(" · ")}</span>
            </div>
            {#if busy === e.id}
              <span class="done">설치 중…</span>
            {:else if st === "installed"}
              <span class="done">
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"></path></svg>
                설치됨
              </span>
            {:else if st === "update"}
              <button class="btn" onclick={() => install(e)}><span class="dot" aria-hidden="true"></span>업데이트</button>
            {:else}
              <button class="btn primary" onclick={() => install(e)}>설치</button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
    {/if}
  </div>
</div>

<style>
  .head {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .title {
    margin: 0;
    font-size: var(--page-title-size);
    font-weight: 600;
  }
  /* 두 탭: 고른 쪽은 글자색, 다른 쪽은 흐리게 */
  .tab {
    font: inherit;
    color: var(--fg-muted);
  }
  .tab.on {
    color: var(--fg);
  }
  .spacer {
    flex: 1;
  }
  .head .btn {
    align-self: center;
  }
  .sub {
    font-size: var(--button-font-size);
    color: var(--fg-muted);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 10px;
    height: var(--store-search-height);
    margin: 4px 0 -8px;
    padding: 0 16px;
    background: var(--field-bg);
    border-radius: var(--store-search-radius);
    color: var(--fg-muted);
  }
  .search input {
    flex: 1;
    height: 100%;
    border: 0;
    background: transparent;
    outline: none;
    font: inherit;
    color: var(--fg);
  }
  .list {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    min-height: var(--store-row-height);
    padding: var(--store-row-padding);
    border-bottom: 1px solid var(--divider);
  }
  .icon {
    flex: none;
    width: var(--ext-icon-large);
    height: var(--ext-icon-large);
    border-radius: calc(var(--ext-icon-large) / 4);
    object-fit: contain;
  }
  .letter {
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--letter-icon-bg);
    color: var(--prompt-primary-fg);
    font-size: calc(var(--ext-icon-large) * 0.62);
    font-weight: 700;
    line-height: 1;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name-line {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .name {
    font-size: var(--store-name-size);
    font-weight: 600;
  }
  .desc {
    font-size: var(--button-font-size);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    font-size: var(--sub-font-size);
    color: var(--fg-muted);
  }
  .btn {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    height: var(--button-height);
    padding: var(--button-padding);
    border-radius: var(--button-radius);
    background: var(--field-bg);
    font-size: var(--button-font-size);
    font-weight: 500;
    white-space: nowrap;
  }
  .btn.primary {
    background: var(--prompt-primary-bg);
    color: var(--prompt-primary-fg);
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--fg);
  }
  .done {
    flex: none;
    display: flex;
    align-items: center;
    gap: 5px;
    height: var(--button-height);
    padding: 0 4px;
    font-size: var(--button-font-size);
    color: var(--fg-muted);
  }
  .state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 36px 0;
    text-align: center;
    color: var(--fg-muted);
  }
  .state-title {
    margin-top: 4px;
    font-size: var(--store-name-size);
    font-weight: 600;
    color: var(--fg);
  }
  .state-sub {
    font-size: var(--button-font-size);
  }
  .state .btn {
    margin-top: 8px;
    color: var(--fg);
  }
</style>
