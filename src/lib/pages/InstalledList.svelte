<script lang="ts">
  // 확장 프로그램 관리의 "설치됨" 탭 (tbs://extensions). 사이트 스크립트와 크롬 확장을 한 목록에서
  // 켜고 끄고, 설정·업데이트·지우기를 한다. 줄 모양은 찾아보기(시안 O)를 따른다.
  import { host, sitesLabel } from "../api";
  import { ui, report } from "../store.svelte";

  type Item = {
    key: string;
    kind: "script" | "ext";
    id: string;
    name: string;
    version: string;
    icon: string | null;
    sites: string;
    enabled: boolean;
    error: string | null;
    update: string | null;
    settings: boolean;
  };

  const items = $derived.by((): Item[] =>
    [
      ...ui.scripts.map((s): Item => ({
        key: "s:" + s.id,
        kind: "script",
        id: s.id,
        name: s.name,
        version: s.version,
        icon: s.icon,
        sites: sitesLabel(s.sites, s.allSites),
        enabled: s.enabled,
        error: s.error,
        update: s.update,
        settings: s.settings.length > 0,
      })),
      ...ui.extensions.map((e): Item => ({
        key: "e:" + e.id,
        kind: "ext",
        id: e.id,
        name: e.name,
        version: e.version,
        icon: e.icon,
        sites: sitesLabel(e.sites, e.allSites),
        enabled: e.enabled,
        error: e.error,
        update: null,
        settings: !!e.options,
      })),
    ].sort((a, b) => a.name.localeCompare(b.name, "ko")),
  );

  let confirmRemove = $state<string | null>(null);

  function act(p: Promise<unknown>) {
    p.catch(report);
  }
  function toggle(i: Item) {
    act(i.kind === "script" ? host.scriptSetEnabled(i.id, !i.enabled) : host.extSetEnabled(i.id, !i.enabled));
  }
  function openSettings(i: Item) {
    act(i.kind === "script" ? host.scriptOpenSettings(i.id) : host.extOpenOptions(i.id));
  }
  function remove(i: Item) {
    confirmRemove = null;
    act(i.kind === "script" ? host.scriptRemove(i.id) : host.extRemove(i.id));
  }
</script>

<div class="list">
  {#each items as i (i.key)}
    <div class="row">
      {#if i.icon}
        <img class="icon" class:off={!i.enabled} src={i.icon} alt="" />
      {:else}
        <span class="icon letter" class:off={!i.enabled} aria-hidden="true">{i.name.slice(0, 1)}</span>
      {/if}
      <div class="text">
        <div class="name-line">
          <span class="name">{i.name}</span>
          <span class="tag">{i.kind === "ext" ? "크롬 확장" : "스크립트"}</span>
          {#if i.version}<span class="meta">버전 {i.version}</span>{/if}
        </div>
        {#if i.error}
          <span class="meta ellipsis" title={i.error}>{i.error}</span>
        {:else}
          <span class="meta">{i.sites}</span>
        {/if}
      </div>
      {#if confirmRemove === i.key}
        <span class="meta">{i.kind === "ext" ? "확장과 저장한 데이터를 지울까요?" : "휴지통으로 옮길까요?"}</span>
        <button class="btn primary" onclick={() => remove(i)}>지우기</button>
        <button class="btn" onclick={() => (confirmRemove = null)}>취소</button>
      {:else}
        {#if i.update}
          <button class="btn" onclick={() => act(host.storeInstall(i.id))}><span class="dot" aria-hidden="true"></span>업데이트</button>
        {/if}
        {#if i.settings}
          <button class="icon-btn" aria-label="{i.name} 설정" title="설정" onclick={() => openSettings(i)}>
            <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 7h10M18 7h2M4 17h2M10 17h10"></path><circle cx="16" cy="7" r="2"></circle><circle cx="8" cy="17" r="2"></circle></svg>
          </button>
        {/if}
        <button class="icon-btn" aria-label="{i.name} 지우기" title="지우기" onclick={() => (confirmRemove = i.key)}>
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 7h14M10 7V5h4v2M7 7l1 13h8l1-13"></path></svg>
        </button>
        <button class="switch" role="switch" aria-checked={i.enabled} aria-label="{i.name} 켜기" disabled={!!i.error && !i.enabled && i.kind === "script"} onclick={() => toggle(i)}><span></span></button>
      {/if}
    </div>
  {/each}
  {#if !items.length}
    <p class="empty">설치한 것이 없습니다.</p>
  {/if}
</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: var(--store-row-height);
    padding: var(--store-row-padding);
    border-bottom: 1px solid var(--divider);
  }
  .icon {
    flex: none;
    width: var(--ext-icon-large);
    height: var(--ext-icon-large);
    margin-right: 4px;
    border-radius: calc(var(--ext-icon-large) / 4);
    object-fit: contain;
  }
  .icon.off {
    opacity: 0.45;
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
    align-items: center;
    gap: 8px;
  }
  .name {
    font-size: var(--store-name-size);
    font-weight: 600;
  }
  .tag {
    height: var(--tag-height);
    padding: var(--tag-padding);
    border-radius: var(--tag-radius);
    background: var(--tag-bg);
    color: var(--fg-muted);
    font-size: var(--tag-font-size);
    line-height: var(--tag-height);
    white-space: nowrap;
  }
  .meta {
    font-size: var(--sub-font-size);
    color: var(--fg-muted);
  }
  .ellipsis {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
  .icon-btn {
    flex: none;
    width: var(--button-height);
    height: var(--button-height);
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--button-radius);
    color: var(--fg-muted);
  }
  .icon-btn:hover {
    background: var(--hover-bg);
  }
  .switch {
    flex: none;
    width: var(--switch-width);
    height: var(--switch-height);
    margin-left: 6px;
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
  .empty {
    margin: 40px 0;
    text-align: center;
    color: var(--fg-muted);
  }
</style>
