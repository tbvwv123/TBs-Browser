<script lang="ts">
  // 권한 요청 (spec 5.5): 카메라, 마이크, 알림, 위치를 쓰려 하면 허용·차단을 묻는다. 고른 답은 사이트별로 기억한다.
  // 콘텐츠 웹뷰 위에 그릴 수 없어서 툴바와 페이지 카드 사이에 한 줄로 띄운다 (그동안 카드가 그만큼 줄어든다).
  import { host7, siteName, PERMISSION_NAMES, type PermissionRequest } from "./api";
  import { report } from "./store.svelte";

  let { request }: { request: PermissionRequest } = $props();

  // 요청이 온 순서와 상관없이 늘 같은 순서로 (카메라, 마이크, 알림, 위치)
  const ORDER = Object.keys(PERMISSION_NAMES);
  const what = $derived(
    [...request.kinds]
      .sort((a, b) => ORDER.indexOf(a) - ORDER.indexOf(b))
      .map((k) => PERMISSION_NAMES[k])
      .join(", "),
  );

  function answer(allow: boolean, remember: boolean) {
    host7.permissionAnswer(request.tab, request.site, allow, remember).catch(report);
  }
</script>

<div class="prompt" role="alertdialog" aria-label="권한 요청">
  <svg class="icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l7 3v5c0 4.5-3 8-7 10-4-2-7-5.5-7-10V6z"></path></svg>
  <p class="text"><strong>{siteName(request.site)}</strong>에서 <strong>{what}</strong> 권한을 요청합니다. 고른 답은 이 사이트에 기억합니다.</p>
  <button class="button" onclick={() => answer(false, true)}>차단</button>
  <button class="button primary" onclick={() => answer(true, true)}>허용</button>
  <button class="close" aria-label="닫기 (이번만 차단)" title="닫기 (이번만 차단)" onclick={() => answer(false, false)}>
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18"></path></svg>
  </button>
</div>

<style>
  .prompt {
    flex: none;
    display: flex;
    align-items: center;
    gap: var(--prompt-gap);
    height: var(--prompt-height);
    padding: var(--prompt-padding);
    background: var(--menu-bg);
    border-radius: var(--menu-radius);
    box-shadow: var(--menu-shadow);
  }
  .icon {
    flex: none;
    color: var(--fg-muted);
  }
  .text {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .text strong {
    font-weight: 600;
  }
  .button {
    flex: none;
    height: var(--prompt-button-height);
    padding: var(--prompt-button-padding);
    border-radius: var(--icon-button-radius);
    background: var(--field-bg);
  }
  .button:hover {
    background: var(--hover-bg);
  }
  .button.primary {
    background: var(--prompt-primary-bg);
    color: var(--prompt-primary-fg);
  }
  .close {
    flex: none;
    width: var(--prompt-button-height);
    height: var(--prompt-button-height);
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--icon-button-radius);
    color: var(--fg-muted);
  }
  .close:hover {
    background: var(--hover-bg);
  }
</style>
