<script lang="ts">
  // 사이트 스크립트 목록 (spec 6.4): 이름, 적용 사이트, 켜고 끄기, 고정.
  // 현재 탭에 적용 중인 스크립트를 맨 위에 모아 보여준다.
  import { host5, scriptApplies, type ScriptInfo } from "./api";
  import { ui, report, activeTab } from "./store.svelte";

  let { compact = false }: { compact?: boolean } = $props();
  let busy = $state(false);

  const url = $derived(activeTab()?.url ?? "");
  const here = $derived(ui.scripts.filter((s) => scriptApplies(s, url)));
  const others = $derived(ui.scripts.filter((s) => !scriptApplies(s, url)));

  function sites(s: ScriptInfo) {
    return s.matches.map((m) => m.replace(/^https?:\/\//, "").replace(/\/\*$/, "")).join(", ");
  }

  // 켤 때 확인이 필요하면 호스트가 확인 창을 띄우고 켜지 않는다. 칸은 실제 상태로 되돌려 두고, 바뀌면 목록이 다시 그린다
  function toggle(s: ScriptInfo, el: HTMLInputElement) {
    const want = el.checked;
    el.checked = s.enabled;
    host5.scriptSetEnabled(s.id, want).catch(report);
  }

  async function reload() {
    busy = true;
    ui.scripts = await host5.scriptsReload().catch((e) => {
      report(e);
      return ui.scripts;
    });
    busy = false;
  }
</script>

{#snippet item(s: ScriptInfo)}
  <div class="item" class:off={!s.enabled}>
    {#if s.icon}
      <img class="badge img" src={s.icon} alt="" />
    {:else}
      <span class="badge" aria-hidden="true">{s.name.slice(0, 1)}</span>
    {/if}
    <div class="text">
      <span class="name">{s.name}</span>
      <span class="sites" title={s.matches.join("\n")}>{sites(s)}</span>
      {#if s.error}<span class="err">{s.error}</span>{/if}
    </div>
    {#if s.settings.length}
      <button class="pin" aria-label="{s.name} 설정" title="설정" onclick={() => host5.scriptOpenSettings(s.id).catch(report)}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 7h10M18 7h2M4 17h2M10 17h10"></path><circle cx="16" cy="7" r="2"></circle><circle cx="8" cy="17" r="2"></circle></svg>
      </button>
    {/if}
    <button
      class="pin"
      class:on={s.pinned}
      aria-pressed={s.pinned}
      aria-label={s.pinned ? "사이드바에서 고정 풀기" : "사이드바에 고정"}
      title={s.pinned ? "고정 풀기" : "사이드바에 고정"}
      onclick={() => host5.scriptSetPinned(s.id, !s.pinned).catch(report)}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill={s.pinned ? "currentColor" : "none"} stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><path d="M9 4h6l-1 6 4 3v2H6v-2l4-3z"></path><path d="M12 15v5" stroke-linecap="round"></path></svg>
    </button>
    <label class="switch" title={s.enabled ? "끄기" : "켜기"}>
      <input type="checkbox" role="switch" checked={s.enabled} disabled={!!s.error && !s.enabled} aria-label="{s.name} 켜기" onchange={(e) => toggle(s, e.currentTarget as HTMLInputElement)} />
      <span aria-hidden="true"></span>
    </label>
  </div>
{/snippet}

<div class="list" class:compact>
  <div class="actions">
    <button class="action" onclick={() => host5.scriptsOpenFolder().catch(report)}>스크립트 폴더 열기</button>
    <button class="action" disabled={busy} onclick={reload}>다시 불러오기</button>
  </div>
  {#if !ui.scripts.length}
    <p class="hint">스크립트가 없습니다.</p>
  {/if}
  {#if here.length}
    <h3 class="title">이 탭에 적용 중</h3>
    {#each here as s (s.id)}{@render item(s)}{/each}
  {/if}
  {#if others.length}
    {#if here.length}<h3 class="title">다른 스크립트</h3>{/if}
    {#each others as s (s.id)}{@render item(s)}{/each}
  {/if}
</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .actions {
    display: flex;
    gap: 6px;
    margin-bottom: 4px;
  }
  .action {
    height: var(--search-height);
    padding: 0 10px;
    border-radius: var(--search-radius);
    background: var(--field-bg);
    font-size: var(--search-font-size);
  }
  .action:hover:not(:disabled) {
    background: var(--divider);
  }
  .title {
    margin: 8px 6px 2px;
    font-size: var(--section-title-font-size);
    font-weight: var(--section-title-weight);
    color: var(--fg-muted);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 4px 6px;
    border-radius: var(--tab-row-radius);
  }
  .item:hover {
    background: var(--hover-bg);
  }
  .item.off .name,
  .item.off .badge {
    opacity: 0.55;
  }
  .badge.img {
    background: none;
    object-fit: contain;
  }
  .badge {
    flex: none;
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 6px;
    background: var(--fg);
    color: var(--card-bg);
    font-size: 11px;
    font-weight: 600;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .name,
  .sites {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sites,
  .err,
  .hint {
    font-size: var(--search-font-size);
    color: var(--fg-muted);
  }
  .err {
    white-space: normal;
  }
  .hint {
    margin: 6px;
  }
  .pin {
    flex: none;
    width: 26px;
    height: 26px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--bookmark-row-radius);
    color: var(--fg-muted);
  }
  .pin:hover {
    background: var(--hover-bg);
  }
  .pin.on {
    color: var(--fg);
  }
  .switch {
    flex: none;
    position: relative;
    width: 30px;
    height: 18px;
    cursor: pointer;
  }
  .switch input {
    position: absolute;
    inset: 0;
    opacity: 0;
    margin: 0;
    cursor: pointer;
  }
  .switch span {
    position: absolute;
    inset: 0;
    border-radius: 9px;
    background: var(--divider);
    transition: background 0.15s;
  }
  .switch span::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--card-bg);
    transition: transform 0.15s;
  }
  .switch input:checked + span {
    background: var(--fg);
  }
  .switch input:checked + span::after {
    transform: translateX(12px);
  }
  .switch input:focus-visible + span {
    outline: 2px solid var(--fg-muted);
    outline-offset: 2px;
  }
</style>
