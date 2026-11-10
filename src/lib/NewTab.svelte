<script lang="ts">
  // 새 탭 화면 (spec 4.7, 시안 I·J): 페이지 카드 가운데(살짝 위)에 검색·주소 입력 칸과 "자주 가는 곳" 바로가기.
  // 빈 탭에는 콘텐츠 웹뷰가 없어서 UI가 카드 자리에 직접 그린다. 자동완성 목록도 카드 안에 그린다.
  import { onMount, tick } from "svelte";
  import { host, host4, host7, readableUrl, type Suggestion, type TopSite } from "./api";
  import { ui, report } from "./store.svelte";

  let input: HTMLInputElement;
  let text = $state("");
  let sites = $state<TopSite[]>([]);
  let suggestions = $state<Suggestion[]>([]);
  let selected = $state(-1);
  let typed = "";
  let timer = 0;

  onMount(() => {
    host7.topSites().then((s) => (sites = s), report);
    return () => clearTimeout(timer);
  });

  export function focus() {
    tick().then(() => input?.focus());
  }

  async function go(value: string) {
    if (ui.activeId === null || !value.trim()) return;
    ui.error = "";
    suggestions = [];
    await host.navigate(ui.activeId, value).catch(report);
  }

  function onInput() {
    typed = text;
    selected = -1;
    clearTimeout(timer);
    const q = text.trim();
    if (!q) {
      suggestions = [];
      return;
    }
    timer = window.setTimeout(async () => {
      const items = await host4.suggest(q).catch(() => []);
      if (text.trim() === q) suggestions = items;
    }, 60);
  }

  function onKey(e: KeyboardEvent) {
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && suggestions.length) {
      e.preventDefault();
      const n = suggestions.length + 1;
      selected = ((e.key === "ArrowDown" ? selected + 1 : selected - 1) + 1 + n) % n - 1;
      text = selected >= 0 ? readableUrl(suggestions[selected].url) : typed;
    } else if (e.key === "Enter") {
      go(selected >= 0 ? suggestions[selected].url : text);
    } else if (e.key === "Escape") {
      if (suggestions.length) {
        suggestions = [];
        text = typed;
      } else {
        text = "";
        input.blur();
      }
    }
  }

  // 바로가기: 누르면 이 탭에서, Ctrl+클릭이나 가운데 클릭은 새 탭에서 연다 (북마크와 같은 규칙, spec 5.3)
  function open(e: MouseEvent, s: TopSite) {
    if (e.button === 1 || e.ctrlKey) host.tabOpenUrl(s.url).catch(report);
    else go(s.url);
  }
</script>

<div class="newtab">
  <div class="column">
    <div class="search-wrap">
      <label class="search">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6"></circle><path d="M20 20l-4.5-4.5"></path></svg>
        <input
          bind:this={input}
          bind:value={text}
          oninput={onInput}
          onkeydown={onKey}
          onblur={() => (suggestions = [])}
          placeholder="검색어 또는 주소 입력"
          aria-label="검색어 또는 주소 입력"
          aria-autocomplete="list"
          aria-expanded={suggestions.length > 0}
          spellcheck="false"
          autocomplete="off"
        />
      </label>
      {#if suggestions.length}
        <div class="suggest" role="listbox" aria-label="자동완성">
          {#each suggestions as s, i}
            <!-- 누르는 순간 고른다 (입력 칸이 포커스를 잃기 전에) -->
            <div
              class="item"
              class:selected={i === selected}
              role="option"
              aria-selected={i === selected}
              tabindex="-1"
              onpointerdown={(e) => {
                e.preventDefault();
                go(s.url);
              }}
            >
              {#if s.kind === "bookmark"}
                <svg class="kind" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-label="북마크"><path d="M12 4l2.5 5 5.5.8-4 3.9.9 5.5-4.9-2.6-4.9 2.6.9-5.5-4-3.9 5.5-.8z"></path></svg>
              {:else}
                <svg class="kind" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-label="방문 기록"><circle cx="12" cy="12" r="8"></circle><path d="M12 8v4l3 2"></path></svg>
              {/if}
              <span class="title">{s.title || readableUrl(s.url)}</span>
              <span class="url">{readableUrl(s.url)}</span>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    {#if sites.length}
      <section class="sites" aria-label="자주 가는 곳">
        {#each sites as s (s.url)}
          <button class="site" title={readableUrl(s.url)} onclick={(e) => open(e, s)} onauxclick={(e) => e.button === 1 && open(e, s)}>
            <span class="icon-box">
              {#if s.favicon}
                <img src={s.favicon} alt="" />
              {:else}
                <span class="letter" aria-hidden="true">{s.title.slice(0, 1).toUpperCase()}</span>
              {/if}
            </span>
            <span class="label">{s.title}</span>
          </button>
        {/each}
      </section>
    {/if}
  </div>
</div>

<style>
  .newtab {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: var(--newtab-padding);
    color: var(--fg);
  }
  .column {
    width: 100%;
    max-width: var(--address-max-width);
    display: flex;
    flex-direction: column;
    gap: var(--newtab-gap);
  }
  .search-wrap {
    position: relative;
  }
  .search {
    display: flex;
    align-items: center;
    gap: var(--newtab-search-icon-gap);
    width: 100%;
    height: var(--newtab-search-height);
    padding: var(--newtab-search-padding);
    background: var(--newtab-search-bg);
    border-radius: var(--newtab-search-radius);
    font-size: var(--newtab-search-font-size);
    color: var(--fg-muted);
    cursor: text;
  }
  .search input {
    flex: 1;
    min-width: 0;
    height: 100%;
    border: 0;
    outline: none;
    background: transparent;
    padding: 0;
    color: var(--fg);
    user-select: text;
  }
  .search input::placeholder {
    color: var(--fg-muted);
  }
  .suggest {
    position: absolute;
    z-index: 5;
    left: 0;
    right: 0;
    top: calc(100% + 4px);
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
    cursor: pointer;
  }
  .item:hover,
  .item.selected {
    background: var(--hover-bg);
  }
  .kind {
    flex: none;
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

  .sites {
    width: 100%;
    max-width: var(--newtab-grid-max-width);
    margin: 0 auto;
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: var(--newtab-grid-gap);
  }
  .site {
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--newtab-item-gap);
    padding: var(--newtab-item-padding);
    border-radius: var(--newtab-item-radius);
    font-size: var(--newtab-label-size);
  }
  .site:hover {
    background: var(--hover-bg);
  }
  .icon-box {
    width: var(--newtab-icon-box);
    height: var(--newtab-icon-box);
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--newtab-icon-bg);
    border-radius: var(--newtab-icon-radius);
  }
  .icon-box img {
    width: var(--newtab-icon-size);
    height: var(--newtab-icon-size);
    object-fit: contain;
  }
  .letter {
    width: var(--newtab-icon-size);
    height: var(--newtab-icon-size);
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    background: var(--fg);
    color: var(--card-bg);
    font-size: 11px;
    font-weight: 600;
  }
  .label {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
