<script lang="ts">
  // 다운로드 페이지 (spec 5.4): 항목마다 파일 열기, 폴더 열기, 취소
  import "./page.css";
  import { host4, fileName, formatBytes, readableUrl } from "../api";
  import { ui, report } from "../store.svelte";

  const STATE: Record<string, string> = { progress: "받는 중", done: "완료", cancelled: "취소됨", failed: "실패" };

  function when(t: number) {
    return new Date(t).toLocaleString("ko-KR", { month: "long", day: "numeric", hour: "2-digit", minute: "2-digit" });
  }
</script>

<div class="page">
  <div class="page-inner">
    <div class="page-head">
      <h1 class="page-title">다운로드</h1>
    </div>
    {#if !ui.downloads.length}
      <p class="page-empty">받은 파일이 없습니다.</p>
    {/if}
    <div class="page-list">
      {#each ui.downloads as d (d.id)}
        <div class="page-row item">
          <div class="info">
            <span class="page-ellipsis name" class:gone={d.state === "cancelled" || d.state === "failed"}>{fileName(d.path)}</span>
            <span class="page-ellipsis page-muted">
              {STATE[d.state] ?? d.state} ·
              {#if d.state === "progress"}{formatBytes(d.received)} / {d.total > 0 ? formatBytes(d.total) : "크기 모름"}{:else}{formatBytes(d.total > 0 ? d.total : d.received)}{/if}
              · {when(d.time)} · {readableUrl(d.url)}
            </span>
            {#if d.state === "progress"}
              <div class="bar" aria-hidden="true"><span style:width="{d.total > 0 ? Math.min(100, (d.received / d.total) * 100) : 0}%"></span></div>
            {/if}
          </div>
          {#if d.state === "done"}
            <button class="page-button" onclick={() => host4.downloadOpen(d.id).catch(report)}>파일 열기</button>
          {/if}
          <button class="page-button" onclick={() => host4.downloadShow(d.id).catch(report)}>폴더 열기</button>
          {#if d.state === "progress"}
            <button class="page-button" onclick={() => host4.downloadCancel(d.id).catch(report)}>취소</button>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .item {
    padding: 8px 10px;
  }
  .info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .name {
    font-weight: 500;
  }
  .gone {
    text-decoration: line-through;
    color: var(--fg-muted);
  }
  .bar {
    height: 4px;
    border-radius: 2px;
    background: var(--field-bg);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--fg);
  }
</style>
