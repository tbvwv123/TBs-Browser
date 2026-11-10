<script lang="ts">
  // 상단 툴바 (spec 4.4): 뒤로, 앞으로, 새로고침, 주소창, 다운로드 표시, 창 버튼. 빈 곳은 창을 끄는 영역이다.
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { host, host4, on, displayUrl, readableUrl, isBookmarked, type Suggestion } from "./api";
  import { ui, report, activeTab } from "./store.svelte";

  let { maxHover, maximized, onMaxButton }: { maxHover: boolean; maximized: boolean; onMaxButton: (el: HTMLButtonElement) => void } = $props();

  const appWindow = getCurrentWindow();
  let address = $state("");
  let addressFocused = $state(false);
  let addressInput: HTMLInputElement;
  let addressBox: HTMLDivElement;
  let ringButton: HTMLButtonElement | undefined = $state();
  let maxButton: HTMLButtonElement;

  // 자동완성 (spec 5.2)
  let suggestions = $state<Suggestion[]>([]);
  let selected = $state(-1);
  let suggestTimer = 0;
  let typed = ""; // 고르기 전에 입력한 글자

  const active = $derived(activeTab());
  const internal = $derived(!!active?.url.startsWith("tbs://"));
  // 새 탭 화면: 주소창은 비어 있고 안내 문구와 검색 아이콘을 보여준다 (spec 4.7, 시안 I)
  const blank = $derived(!active?.url);
  const secure = $derived(!!active?.url.startsWith("https://"));
  const starred = $derived(!!active?.url && isBookmarked(ui.bookmarks.items, active.url));

  // 다운로드 진행 고리 (spec 4.4): 진행 중일 때만 보인다
  const running = $derived(ui.downloads.filter((d) => d.state === "progress"));
  const ringFraction = $derived.by(() => {
    const known = running.filter((d) => d.total > 0);
    if (!known.length) return 0.25;
    const total = known.reduce((n, d) => n + d.total, 0);
    return Math.max(0.03, Math.min(1, known.reduce((n, d) => n + d.received, 0) / total));
  });
  const RING = 2 * Math.PI * 9.5;

  $effect(() => onMaxButton(maxButton));

  // 주소창을 쓰는 중이 아니면 활성 탭의 주소를 보여준다
  $effect(() => {
    const url = active?.url ?? "";
    if (!addressFocused) address = displayUrl(url);
  });

  onMount(() => {
    const offs = [
      // 팝업에서 고르면 호스트가 이동한다. 주소창은 정리만 한다.
      on<string>("popup_picked", () => {
        // 팝업은 호스트가 이미 닫았다
        suggestShown = false;
        clearTimeout(suggestTimer);
        suggestions = [];
        selected = -1;
        ui.error = "";
        addressInput.blur();
      }),
      on<null>("popup_closed", () => {
        suggestShown = false;
        suggestions = [];
        selected = -1;
      }),
    ];
    return () => offs.forEach((p) => p.then((f) => f()));
  });

  export function focusAddress() {
    addressInput.focus();
  }

  async function go(input = address) {
    if (ui.activeId === null) return;
    ui.error = "";
    hideSuggestions();
    addressInput.blur();
    await host.navigate(ui.activeId, input).catch(report);
  }

  function boxRect() {
    const r = addressBox.getBoundingClientRect();
    return { x: r.left, y: r.bottom + 4, w: r.width };
  }

  // 자동완성 팝업이 떠 있는지. 목록이 비었어도 팝업 창은 남아 있을 수 있어서 따로 기억한다.
  let suggestShown = false;

  function showSuggestions() {
    if (!suggestions.length || !addressFocused) return hideSuggestions();
    const r = boxRect();
    const h = 8 + suggestions.length * 34;
    suggestShown = true;
    host4.popupShow("suggest", { ...r, h }, { items: suggestions, selected }).catch(report);
  }

  function hideSuggestions() {
    clearTimeout(suggestTimer);
    if (suggestShown) host4.popupHide().catch(report);
    suggestShown = false;
    suggestions = [];
    selected = -1;
  }

  // 탭을 바꾸면 이전 탭에서 띄운 목록을 닫는다
  $effect(() => {
    void ui.activeId;
    hideSuggestions();
  });

  function onInput() {
    typed = address;
    selected = -1;
    clearTimeout(suggestTimer);
    const q = address.trim();
    if (!q) return hideSuggestions();
    suggestTimer = window.setTimeout(async () => {
      const items = await host4.suggest(q).catch(() => []);
      if (address.trim() !== q || !addressFocused) return;
      suggestions = items;
      showSuggestions();
    }, 60);
  }

  function onKey(e: KeyboardEvent) {
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && suggestions.length) {
      e.preventDefault();
      const n = suggestions.length;
      selected = e.key === "ArrowDown" ? (selected + 1) % (n + 1) : (selected - 1 + n + 1) % (n + 1);
      if (selected === n) selected = -1;
      address = selected >= 0 ? readableUrl(suggestions[selected].url) : typed;
      showSuggestions();
      return;
    }
    if (e.key === "Enter") go(selected >= 0 ? suggestions[selected].url : address);
    if (e.key === "Escape") {
      if (suggestions.length) {
        hideSuggestions();
        address = typed;
      } else {
        address = displayUrl(active?.url ?? "");
        addressInput.blur();
      }
    }
  }

  function onFocus() {
    // 포커스를 다시 받으면 새로 시작한다. 새 탭을 열면 잠깐 포커스를 잃었다 다시 받는데,
    // 그 사이에 닫히지 못한 이전 목록이 남지 않게 한다.
    hideSuggestions();
    addressFocused = true;
    // 누르면 전체 주소를 선택한 상태로 바꾼다 (spec 5.2)
    address = internal ? (active?.url ?? "") : readableUrl(active?.url ?? "");
    tick().then(() => addressInput.select());
  }

  function onBlur() {
    addressFocused = false;
    address = displayUrl(active?.url ?? "");
    // 바로 닫는다. 팝업의 항목을 누르는 중이었어도, 팝업이 고른 주소를 호스트에 직접 보내므로 고르기는 끊기지 않는다.
    hideSuggestions();
  }

  // 별 버튼: 현재 페이지를 북마크에 넣거나 뺀다 (spec 5.2)
  function toggleStar() {
    const t = active;
    if (!t?.url || internal) return;
    host.bookmarkToggle(t.title, t.url, t.favicon).catch(report);
  }

  // 진행 고리를 누르면 진행 중인 항목이 뜬다 (spec 5.4)
  function showDownloads() {
    if (!ringButton || !running.length) return;
    const r = ringButton.getBoundingClientRect();
    const w = 320;
    const h = 8 + running.length * 62;
    host4.popupShow("downloads", { x: Math.max(8, r.right - w), y: r.bottom + 4, w, h }, { items: running }).catch(report);
  }
</script>

<div class="toolbar" data-tauri-drag-region>
  <div class="side" data-tauri-drag-region>
    <button class="ib" aria-label="뒤로" disabled={!active?.canGoBack} onclick={() => ui.activeId !== null && host.goBack(ui.activeId)}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 18l-6-6 6-6"></path></svg>
    </button>
    <button class="ib" aria-label="앞으로" disabled={!active?.canGoForward} onclick={() => ui.activeId !== null && host.goForward(ui.activeId)}>
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6"></path></svg>
    </button>
    {#if active?.loading}
      <button class="ib" aria-label="중지" onclick={() => ui.activeId !== null && host.stop(ui.activeId)}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"></path></svg>
      </button>
    {:else}
      <button class="ib" aria-label="새로고침" disabled={!active?.url || internal} onclick={() => ui.activeId !== null && host.reload(ui.activeId)}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7"></path><path d="M20 4v7h-7"></path></svg>
      </button>
    {/if}
  </div>

  <div class="address" class:blank bind:this={addressBox}>
    {#if blank && !addressFocused}
      <svg class="lock" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6"></circle><path d="M20 20l-4.5-4.5"></path></svg>
    {:else if secure && !addressFocused}
      <svg class="lock" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" role="img" aria-label="보안 연결"><rect x="5" y="11" width="14" height="10" rx="2"></rect><path d="M8 11V7a4 4 0 0 1 8 0v4"></path></svg>
    {/if}
    <input
      class="address-input"
      aria-label="주소창"
      aria-autocomplete="list"
      aria-expanded={suggestions.length > 0}
      bind:this={addressInput}
      bind:value={address}
      oninput={onInput}
      onkeydown={onKey}
      onfocus={onFocus}
      onblur={onBlur}
      placeholder="검색어 또는 주소 입력"
      spellcheck="false"
      autocomplete="off"
    />
    {#if !blank}
      <button class="star" aria-label={starred ? "북마크에서 빼기" : "북마크에 추가"} aria-pressed={starred} disabled={internal} onclick={toggleStar}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill={starred ? "currentColor" : "none"} stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><path d="M12 4l2.5 5 5.5.8-4 3.9.9 5.5-4.9-2.6-4.9 2.6.9-5.5-4-3.9 5.5-.8z"></path></svg>
      </button>
    {/if}
  </div>

  <div class="side end" data-tauri-drag-region>
    {#if running.length}
      <button class="ib" bind:this={ringButton} aria-label="다운로드 진행 중 ({running.length}개)" onclick={showDownloads}>
        <svg width="20" height="20" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <circle cx="12" cy="12" r="9.5" stroke="var(--ring-track)" stroke-width="2"></circle>
          <circle cx="12" cy="12" r="9.5" stroke="var(--fg)" stroke-width="2" stroke-linecap="round" stroke-dasharray="{RING * ringFraction} {RING}" transform="rotate(-90 12 12)"></circle>
          <path d="M12 7.5v8M9 12.5l3 3 3-3" stroke="var(--fg)" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"></path>
        </svg>
      </button>
    {/if}
    <div class="window-buttons">
      <button class="ib wb" aria-label="최소화" onclick={() => appWindow.minimize()}>
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1" aria-hidden="true"><path d="M0 5h10"></path></svg>
      </button>
      <!-- 마우스로 누르는 것은 위에 얹은 스냅 레이아웃용 창이 받는다. 키보드로는 이 버튼이 받는다. -->
      <button class="ib wb" class:hover={maxHover} bind:this={maxButton} aria-label={maximized ? "이전 크기로 복원" : "최대화"} onclick={() => appWindow.toggleMaximize()}>
        {#if maximized}
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1" aria-hidden="true"><rect x="0.5" y="2.5" width="7" height="7"></rect><path d="M2.5 2.5V0.5h7v7h-2"></path></svg>
        {:else}
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1" aria-hidden="true"><rect x="0.5" y="0.5" width="9" height="9"></rect></svg>
        {/if}
      </button>
      <button class="ib wb" aria-label="닫기" onclick={() => appWindow.close()}>
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1" aria-hidden="true"><path d="M0 0l10 10M10 0L0 10"></path></svg>
      </button>
    </div>
  </div>
</div>

<style>
  .toolbar {
    flex: none;
    display: flex;
    align-items: center;
    min-height: var(--top-row-height);
  }
  /* 주소창 양쪽 묶음에 같은 폭을 줘서 주소창을 카드 중심선에 맞춘다 (spec 4.4) */
  .side {
    flex: 1 1 0;
    display: flex;
    align-items: center;
    gap: var(--toolbar-gap);
    align-self: stretch;
  }
  .side.end {
    justify-content: flex-end;
  }
  .ib {
    display: flex;
    align-items: center;
    justify-content: center;
    width: var(--icon-button-size);
    height: var(--icon-button-size);
    border-radius: var(--icon-button-radius);
    color: var(--fg);
  }
  .ib:hover:not(:disabled),
  .ib.hover {
    background: var(--hover-bg);
  }
  .ib:disabled {
    color: var(--fg-disabled);
    cursor: default;
  }
  .window-buttons {
    display: flex;
    align-items: center;
  }
  .wb {
    width: var(--window-button-width);
    height: var(--window-button-height);
    border-radius: var(--window-button-radius);
  }
  .address {
    flex: 0 1 var(--address-max-width);
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--address-icon-gap);
    height: var(--address-height);
    margin: var(--address-margin);
    padding-left: var(--address-padding-left);
    background: var(--field-bg);
    border-radius: var(--address-radius);
  }
  /* 별 버튼이 없으니 오른쪽도 왼쪽만큼 띄운다 (시안 I) */
  .address.blank {
    padding-right: var(--address-padding-left);
  }
  .lock {
    flex: none;
    color: var(--fg-muted);
  }
  .address-input {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: 0;
    outline: none;
    background: transparent;
    padding: 0;
    user-select: text;
  }
  .address-input::placeholder {
    color: var(--fg-muted);
  }
  .star {
    flex: none;
    width: var(--address-star-width);
    height: var(--address-height);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--fg-muted);
  }
  .star:disabled {
    cursor: default;
  }
</style>
