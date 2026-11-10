<script lang="ts">
  // 설치·권한 확인 (spec 6.6, 시안 M). 주소창 아래 가운데의 페이지 위 팝업 창에 뜬다.
  // 바깥을 눌러도 닫히지 않고 "취소"나 Esc로만 닫는다.
  import type { ExtInfo } from "./api";

  let { info, onAnswer }: { info: ExtInfo; onAnswer: (accept: boolean) => void } = $props();

  const update = $derived(info.updateFrom !== null);
  const script = $derived(info.kind === "script");
  const head = $derived(script ? "스크립트 켜기" : update ? "업데이트 확인" : "크롬 확장 설치");
  const meta = $derived(
    [info.author && `만든 사람 ${info.author}`, update ? `버전 ${info.updateFrom} → ${info.version}` : info.version && `버전 ${info.version}`]
      .filter(Boolean)
      .join(" · "),
  );
</script>

<section class="dialog" aria-label={head}>
  <h2 class="head">{head}</h2>
  <div class="who">
    {#if info.icon}
      <img class="icon" src={info.icon} alt="" />
    {:else}
      <span class="icon letter" aria-hidden="true">{info.name.slice(0, 1)}</span>
    {/if}
    <div class="who-text">
      <div class="name-line">
        <span class="name">{info.name}</span>
        <span class="tag">{script ? "스크립트" : "크롬 확장"}</span>
      </div>
      {#if meta}<span class="meta">{meta}</span>{/if}
    </div>
  </div>
  {#if info.description}<p class="desc">{info.description}</p>{/if}

  <div class="block">
    <h3 class="label">적용 사이트</h3>
    {#if info.allSites}
      <!-- 무채색 안에서 가장 강한 대비: 색을 뒤집은 면 (시안 M) -->
      <div class="warn">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4l9 16H3z"></path><path d="M12 10v4M12 17v.5"></path></svg>
        <span class="warn-text">
          <strong>모든 사이트에서 실행됩니다</strong>
          <span>방문하는 모든 페이지의 내용을 읽고 바꿀 수 있습니다.</span>
        </span>
      </div>
    {:else if info.sites.length}
      <div class="chips">
        {#each info.sites as s}<span class="chip">{s}</span>{/each}
      </div>
    {:else}
      <span class="meta">페이지에 끼어드는 사이트가 없습니다.</span>
    {/if}
  </div>

  {#if info.permissions.length}
    <div class="block tight">
      <h3 class="label">권한</h3>
      <ul class="perms">
        {#each info.permissions as p}
          <li>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12.5l4.5 4.5L19 7.5"></path></svg>
            <span>{p}</span>
          </li>
        {/each}
      </ul>
    </div>
  {/if}

  <div class="note">
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="8.5"></circle><path d="M12 11v5M12 8v.5"></path></svg>
    <span>{info.error ?? (script ? "열린 탭은 새로고침하면 적용됩니다." : "크롬 확장의 일부 기능은 동작하지 않을 수 있습니다.")}</span>
  </div>

  <div class="buttons">
    <button class="button" onclick={() => onAnswer(false)}>취소</button>
    <button class="button primary" onclick={() => onAnswer(true)}>{script ? "켜기" : update ? "업데이트" : "설치"}</button>
  </div>
</section>

<style>
  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--install-dialog-gap);
    padding: var(--install-dialog-padding);
    background: var(--menu-bg);
    border-radius: var(--menu-radius);
    box-shadow: var(--menu-shadow);
    color: var(--fg);
  }
  .head {
    margin: 0;
    font-size: var(--section-title-font-size);
    font-weight: 600;
    color: var(--fg-muted);
  }
  .who {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .icon {
    flex: none;
    width: var(--ext-icon-large);
    height: var(--ext-icon-large);
    object-fit: contain;
  }
  .letter {
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: calc(var(--ext-icon-large) / 4);
    background: var(--letter-icon-bg);
    color: var(--prompt-primary-fg);
    font-size: calc(var(--ext-icon-large) * 0.62);
    font-weight: 700;
    line-height: 1;
  }
  .who-text {
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
    min-width: 0;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--page-section-title-size);
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
  }
  .meta {
    font-size: var(--sub-font-size);
    color: var(--fg-muted);
  }
  .desc {
    margin: 0;
    font-size: var(--button-font-size);
  }
  .block {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .block.tight {
    gap: 4px;
  }
  .label {
    margin: 0;
    font-size: var(--sub-font-size);
    font-weight: 600;
    color: var(--fg-muted);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    height: var(--chip-height);
    padding: var(--chip-padding);
    border-radius: var(--chip-radius);
    background: var(--tag-bg);
    font-size: var(--chip-font-size);
    line-height: var(--chip-height);
  }
  .warn {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 9px 10px;
    border-radius: var(--button-radius);
    background: var(--prompt-primary-bg);
    color: var(--prompt-primary-fg);
  }
  .warn svg {
    flex: none;
  }
  .warn-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    font-size: var(--sub-font-size);
  }
  .warn-text strong {
    font-size: var(--button-font-size);
    font-weight: 600;
  }
  .perms {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    font-size: var(--button-font-size);
  }
  .perms li {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 24px;
  }
  .perms svg {
    flex: none;
  }
  .note {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 8px 10px;
    border-radius: var(--button-radius);
    background: var(--field-bg);
    font-size: var(--small-button-font-size);
  }
  .note svg {
    flex: none;
    color: var(--fg-muted);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 4px;
  }
  .button {
    height: var(--button-height);
    padding: var(--button-padding);
    border-radius: var(--button-radius);
    background: var(--field-bg);
    font-size: var(--button-font-size);
    font-weight: 500;
  }
  .button.primary {
    background: var(--prompt-primary-bg);
    color: var(--prompt-primary-fg);
  }
</style>
