<script lang="ts">
  // 사이트 스크립트 설정 화면 (spec 6.1 settings·6.4, 시안 N). 주소: tbs://scripts/<id>
  // script.json의 settings 칸을 그리고, 바꾸면 바로 저장한다. 열린 페이지에는 tbs.onSettingsChanged로 알린다.
  import "./page.css";
  import { host5, type ScriptInfo, type SettingField } from "../api";
  import { ui, report } from "../store.svelte";

  let { id }: { id: string } = $props();

  const script = $derived(ui.scripts.find((s) => s.id === id) ?? null);
  let openSelect = $state<string | null>(null);
  let confirmRemove = $state(false);
  let draft = $state<Record<string, string>>({});

  /** 칸을 차례대로 구역으로 묶는다. 글자 목록은 혼자 한 구역이고 제목에 개수를 붙인다 */
  type Group = { title: string; list: SettingField | null; fields: SettingField[] };
  const groups = $derived.by((): Group[] => {
    const out: Group[] = [];
    for (const f of script?.settings ?? []) {
      if (f.type === "list") {
        out.push({ title: f.label, list: f, fields: [] });
        continue;
      }
      const last = out[out.length - 1];
      if (last && !last.list && last.title === f.section) last.fields.push(f);
      else out.push({ title: f.section, list: null, fields: [f] });
    }
    return out;
  });

  function sites(s: ScriptInfo): string {
    if (s.matches.some((m) => m === "*" || /^(\*|https?):\/\/\*\/?\*?$/.test(m) || m === "<all_urls>")) return "모든 사이트";
    const hosts = [...new Set(s.matches.map((m) => m.replace(/^\w+:\/\//, "").split("/")[0].replace(/^\*\./, "").replace(/^www\./, "")))].filter((h) => h && h !== "*");
    return hosts.length > 1 ? `${hosts[0]} 외 ${hosts.length - 1}곳` : (hosts[0] ?? "");
  }

  function set(key: string, value: unknown) {
    if (!script) return;
    host5.scriptSetValue(script.id, key, value).catch(report);
  }

  function value<T>(f: SettingField): T {
    return (script?.values[f.key] ?? f.default) as T;
  }

  function listOf(f: SettingField): string[] {
    const v = script?.values[f.key];
    return Array.isArray(v) ? (v as string[]) : [];
  }

  function addToList(f: SettingField) {
    const t = (draft[f.key] ?? "").trim();
    if (!t) return;
    const cur = listOf(f);
    if (!cur.includes(t)) set(f.key, [...cur, t]);
    draft[f.key] = "";
  }

  function setNumber(f: SettingField, el: HTMLInputElement) {
    const n = Number(el.value);
    if (!Number.isFinite(n)) {
      el.value = String(value<number>(f));
      return;
    }
    set(f.key, n);
  }

  function toggleEnabled(s: ScriptInfo) {
    host5.scriptSetEnabled(s.id, !s.enabled).catch(report);
  }

  async function remove(s: ScriptInfo) {
    confirmRemove = false;
    await host5.scriptRemove(s.id).catch(report);
  }

  function optionLabel(f: SettingField): string {
    const v = value<string>(f);
    return f.options.find((o) => o.value === v)?.label ?? v;
  }

  function closeSelect(e: MouseEvent) {
    if (openSelect && !(e.target as Element).closest(".select")) openSelect = null;
  }
</script>

<svelte:window onclick={closeSelect} onkeydown={(e) => e.key === "Escape" && (openSelect = null)} />

{#snippet control(f: SettingField)}
  {#if f.type === "toggle"}
    <button class="switch" role="switch" aria-checked={!!value(f)} aria-label={f.label} onclick={() => set(f.key, !value(f))}><span></span></button>
  {:else if f.type === "number"}
    <input class="field num" inputmode="numeric" aria-label={f.label} value={value<number>(f)} onchange={(e) => setNumber(f, e.currentTarget)} />
    <span class="unit">{f.unit}</span>
  {:else if f.type === "text"}
    <input class="field text" aria-label={f.label} placeholder={f.placeholder} value={value<string>(f)} onchange={(e) => set(f.key, e.currentTarget.value)} />
  {:else if f.type === "select"}
    <div class="select">
      <button class="field select-button" aria-haspopup="listbox" aria-expanded={openSelect === f.key} aria-label={f.label} onclick={() => (openSelect = openSelect === f.key ? null : f.key)}>
        <span class="page-ellipsis">{optionLabel(f)}</span>
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6"></path></svg>
      </button>
      {#if openSelect === f.key}
        <div class="menu" role="listbox" aria-label={f.label}>
          {#each f.options as o (o.value)}
            <button class="menu-item" role="option" aria-selected={o.value === value(f)} onclick={() => ((openSelect = null), set(f.key, o.value))}>
              <span class="page-ellipsis">{o.label}</span>
              {#if o.value === value(f)}
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"></path></svg>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
{/snippet}

<div class="page">
  <div class="page-inner">
    {#if !script}
      <p class="page-empty">스크립트를 찾지 못했습니다.</p>
    {:else}
      <div class="head">
        {#if script.icon}
          <img class="icon" src={script.icon} alt="" />
        {:else}
          <span class="icon letter" aria-hidden="true">{script.name.slice(0, 1)}</span>
        {/if}
        <div class="head-text">
          <h1 class="title">{script.name}</h1>
          <span class="sub">{[script.version && `버전 ${script.version}`, sites(script)].filter(Boolean).join(" · ")}</span>
        </div>
        <button class="switch" role="switch" aria-checked={script.enabled} aria-label="{script.name} 켜기" disabled={!!script.error && !script.enabled} onclick={() => toggleEnabled(script)}><span></span></button>
        {#if confirmRemove}
          <span class="sub">휴지통으로 옮길까요?</span>
          <button class="small primary" onclick={() => remove(script)}>지우기</button>
          <button class="small" onclick={() => (confirmRemove = false)}>취소</button>
        {:else}
          <button class="ghost" onclick={() => (confirmRemove = true)}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 7h14M10 7V5h4v2M7 7l1 13h8l1-13"></path></svg>
            스크립트 지우기
          </button>
        {/if}
      </div>
      {#if script.description}<p class="desc">{script.description}</p>{/if}
      {#if script.error}<p class="desc">{script.error}</p>{/if}

      {#each groups as g, gi (gi)}
        {#if g.list}
          {@const f = g.list}
          {@const items = listOf(f)}
          <div class="subhead"><h2>{f.label} · {items.length}개</h2></div>
          {#if f.description}<p class="desc">{f.description}</p>{/if}
          <form class="add" onsubmit={(e) => (e.preventDefault(), addToList(f))}>
            <input class="field add-input" aria-label={f.placeholder || f.label} placeholder={f.placeholder || f.label} bind:value={draft[f.key]} />
            <button class="small-strong" type="submit">더하기</button>
          </form>
          {#if items.length}
            <div class="rows boxed">
              {#each items as item (item)}
                <div class="row">
                  <span class="page-ellipsis">{item}</span>
                  <span class="spacer"></span>
                  <button class="x" aria-label="{item} 지우기" onclick={() => set(f.key, items.filter((x) => x !== item))}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"></path></svg>
                  </button>
                </div>
              {/each}
            </div>
          {/if}
        {:else}
          {#if g.title}<div class="subhead"><h2>{g.title}</h2></div>{/if}
          <div class="rows">
            {#each g.fields as f (f.key)}
              <div class="row">
                {#if f.icon}
                  <!-- 한 색 아이콘을 글자색으로 칠한다 (다크 테마에서도 보이게) -->
                  <span class="field-icon" aria-hidden="true"><span class="mask" style="-webkit-mask-image: url({f.icon}); mask-image: url({f.icon})"></span></span>
                {/if}
                <span class="label">
                  <span>{f.label}</span>
                  {#if f.description}<span class="sub">{f.description}</span>{/if}
                </span>
                <span class="spacer"></span>
                {@render control(f)}
              </div>
            {/each}
          </div>
        {/if}
      {/each}
    {/if}
  </div>
</div>

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 14px;
    padding-bottom: 20px;
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
  .head-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .title {
    margin: 0;
    font-size: var(--page-title-size);
    font-weight: 600;
    line-height: 1.25;
  }
  .sub {
    font-size: var(--page-subhead-size);
    color: var(--fg-muted);
  }
  .desc {
    margin: 0;
    color: var(--fg-muted);
    font-size: var(--page-subhead-size);
    white-space: pre-line;
  }
  .ghost {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    height: var(--button-height);
    padding: 0 10px;
    border-radius: var(--button-radius);
    color: var(--fg-muted);
    font-size: var(--button-font-size);
  }
  .ghost:hover {
    background: var(--hover-bg);
  }
  .small {
    flex: none;
    height: var(--button-height);
    padding: var(--button-padding);
    border-radius: var(--button-radius);
    background: var(--field-bg);
    font-size: var(--button-font-size);
    font-weight: 500;
  }
  .small.primary,
  .small-strong {
    background: var(--prompt-primary-bg);
    color: var(--prompt-primary-fg);
  }
  .small-strong {
    flex: none;
    height: var(--button-height);
    padding: var(--button-padding);
    border-radius: var(--button-radius);
    font-size: var(--button-font-size);
    font-weight: 500;
    white-space: nowrap;
  }
  .subhead {
    display: flex;
    align-items: center;
    height: var(--page-subhead-height);
    margin-top: 12px;
    margin-bottom: -16px; /* 아래 줄과 붙인다 (page-inner의 사이 간격을 지운다) */
  }
  .subhead h2 {
    margin: 0;
    font-size: var(--page-subhead-size);
    font-weight: 600;
    color: var(--fg-muted);
  }
  .rows {
    border-top: 1px solid var(--divider);
  }
  .rows.boxed {
    border-bottom: 1px solid var(--divider);
    margin-top: -6px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: var(--page-row-height);
    border-bottom: 1px solid var(--divider);
  }
  .row:last-child {
    border-bottom: 0;
  }
  .label {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .spacer {
    flex: 1;
  }
  .field-icon {
    flex: none;
    width: var(--field-icon-box);
    height: var(--field-icon-box);
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--field-bg);
    border-radius: var(--field-icon-radius);
  }
  .mask {
    width: 16px;
    height: 16px;
    background: var(--fg);
    -webkit-mask-size: contain;
    mask-size: contain;
    -webkit-mask-repeat: no-repeat;
    mask-repeat: no-repeat;
  }
  .field {
    flex: none;
    height: var(--field-height);
    padding: 0 10px;
    border: 0;
    border-radius: var(--button-radius);
    background: var(--field-bg);
    outline: none;
    font: inherit;
    color: inherit;
  }
  .field:focus-visible {
    box-shadow: 0 0 0 2px var(--fg);
  }
  .num {
    width: var(--number-input-width);
    text-align: right;
  }
  .unit {
    width: 18px;
    margin-left: -6px;
    font-size: var(--page-subhead-size);
    color: var(--fg-muted);
  }
  .text {
    width: var(--text-input-width);
  }
  .select {
    position: relative;
    flex: none;
  }
  .select-button {
    width: var(--select-width);
    height: var(--select-height);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    text-align: left;
  }
  .select-button svg {
    flex: none;
    color: var(--fg-muted);
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 5;
    width: var(--menu-width);
    padding: 4px;
    background: var(--menu-bg);
    border-radius: var(--menu-radius);
    box-shadow: var(--menu-shadow);
    display: flex;
    flex-direction: column;
  }
  .menu-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    height: var(--menu-item-height);
    padding: 0 10px;
    border-radius: var(--menu-item-radius);
    text-align: left;
  }
  .menu-item:hover {
    background: var(--hover-bg);
  }
  .add {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
  }
  .add-input {
    flex: 1;
    height: var(--list-input-height);
    padding: 0 12px;
  }
  .x {
    flex: none;
    width: var(--small-icon-button);
    height: var(--small-icon-button);
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--small-button-radius);
    color: var(--fg-muted);
  }
  .x:hover {
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
  .switch:disabled {
    opacity: 0.5;
  }
  .switch span {
    display: block;
    width: var(--switch-knob);
    height: var(--switch-knob);
    border-radius: 50%;
    background: var(--switch-knob-bg);
  }
</style>
