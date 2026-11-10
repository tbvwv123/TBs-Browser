<script lang="ts">
  // 설정 페이지 (spec 5.4). 바꾸면 바로 저장한다.
  import "./page.css";
  import { host4, host10, whenText, type AdblockList, siteName, PERMISSION_NAMES, type Settings, type Theme, type SearchEngine, type PermissionKind } from "../api";
  import { ui, report } from "../store.svelte";

  const THEMES: [Theme, string][] = [
    ["light", "라이트"],
    ["dark", "다크"],
    ["system", "시스템 따름"],
  ];
  const ENGINES: [SearchEngine, string][] = [
    ["google", "Google"],
    ["naver", "네이버"],
    ["bing", "Bing"],
    ["duckduckgo", "DuckDuckGo"],
  ];

  async function save(patch: (s: Settings) => void) {
    if (!ui.settings) return;
    const next: Settings = JSON.parse(JSON.stringify(ui.settings));
    patch(next);
    ui.settings = await host4.settingsSet(next).catch((e) => {
      report(e);
      return ui.settings;
    });
  }

  async function pickFolder() {
    const dir = await host4.pickDownloadDir().catch((e) => {
      report(e);
      return null;
    });
    if (dir) save((s) => (s.downloadDir = dir));
  }

  // 사이트별로 기억한 권한 (spec 5.5). 지우면 다음에 다시 묻는다.
  const granted = $derived(
    Object.entries(ui.settings?.permissions ?? {}).flatMap(([site, kinds]) =>
      Object.entries(kinds).map(([kind, value]) => ({ site, kind: kind as PermissionKind, value })),
    ),
  );

  function forget(site: string, kind: string) {
    save((x) => {
      delete x.permissions[site]?.[kind];
      if (x.permissions[site] && !Object.keys(x.permissions[site]).length) delete x.permissions[site];
    });
  }

  // 광고 차단 (spec 6.7, 시안 L)
  let lists = $state<AdblockList[]>([]);
  let updating = $state(false);
  const loadLists = () => host10.adblockLists().then((l) => (lists = l), report);
  // 처음 열 때와, 목록을 새로 받아 호스트가 settings_changed를 다시 보낼 때 읽는다
  $effect(() => {
    void ui.settings;
    loadLists();
  });

  async function updateNow() {
    updating = true;
    await host10.adblockUpdateNow().catch(report);
    setTimeout(() => (updating = false), 1500);
  }

  function num(v: string, min: number, max: number, fallback: number) {
    const n = Math.round(Number(v));
    return Number.isFinite(n) ? Math.min(max, Math.max(min, n)) : fallback;
  }
</script>

<div class="page">
  <div class="page-inner">
    <div class="page-head">
      <h1 class="page-title">설정</h1>
    </div>
    {#if ui.settings}
      {@const s = ui.settings}
      <section>
        <h2 class="page-section-title">테마</h2>
        <div class="choices" role="radiogroup" aria-label="테마">
          {#each THEMES as [value, label]}
            <label class="choice">
              <input type="radio" name="theme" checked={s.theme === value} onchange={() => save((x) => (x.theme = value))} />
              {label}
            </label>
          {/each}
        </div>
      </section>

      <section>
        <h2 class="page-section-title">기본 검색 엔진</h2>
        <select class="page-field" value={s.searchEngine} onchange={(e) => save((x) => (x.searchEngine = (e.currentTarget as HTMLSelectElement).value as SearchEngine))}>
          {#each ENGINES as [value, label]}<option {value}>{label}</option>{/each}
        </select>
      </section>

      <section>
        <h2 class="page-section-title">다운로드 폴더</h2>
        <div class="row">
          <span class="page-ellipsis path">{s.downloadDir || "Windows의 다운로드 폴더"}</span>
          <button class="page-button" onclick={pickFolder}>바꾸기</button>
          {#if s.downloadDir}<button class="page-button" onclick={() => save((x) => (x.downloadDir = ""))}>기본값으로</button>{/if}
        </div>
      </section>

      <section>
        <h2 class="page-section-title">시작할 때</h2>
        <label class="choice">
          <input type="checkbox" checked={s.restoreTabs} onchange={(e) => save((x) => (x.restoreTabs = (e.currentTarget as HTMLInputElement).checked))} />
          이전에 열었던 탭 다시 열기
        </label>
      </section>

      <section>
        <h2 class="page-section-title">메모리 관리</h2>
        <div class="numbers">
          <label>안 본 지 <input class="page-field num" type="number" min="1" max="1440" value={s.memory.suspendAfterMinutes}
              onchange={(e) => save((x) => (x.memory.suspendAfterMinutes = num((e.currentTarget as HTMLInputElement).value, 1, 1440, 5)))} /> 분 뒤 재우기</label>
          <label>안 본 지 <input class="page-field num" type="number" min="1" max="1440" value={s.memory.discardAfterMinutes}
              onchange={(e) => save((x) => (x.memory.discardAfterMinutes = num((e.currentTarget as HTMLInputElement).value, 1, 1440, 30)))} /> 분 뒤 버리기</label>
          <label>깨어 있는 탭 최대 <input class="page-field num" type="number" min="1" max="200" value={s.memory.maxAwakeTabs}
              onchange={(e) => save((x) => (x.memory.maxAwakeTabs = num((e.currentTarget as HTMLInputElement).value, 1, 200, 12)))} /> 개</label>
        </div>
      </section>
      <section>
        <h2 class="page-section-title">사이트 권한</h2>
        {#if granted.length}
          <ul class="perms">
            {#each granted as g (g.site + g.kind)}
              <li class="row">
                <span class="page-ellipsis path">{siteName(g.site)}</span>
                <span class="page-muted">{PERMISSION_NAMES[g.kind] ?? g.kind} · {g.value === "allow" ? "허용" : "차단"}</span>
                <button class="page-button" onclick={() => forget(g.site, g.kind)}>지우기</button>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="page-muted">기억한 권한이 없습니다.</p>
        {/if}
      </section>

      <section>
        <h2 class="page-section-title">광고 차단</h2>
        <div class="ab-row">
          <span class="ab-text">
            <span>광고 차단 사용</span>
            <span class="ab-sub">끄면 모든 사이트에서 광고를 막지 않습니다</span>
          </span>
          <button class="switch" role="switch" aria-checked={s.adblock.enabled} aria-label="광고 차단 사용" onclick={() => save((x) => (x.adblock.enabled = !x.adblock.enabled))}><span></span></button>
        </div>
        <div class="ab-subhead">
          <h3>차단 목록</h3>
          <button class="page-button" disabled={updating} onclick={updateNow}>{updating ? "받는 중…" : "지금 갱신"}</button>
        </div>
        <div class="ab-list">
          {#each lists as l (l.id)}
            <div class="ab-row">
              <span class="ab-text">
                <span>{l.name}</span>
                <span class="ab-sub">{l.kind} · 규칙 {l.rules.toLocaleString("ko-KR")}개 · {whenText(l.updated)} 갱신</span>
              </span>
              <button class="switch" role="switch" aria-checked={l.enabled} aria-label={l.name} onclick={() => save((x) => (x.adblock.lists[l.id] = !l.enabled))}><span></span></button>
            </div>
          {/each}
        </div>
        <div class="ab-subhead">
          <h3>광고 차단을 끈 사이트</h3>
          {#if s.adblock.disabledSites.length}
            <button class="page-button ghost" onclick={() => save((x) => (x.adblock.disabledSites = []))}>모두 지우기</button>
          {/if}
        </div>
        {#if s.adblock.disabledSites.length}
          <div class="ab-list">
            {#each s.adblock.disabledSites as site (site)}
              <div class="ab-row short">
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="8.5"></circle><path d="M3.5 12h17M12 3.5c3 3 3 14 0 17M12 3.5c-3 3-3 14 0 17"></path></svg>
                <span class="ab-text">{site}</span>
                <button class="ab-x" aria-label="{site} 지우기" onclick={() => save((x) => (x.adblock.disabledSites = x.adblock.disabledSites.filter((d) => d !== site)))}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"></path></svg>
                </button>
              </div>
            {/each}
          </div>
        {:else}
          <p class="page-muted">없음</p>
        {/if}
      </section>

      <section>
        <h2 class="page-section-title">확장 프로그램</h2>
        <div class="row">
          <span class="page-ellipsis path">사이트 스크립트 {ui.scripts.length}개 · 크롬 확장 {ui.extensions.length}개</span>
          <button class="page-button" onclick={() => host4.runShortcut("extensions").catch(report)}>관리</button>
          <button class="page-button" onclick={() => host4.runShortcut("store").catch(report)}>찾아보기</button>
        </div>
      </section>
    {/if}
  </div>
</div>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .choices {
    display: flex;
    gap: 16px;
  }
  .choice {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .path {
    flex: 1;
  }
  .perms {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .ab-row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 52px;
    border-bottom: 1px solid var(--divider);
  }
  .ab-row.short {
    min-height: var(--page-row-height);
    color: var(--fg-muted);
  }
  .ab-row.short .ab-text {
    color: var(--fg);
  }
  .ab-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .ab-sub {
    font-size: var(--sub-font-size);
    color: var(--fg-muted);
  }
  .ab-subhead {
    display: flex;
    align-items: center;
    gap: 12px;
    height: var(--page-subhead-height);
    margin-top: 4px;
  }
  .ab-subhead h3 {
    flex: 1;
    margin: 0;
    font-size: var(--page-subhead-size);
    font-weight: 600;
    color: var(--fg-muted);
  }
  .ab-list {
    border-top: 1px solid var(--divider);
  }
  .ab-x {
    flex: none;
    width: var(--small-icon-button);
    height: var(--small-icon-button);
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--small-button-radius);
    color: var(--fg-muted);
  }
  .ab-x:hover {
    background: var(--hover-bg);
  }
  .switch {
    flex: none;
    width: var(--switch-width);
    height: var(--switch-height);
    padding: var(--switch-padding);
    border-radius: var(--switch-radius);
    background: var(--switch-off-bg);
    display: flex;
    justify-content: flex-start;
  }
  .switch[aria-checked="true"] {
    background: var(--switch-on-bg);
    justify-content: flex-end;
  }
  .switch span {
    display: block;
    width: var(--switch-knob);
    height: var(--switch-knob);
    border-radius: 50%;
    background: var(--switch-knob-bg);
  }
  .numbers {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .num {
    width: 64px;
    margin: 0 4px;
  }
  select.page-field {
    align-self: flex-start;
    min-width: 200px;
  }
</style>
