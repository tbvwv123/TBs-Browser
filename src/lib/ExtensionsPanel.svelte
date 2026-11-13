<script lang="ts">
  // 확장 패널 (spec 6.4·6.6, 시안 K): 사이트 스크립트와 크롬 확장을 한 목록에 보여준다.
  // 펼친 사이드바에서는 사이드바 안에, 접힌 사이드바에서는 페이지 위 팝업 창 안에 그린다.
  // 줄은 켜기·끄기만 늘 보이고, 누르면 펼쳐져 팝업 열기·설정·고정·지우기가 나온다.
  import { host, scriptApplies, extApplies, matchSites, sitesLabel, type ScriptInfo, type ExtInfo } from "./api";

  let {
    scripts,
    extensions,
    url,
    onError,
    popupAnchor,
  }: {
    scripts: ScriptInfo[];
    extensions: ExtInfo[];
    url: string;
    onError: (e: unknown) => void;
    /** 확장 팝업을 띄울 자리 (메인 창 안쪽 좌표: 왼쪽, 아래 끝) */
    popupAnchor: () => { x: number; bottom: number };
  } = $props();

  type Item = {
    key: string;
    kind: "script" | "ext";
    id: string;
    name: string;
    icon: string | null;
    sites: string;
    enabled: boolean;
    pinned: boolean;
    error: string | null;
    applies: boolean;
    popup: boolean;
    options: boolean;
    /** 사이트 스크립트의 "이 탭에서 실행" 버튼 (spec 6.1 actions) */
    actions: { id: string; name: string }[];
    /** 저장소에 있는 새 판 (spec 6.8) */
    update: string | null;
  };

  let expanded = $state<string | null>(null);
  let confirmRemove = $state<string | null>(null);
  let installChoice = $state(false);
  let busy = $state(false);

  const NO_SITES = "적용 사이트 없음";

  const items = $derived.by((): Item[] => [
    ...scripts.map((s): Item => {
      const { hosts, all } = matchSites(s.matches);
      return {
        key: "s:" + s.id,
        kind: "script",
        id: s.id,
        name: s.name,
        icon: s.icon,
        sites: sitesLabel(hosts, all, NO_SITES),
        enabled: s.enabled,
        pinned: s.pinned,
        error: s.error,
        applies: s.enabled && scriptApplies(s, url),
        popup: false,
        options: s.settings.length > 0,
        actions: s.actions,
        update: s.update,
      };
    }),
    ...extensions.map((e): Item => ({
      key: "e:" + e.id,
      kind: "ext",
      id: e.id,
      name: e.name,
      icon: e.icon,
      sites: sitesLabel(e.sites, e.allSites, NO_SITES),
      enabled: e.enabled,
      pinned: e.pinned,
      error: e.error,
      applies: extApplies(e, url),
      popup: !!e.popup,
      options: !!e.options,
      actions: [],
      update: null,
    })),
  ]);
  const here = $derived(items.filter((i) => i.applies));
  const others = $derived(items.filter((i) => !i.applies));
  const pageHost = $derived.by(() => {
    try {
      return new URL(url).hostname.replace(/^www\./, "");
    } catch {
      return "";
    }
  });

  async function run(p: Promise<unknown>) {
    busy = true;
    await p.catch(onError);
    busy = false;
  }

  function toggle(i: Item) {
    if (i.kind === "script") run(host.scriptSetEnabled(i.id, !i.enabled));
    else run(host.extSetEnabled(i.id, !i.enabled));
  }

  function pin(i: Item) {
    if (i.kind === "script") run(host.scriptSetPinned(i.id, !i.pinned));
    else run(host.extSetPinned(i.id, !i.pinned));
  }

  function openPopup(i: Item) {
    const a = popupAnchor();
    run(host.extOpenPopup(i.id, a.x, a.bottom));
  }

  function openStore() {
    host.popupHide().catch(() => {});
    run(host.runShortcut("store"));
  }

  function install(folder: boolean) {
    installChoice = false;
    run(host.extInstallPick(folder));
  }
</script>

{#snippet icon(i: Item)}
  {#if i.icon}
    <img class="icon" src={i.icon} alt="" />
  {:else}
    <!-- 아이콘이 없으면 이름 첫 글자 (spec 6.4) -->
    <span class="icon letter" aria-hidden="true">{i.name.slice(0, 1)}</span>
  {/if}
{/snippet}

{#snippet row(i: Item)}
  {@const open = expanded === i.key}
  <div class="item" class:open>
    <div class="line">
      <button class="main" aria-expanded={open} onclick={() => ((expanded = open ? null : i.key), (confirmRemove = null))}>
        {@render icon(i)}
        <span class="text">
          <span class="title-line">
            <span class="name" class:off={!i.enabled} class:strong={open}>{i.name}</span>
            {#if i.kind === "ext"}
              <span class="tag" title="크롬 확장의 일부 기능은 동작하지 않을 수 있습니다.">크롬 확장</span>
            {:else}
              <span class="tag">스크립트</span>
            {/if}
          </span>
          <span class="sub">
            {#if i.update && !i.error}
              <!-- 업데이트: 색 대신 굵은 글자와 점 (시안 K) -->
              <span class="update"><span class="dot" aria-hidden="true"></span>업데이트 있음</span>
            {:else if i.error}
              <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4l9 16H3z"></path><path d="M12 10v4M12 17v.5"></path></svg>
              <span class="err" title={i.error}>{i.kind === "script" ? "파일 오류" : "오류"}</span>
            {:else}
              <span class="sites">{i.sites}</span>
            {/if}
          </span>
        </span>
      </button>
      {#if i.pinned && !open}
        <svg class="pinned-mark" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" role="img" aria-label="고정됨"><path d="M9 4h6l-1 6 3 3H7l3-3z"></path><path d="M12 13v7"></path></svg>
      {/if}
      <button
        class="switch"
        role="switch"
        aria-checked={i.enabled}
        aria-label="{i.name} 켜기"
        disabled={busy || (!!i.error && !i.enabled && i.kind === "script")}
        onclick={() => toggle(i)}
      >
        <span></span>
      </button>
    </div>
    {#if open}
      <div class="actions">
        {#if confirmRemove === i.key}
          <span class="confirm">{i.kind === "ext" ? "확장과 저장한 데이터를 지울까요?" : "휴지통으로 옮길까요?"}</span>
          <button class="small primary" disabled={busy} onclick={() => run(i.kind === "ext" ? host.extRemove(i.id) : host.scriptRemove(i.id))}>지우기</button>
          <button class="small" onclick={() => (confirmRemove = null)}>취소</button>
        {:else}
          {#if i.popup}
            <button class="small primary" disabled={!i.enabled} onclick={() => openPopup(i)}>팝업 열기</button>
          {/if}
          {#if i.update}
            <button class="small primary" disabled={busy} onclick={() => run(host.storeInstall(i.id))}>업데이트</button>
          {/if}
          {#if i.applies && !i.error}
            {#each i.actions as a (a.id)}
              <button class="small primary" title="이 탭에서 실행" onclick={() => run(host.scriptRunAction(i.id, a.id))}>{a.name}</button>
            {/each}
          {/if}
          {#if i.error}<span class="err-text">{i.error}</span>{/if}
          <span class="spacer"></span>
          {#if i.options}
            <button class="icon-button" aria-label="{i.name} 설정" title="설정" onclick={() => run(i.kind === "script" ? host.scriptOpenSettings(i.id) : host.extOpenOptions(i.id))}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 7h10M18 7h2M4 17h2M10 17h10"></path><circle cx="16" cy="7" r="2"></circle><circle cx="8" cy="17" r="2"></circle></svg>
            </button>
          {/if}
          <button class="icon-button" class:on={i.pinned} aria-label="{i.name} 고정" aria-pressed={i.pinned} title={i.pinned ? "고정 풀기" : "사이드바에 고정"} onclick={() => pin(i)}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill={i.pinned ? "currentColor" : "none"} stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 4h6l-1 6 3 3H7l3-3z"></path><path d="M12 13v7"></path></svg>
          </button>
          <button class="icon-button" aria-label="{i.name} 지우기" title="지우기" onclick={() => (confirmRemove = i.key)}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 7h14M10 7V5h4v2M7 7l1 13h8l1-13"></path></svg>
          </button>
        {/if}
      </div>
    {/if}
  </div>
{/snippet}

<section class="panel" aria-label="확장 프로그램">
  <div class="title">확장 프로그램</div>
  {#if here.length}
    <div class="group-label">이 탭에서 적용 중{pageHost ? ` · ${pageHost}` : ""}</div>
    {#each here as i (i.key)}{@render row(i)}{/each}
    {#if others.length}
      <div class="divider" aria-hidden="true"></div>
      <div class="group-label">그 밖</div>
    {/if}
  {/if}
  {#each others as i (i.key)}{@render row(i)}{/each}
  {#if !items.length}
    <p class="hint">설치한 것이 없습니다.</p>
  {/if}
  <div class="divider" aria-hidden="true"></div>
  <div class="footer">
    {#if installChoice}
      <button class="small" onclick={() => install(false)}>zip 파일</button>
      <button class="small" onclick={() => install(true)}>압축 푼 폴더</button>
      <button class="small ghost" onclick={() => (installChoice = false)}>취소</button>
    {:else}
      <button class="small" onclick={openStore}>찾아보기</button>
      <button class="small" disabled={busy} onclick={() => (installChoice = true)}>설치…</button>
      <span class="spacer"></span>
      <button class="icon-button" aria-label="스크립트 폴더 열기" title="스크립트 폴더 열기" onclick={() => run(host.scriptsOpenFolder())}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"></path></svg>
      </button>
      <button class="icon-button" aria-label="다시 불러오기" title="스크립트 다시 불러오기" disabled={busy} onclick={() => run(host.scriptsReload())}>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7"></path><path d="M20 4v7h-7"></path></svg>
      </button>
    {/if}
  </div>
</section>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    padding: 4px;
    background: var(--menu-bg);
    border-radius: var(--menu-radius);
    box-shadow: var(--menu-shadow);
    color: var(--fg);
  }
  .title {
    display: flex;
    align-items: center;
    height: var(--ext-title-height);
    padding: 0 8px;
    font-size: var(--section-title-font-size);
    font-weight: 600;
    color: var(--fg-muted);
  }
  .group-label {
    display: flex;
    align-items: center;
    height: var(--ext-group-label-height);
    padding: 0 8px;
    font-size: var(--sub-font-size);
    color: var(--fg-muted);
  }
  .divider {
    flex: none;
    height: 1px;
    margin: 4px 8px;
    background: var(--divider);
  }
  .item {
    border-radius: var(--tab-row-radius);
  }
  .item.open {
    background: var(--hover-bg);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    height: var(--ext-item-height);
    padding: 0 8px;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: var(--ext-item-gap);
    height: var(--ext-item-height);
  }
  .icon {
    flex: none;
    width: var(--ext-icon);
    height: var(--ext-icon);
    object-fit: contain;
  }
  .letter {
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: calc(var(--ext-icon) / 4);
    background: var(--letter-icon-bg);
    color: var(--prompt-primary-fg);
    font-size: calc(var(--ext-icon) * 0.62);
    font-weight: 700;
    line-height: 1;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .title-line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name.off {
    color: var(--fg-disabled);
  }
  .name.strong {
    font-weight: 600;
  }
  .tag {
    flex: none;
    height: var(--tag-height);
    padding: var(--tag-padding);
    border-radius: var(--tag-radius);
    background: var(--tag-bg);
    color: var(--fg-muted);
    font-size: var(--tag-font-size);
    line-height: var(--tag-height);
    white-space: nowrap;
  }
  .sub {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    font-size: var(--sub-font-size);
    color: var(--fg-muted);
  }
  .sub svg {
    flex: none;
    color: var(--fg);
  }
  .sites {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .update {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-weight: 600;
    color: var(--fg);
  }
  .update .dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--fg);
  }
  .err {
    flex: none;
    color: var(--fg);
    font-weight: 600;
  }
  .err-text {
    min-width: 0;
    font-size: var(--sub-font-size);
    color: var(--fg-muted);
  }
  .pinned-mark {
    flex: none;
    color: var(--fg-muted);
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
  .switch:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px;
    padding: 0 8px 8px var(--ext-item-indent);
  }
  .confirm {
    flex-basis: 100%;
    font-size: var(--sub-font-size);
    padding-bottom: 2px;
  }
  .spacer {
    flex: 1;
  }
  .small {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    height: var(--small-button-height);
    padding: var(--small-button-padding);
    border-radius: var(--small-button-radius);
    background: var(--field-bg);
    font-size: var(--small-button-font-size);
    font-weight: 500;
    white-space: nowrap;
  }
  .small.primary {
    background: var(--prompt-primary-bg);
    color: var(--prompt-primary-fg);
  }
  .small.ghost {
    background: none;
    color: var(--fg-muted);
  }
  .small:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .icon-button {
    flex: none;
    width: var(--small-icon-button);
    height: var(--small-icon-button);
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--small-button-radius);
    color: var(--fg-muted);
  }
  .icon-button:hover,
  .icon-button.on {
    background: var(--hover-bg);
    color: var(--fg);
  }
  .footer {
    display: flex;
    align-items: center;
    gap: 4px;
    height: var(--ext-footer-height);
    padding: 0 4px;
  }
  .hint {
    margin: 4px 8px 8px;
    font-size: var(--sub-font-size);
    color: var(--fg-muted);
  }
</style>
