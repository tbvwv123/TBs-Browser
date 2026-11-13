<script lang="ts">
  // 광고 차단 팝업 (spec 6.7, 시안 L): 이 페이지에서 막은 개수, 이 사이트에서 켜기·끄기, 목록 갱신 시각.
  // 주소창의 방패 버튼을 누르면 페이지 위 팝업 창에 뜬다.
  import { host, whenText, type AdblockStatus } from "./api";

  let { status, onError }: { status: AdblockStatus; onError: (e: unknown) => void } = $props();

  // 이 팝업에서 바꾼 값 (끄면 "새로고침해야 적용됩니다"를 보인다)
  let siteOn = $state(true);
  let changed = $state(false);
  $effect(() => {
    siteOn = status.siteEnabled;
    changed = false;
  });

  function toggle() {
    if (!status.site) return;
    siteOn = !siteOn;
    changed = true;
    host.adblockSetSite(status.site, siteOn).catch(onError);
  }

  function openSettings() {
    host.popupHide().catch(() => {});
    host.runShortcut("settings").catch(onError);
  }
</script>

<section class="popup" aria-label="광고 차단">
  <div class="head">
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M12 3l7 3v5c0 4.5-3 8-7 10-4-2-7-5.5-7-10V6z"></path>
      {#if !siteOn || !status.enabled}<path d="M4 4l16 16"></path>{/if}
    </svg>
    <span>광고 차단</span>
  </div>
  {#if !status.enabled}
    <p class="text">광고 차단이 꺼져 있습니다.</p>
  {:else if siteOn && !changed}
    <div class="count">
      <span class="num">{status.count}</span>
      <span>개 막음</span>
      <span class="sub">이 페이지에서</span>
    </div>
  {:else if siteOn}
    <p class="text">이 사이트에서 광고를 막습니다.</p>
  {:else}
    <p class="text">이 사이트에서는 광고를 막지 않습니다.</p>
  {/if}
  {#if status.enabled && status.site}
    <div class="divider" aria-hidden="true"></div>
    <div class="row">
      <span class="row-text">
        <span>이 사이트에서 광고 차단</span>
        <span class="sub">{status.site}</span>
      </span>
      <button class="switch" role="switch" aria-checked={siteOn} aria-label="이 사이트에서 광고 차단" onclick={toggle}><span></span></button>
    </div>
    {#if changed}
      <div class="note">
        <span>새로고침해야 적용됩니다</span>
        <button class="small primary" onclick={() => host.adblockReload().catch(onError)}>새로고침</button>
      </div>
    {/if}
  {/if}
  <div class="divider" aria-hidden="true"></div>
  <div class="foot">
    <span>{status.busy ? "목록을 받는 중…" : `목록 갱신 · ${whenText(status.updated)}`}</span>
    <button class="link" onclick={openSettings}>광고 차단 설정</button>
  </div>
  {#if status.error}<p class="error">{status.error}</p>{/if}
</section>

<style>
  .popup {
    display: flex;
    flex-direction: column;
    padding: 8px 4px 4px;
    background: var(--menu-bg);
    border-radius: var(--menu-radius);
    box-shadow: var(--menu-shadow);
    color: var(--fg);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    height: var(--ext-title-height);
    padding: 0 8px;
    font-weight: 600;
  }
  .count {
    display: flex;
    align-items: baseline;
    gap: 6px;
    padding: 4px 8px 10px;
  }
  .num {
    font-size: var(--adblock-count-size);
    font-weight: 600;
    line-height: 1.1;
  }
  .text {
    margin: 0;
    padding: 4px 8px 10px;
    color: var(--fg-muted);
  }
  .sub {
    font-size: var(--sub-font-size);
    color: var(--fg-muted);
  }
  .divider {
    flex: none;
    height: 1px;
    margin: 0 8px;
    background: var(--divider);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 52px;
    padding: 0 8px;
  }
  .row-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
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
  .note {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 4px 6px;
    padding: 7px 7px 7px 10px;
    background: var(--field-bg);
    border-radius: var(--button-radius);
    font-size: var(--small-button-font-size);
  }
  .note span {
    flex: 1;
  }
  .small {
    flex: none;
    height: var(--small-button-height);
    padding: var(--small-button-padding);
    border-radius: var(--small-button-radius);
    font-size: var(--small-button-font-size);
    font-weight: 500;
  }
  .small.primary {
    background: var(--prompt-primary-bg);
    color: var(--prompt-primary-fg);
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    height: 36px;
    padding: 0 8px;
    font-size: var(--sub-font-size);
    color: var(--fg-muted);
  }
  .link {
    font-size: var(--small-button-font-size);
    color: var(--fg);
    text-decoration: underline;
    white-space: nowrap;
  }
  .error {
    margin: 0 8px 6px;
    font-size: var(--sub-font-size);
    color: var(--fg-muted);
    white-space: pre-line;
  }
</style>
