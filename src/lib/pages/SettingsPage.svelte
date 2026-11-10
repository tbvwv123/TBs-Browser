<script lang="ts">
  // 설정 페이지 (spec 5.4). 바꾸면 바로 저장한다.
  import "./page.css";
  import ScriptList from "../ScriptList.svelte";
  import { host4, siteName, PERMISSION_NAMES, type Settings, type Theme, type SearchEngine, type PermissionKind } from "../api";
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
        <p class="page-muted">안 보는 탭을 재우고 버리는 기준입니다. 소리를 내는 탭, 다운로드 중인 탭, "항상 유지" 탭은 빼고 적용합니다.</p>
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
          <p class="page-muted">사이트가 카메라, 마이크, 알림, 위치를 요청하면 허용·차단을 묻고 여기에 기억합니다.</p>
        {/if}
      </section>

      <section>
        <h2 class="page-section-title">사이트 스크립트</h2>
        <ScriptList />
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
