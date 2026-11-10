// 탭과 북마크를 끌어 옮기기 (spec 5.1, 5.3). HTML 끌어놓기 대신 포인터 이벤트로 직접 한다.
// 놓을 수 있는 줄에는 data-drop 속성이 있다:
//   data-drop="tab" data-id
//   data-drop="group" data-id [data-edge: 위쪽 몇 px까지 "그룹 앞"인지. 없으면 높이의 30%]
//   data-drop="tabs-head"
//   data-drop="bookmark" data-id data-parent data-folder
//   data-drop="bookmarks-head"
import { host, childrenOf } from "./api";
import { ui, report, type DragSource, type DropTarget } from "./store.svelte";

const THRESHOLD = 4;
let suppressClick = false;

/** 방금 끌기가 끝났으면 그 뒤의 클릭은 무시한다 */
export function consumeDragClick(): boolean {
  const s = suppressClick;
  suppressClick = false;
  return s;
}

export function startDrag(e: PointerEvent, source: DragSource, label: string) {
  if (e.button !== 0 || ui.search) return; // 검색 중에는 옮기지 않는다
  const x0 = e.clientX;
  const y0 = e.clientY;
  const el = e.currentTarget as HTMLElement;
  const pointer = e.pointerId;
  let started = false;

  const move = (ev: PointerEvent) => {
    if (!started) {
      if (Math.hypot(ev.clientX - x0, ev.clientY - y0) < THRESHOLD) return;
      started = true;
      // 페이지(다른 창) 위로 나가도 포인터를 놓치지 않게 잡아 둔다
      try {
        el.setPointerCapture(pointer);
      } catch {}
      ui.drag = { source, label, x: ev.clientX, y: ev.clientY, target: null };
    }
    ui.drag!.x = ev.clientX;
    ui.drag!.y = ev.clientY;
    ui.drag!.target = targetAt(ev.clientX, ev.clientY, source);
  };
  const up = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("keydown", esc);
    if (!started) return;
    suppressClick = true;
    setTimeout(() => (suppressClick = false), 0);
    const d = ui.drag;
    ui.drag = null;
    if (d?.target) drop(d.source, d.target).catch(report);
  };
  const esc = (ev: KeyboardEvent) => {
    if (ev.key === "Escape" && ui.drag) ui.drag.target = null;
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("keydown", esc);
}

function targetAt(x: number, y: number, source: DragSource): DropTarget | null {
  const el = (document.elementFromPoint(x, y) as HTMLElement | null)?.closest<HTMLElement>("[data-drop]");
  if (!el) return null;
  const r = el.getBoundingClientRect();
  const f = (y - r.top) / Math.max(1, r.height);
  const kind = el.dataset.drop;
  if (kind === "tab") {
    const id = Number(el.dataset.id);
    if (source.kind === "bookmark") return null;
    return { kind: "tab", id, pos: f < 0.5 ? "before" : "after" };
  }
  if (kind === "group") {
    if (source.kind === "bookmark") return null;
    const edge = el.dataset.edge ? Number(el.dataset.edge) : r.height * 0.3;
    return { kind: "group", id: Number(el.dataset.id), pos: y - r.top < edge ? "before" : "inside" };
  }
  if (kind === "tabs-head") return source.kind === "tab" ? { kind: "tabs-head" } : null;
  if (kind === "bookmarks-head") return { kind: "bookmarks-head" };
  if (kind === "bookmark") {
    const id = el.dataset.id!;
    if (source.kind === "bookmark" && source.id === id) return null;
    const parent = el.dataset.parent || null;
    const folder = el.dataset.folder === "1";
    const pos = folder ? (f < 0.25 ? "before" : f > 0.75 ? "after" : "inside") : f < 0.5 ? "before" : "after";
    return { kind: "bookmark", id, parent, pos };
  }
  return null;
}

/** 탭 목록에서 `id` 다음 탭 (`skip`은 건너뛴다) */
function tabAfter(id: number, skip: number): number | null {
  const ids = ui.tabs.map((t) => t.id).filter((t) => t !== skip);
  const i = ids.indexOf(id);
  return i >= 0 && i + 1 < ids.length ? ids[i + 1] : null;
}

/** 북마크 `id` 다음 형제 (`skip`은 건너뛴다) */
function bookmarkAfter(parent: string | null, id: string, skip: string | null): string | null {
  const list = (childrenOf(ui.bookmarks.items, parent) ?? []).filter((n) => n.id !== skip);
  const i = list.findIndex((n) => n.id === id);
  return i >= 0 && i + 1 < list.length ? list[i + 1].id : null;
}

/** 북마크 자리를 (parent, before)로 바꾼다 */
function bookmarkPlace(t: DropTarget, skip: string | null): { parent: string | null; before: string | null } | null {
  if (t.kind === "bookmarks-head") return { parent: null, before: ui.bookmarks.items.find((n) => n.id !== skip)?.id ?? null };
  if (t.kind !== "bookmark") return null;
  if (t.pos === "inside") return { parent: t.id, before: null };
  if (t.pos === "before") return { parent: t.parent, before: t.id };
  return { parent: t.parent, before: bookmarkAfter(t.parent, t.id, skip) };
}

async function drop(source: DragSource, t: DropTarget) {
  if (source.kind === "bookmark") {
    const place = bookmarkPlace(t, source.id);
    if (place) await host.bookmarkMove(source.id, place.parent, place.before);
    return;
  }
  const tab = ui.tabs.find((x) => x.id === source.id);
  if (!tab) return;
  // 탭을 북마크 구역에 놓으면 북마크로 추가한다 (spec 5.3)
  const place = bookmarkPlace(t, null);
  if (place) {
    if (!tab.url) throw "아직 주소가 없는 탭은 북마크로 추가할 수 없습니다.";
    await host.bookmarkAdd(place.parent, place.before, tab.title, tab.url, tab.favicon);
    return;
  }
  if (t.kind === "tab") {
    const target = ui.tabs.find((x) => x.id === t.id);
    if (!target || target.id === tab.id) return;
    const before = t.pos === "before" ? target.id : tabAfter(target.id, tab.id);
    await host.tabMove(tab.id, before, target.group);
  } else if (t.kind === "group") {
    const first = ui.tabs.find((x) => x.group === t.id && x.id !== tab.id);
    await host.tabMove(tab.id, first?.id ?? null, t.pos === "inside" ? t.id : null);
  } else if (t.kind === "tabs-head") {
    const first = ui.tabs.find((x) => x.id !== tab.id);
    await host.tabMove(tab.id, first?.id ?? null, null);
  }
}

/** 이 줄 위에 그릴 놓을 자리 표시 ("before" | "after" | "inside" | null) */
export function dropMark(kind: string, id: string | number): string | null {
  const t = ui.drag?.target;
  if (!t || t.kind !== kind) return null;
  if ("id" in t && t.id !== id) return null;
  return "pos" in t ? t.pos : "before";
}
