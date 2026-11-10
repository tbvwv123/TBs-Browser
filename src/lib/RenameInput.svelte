<script lang="ts">
  // 줄 안에서 이름을 바꾸는 입력 칸. Enter나 칸 밖을 누르면 저장, Esc는 취소.
  import { onMount } from "svelte";

  let { value, onCommit, onCancel }: { value: string; onCommit: (v: string) => void; onCancel: () => void } = $props();
  let input: HTMLInputElement;
  let done = false;

  function commit() {
    if (done) return;
    done = true;
    const v = input.value.trim();
    if (v && v !== value) onCommit(v);
    else onCancel();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter") commit();
    if (e.key === "Escape") {
      done = true;
      onCancel();
    }
    e.stopPropagation();
  }

  onMount(() => {
    input.focus();
    input.select();
  });
</script>

<input
  class="rename"
  bind:this={input}
  {value}
  aria-label="새 이름"
  spellcheck="false"
  onkeydown={onKey}
  onblur={commit}
  onpointerdown={(e) => e.stopPropagation()}
  onclick={(e) => e.stopPropagation()}
/>

<style>
  .rename {
    flex: 1;
    min-width: 0;
    height: 22px;
    padding: 0 6px;
    border: 1px solid var(--divider);
    border-radius: 6px;
    background: var(--card-bg);
    outline: none;
    user-select: text;
  }
</style>
