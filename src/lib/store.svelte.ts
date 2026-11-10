// UI가 그리는 상태. 원본은 호스트에 있고, 여기 값은 호스트가 보낸 것을 받아 둔 것이다.
import { host, host4, host7, host8, host9, host12, displayUrl, internalTitle, type ExtInfo, type Bookmarks, type Download, type Group, type PermissionRequest, type ScriptInfo, type Settings, type Tab, type TabId } from "./api";

export interface MenuItem {
  label: string;
  run: () => void;
  danger?: boolean;
  /** 메뉴 항목 앞에 그릴 선 아이콘 (SVG path의 d 값들) */
  icon?: string[];
}

/** 시안 F의 메뉴 아이콘 (선 두께 1.8) */
export const ICONS = {
  downloads: ["M12 4v11", "M7 11l5 5 5-5", "M5 20h14"],
  history: ["M20 12a8 8 0 1 1-16 0 8 8 0 0 1 16 0", "M12 8v4l3 2"],
  settings: ["M4 7h10", "M18 7h2", "M4 17h2", "M10 17h10", "M18 7a2 2 0 1 1-4 0 2 2 0 0 1 4 0", "M10 17a2 2 0 1 1-4 0 2 2 0 0 1 4 0"],
  update: ["M20 11a8 8 0 1 0-2.3 5.7", "M20 4v7h-7"],
};

export type Renaming = { kind: "bookmark"; id: string } | { kind: "group"; id: number } | null;

/** 끌어 옮기는 중인 것 */
export type DragSource = { kind: "tab"; id: TabId } | { kind: "bookmark"; id: string };

/** 놓을 자리 */
export type DropTarget =
  | { kind: "tab"; id: TabId; pos: "before" | "after" }
  | { kind: "group"; id: number; pos: "before" | "inside" }
  | { kind: "tabs-head" }
  | { kind: "bookmark"; id: string; parent: string | null; pos: "before" | "after" | "inside" }
  | { kind: "bookmarks-head" };

export const ui = $state({
  tabs: [] as Tab[],
  groups: [] as Group[],
  activeId: null as TabId | null,
  bookmarks: { version: 1, collapsed: false, items: [] } as Bookmarks,
  error: "",
  /** 탭 검색 칸에 입력한 글자 (spec 5.1) */
  search: "",
  menu: null as { x: number; y: number; items: MenuItem[]; above?: boolean } | null,
  renaming: null as Renaming,
  drag: null as { source: DragSource; label: string; x: number; y: number; target: DropTarget | null } | null,
  downloads: [] as Download[],
  settings: null as Settings | null,
  /** F11 전체 화면: 사이드바와 툴바를 숨긴다 */
  fullscreen: false,
  /** 사이트 스크립트 (spec 6장) */
  scripts: [] as ScriptInfo[],
  /** 확장 프로그램 버튼으로 연 스크립트 목록 */
  scriptsOpen: false,
  /** 사이드바를 접었는지 (spec 4.6). 원본은 호스트의 세션 */
  sidebarCollapsed: false,
  /** 기다리는 권한 질문 (spec 5.5) */
  permissions: [] as PermissionRequest[],
  /** 설치할 수 있는 새 버전 (spec 5.7). 있으면 점 3개 버튼에 점을 찍는다 */
  update: null as string | null,
  /** 설치한 크롬 확장 (spec 6.6) */
  extensions: [] as ExtInfo[],
  /** 설치 확인을 기다리는 확장 (시안 M) */
  install: null as ExtInfo | null,
});

/** 사이드바 맨 아래 줄에 고정한 것: 스크립트와 크롬 확장 (spec 6.4) */
export interface Pinned {
  kind: "script" | "ext";
  id: string;
  name: string;
  icon: string | null;
  enabled: boolean;
  /** 크롬 확장의 팝업이 있는지 */
  popup: boolean;
}

export function pinnedItems(): Pinned[] {
  return [
    ...ui.scripts.filter((s) => s.pinned).map((s): Pinned => ({ kind: "script", id: s.id, name: s.name, icon: s.icon, enabled: s.enabled, popup: false })),
    ...ui.extensions.filter((e) => e.pinned).map((e): Pinned => ({ kind: "ext", id: e.id, name: e.name, icon: e.icon, enabled: e.enabled, popup: !!e.popup })),
  ];
}

/** 설치·권한 확인을 주소창 아래 가운데에 띄운다 (시안 M). 높이는 팝업이 내용에 맞춘다 */
export function showInstall(info: ExtInfo) {
  ui.install = info;
  const w = token("--install-dialog-width");
  const bar = document.querySelector(".address")?.getBoundingClientRect();
  const cx = bar ? bar.left + bar.width / 2 : window.innerWidth / 2;
  const y = bar ? bar.bottom + 6 : 48;
  host4.popupShow("install", { x: Math.max(8, cx - w / 2), y, w, h: 420 }, { info }).catch(report);
}

/** 설치 확인을 취소한다 (Esc, 팝업 창이 닫힘) */
export function cancelInstall() {
  if (!ui.install) return;
  const script = ui.install.kind === "script";
  const fromStore = ui.install.mode === "install" || ui.install.mode === "update";
  const id = ui.install.id;
  ui.install = null;
  if (fromStore) host12.storeConfirm(id, false).catch(() => {});
  if (script) host4.popupHide().catch(() => {});
  else host9.extInstallConfirm(false).catch(() => {});
}

export function report(e: unknown) {
  ui.error = String(e);
  // 오류 로그 파일에도 남긴다 (spec 9장 8단계)
  host8.logUi(ui.error).catch(() => {});
}

/** 점 3개 메뉴 (spec 4.5): 새 버전이 있으면 맨 위에 "업데이트 설치" (spec 5.7) */
export function moreMenuItems(): MenuItem[] {
  const items: MenuItem[] = [
    { label: "다운로드", icon: ICONS.downloads, run: () => host4.runShortcut("downloads").catch(report) },
    { label: "방문 기록", icon: ICONS.history, run: () => host4.runShortcut("history").catch(report) },
    { label: "설정", icon: ICONS.settings, run: () => host4.runShortcut("settings").catch(report) },
  ];
  if (ui.update) {
    items.unshift({
      label: `업데이트 설치 (${ui.update})`,
      icon: ICONS.update,
      run: () => {
        ui.error = `새 버전 ${ui.update}을(를) 내려받는 중입니다. 다 받으면 설치하고 다시 시작합니다. 열린 탭은 그대로 돌아옵니다.`;
        host8.updateInstall().catch(report);
      },
    });
  }
  return items;
}

export function activeTab(): Tab | null {
  return ui.tabs.find((t) => t.id === ui.activeId) ?? null;
}

/** 오른쪽 클릭 메뉴. 펼친 사이드바에서는 사이드바 안에 그리고 (콘텐츠 웹뷰 위로 넘어가지 않게),
 *  접힌 사이드바에서는 44px 안에 들어가지 않으므로 콘텐츠 위에 뜨는 팝업 창으로 띄운다. */
export function openMenu(e: MouseEvent, items: MenuItem[]) {
  e.preventDefault();
  if (ui.sidebarCollapsed) popupMenu(e.clientX, e.clientY, items, false);
  else ui.menu = { x: e.clientX, y: e.clientY, items };
}

/** 버튼 위로 펼쳐지는 메뉴 (사이드바 맨 아래 줄의 점 3개, spec 4.5) */
export function openMenuAbove(el: HTMLElement, items: MenuItem[]) {
  const r = el.getBoundingClientRect();
  if (ui.sidebarCollapsed) popupMenu(r.left, r.top - 6, items, true);
  else ui.menu = { x: r.right, y: r.top - 6, items, above: true };
}

/** 팝업 창으로 띄운 메뉴의 항목들. 팝업이 고른 번호를 돌려주면 여기서 실행한다 */
let popupItems: MenuItem[] = [];

export function token(name: string): number {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name)) || 0;
}

function popupMenu(x: number, y: number, items: MenuItem[], above: boolean) {
  popupItems = items;
  const w = token("--menu-width");
  const h = 8 + items.length * token("--menu-item-height") + (items.length - 1) * 2;
  const top = above ? y - h : Math.min(y, window.innerHeight - h - 8);
  const data = { items: items.map((i) => ({ label: i.label, danger: !!i.danger, icon: i.icon ?? null })) };
  host4.popupShow("menu", { x, y: Math.max(8, top), w, h }, data).catch(report);
}

export function runPopupMenuItem(index: number) {
  const item = popupItems[index];
  popupItems = [];
  item?.run();
}

/** 탭 줄에 보일 이름 */
export function tabLabel(tab: Tab): string {
  if (tab.url.startsWith("tbs://scripts/")) {
    const id = decodeURIComponent(tab.url.slice("tbs://scripts/".length));
    const s = ui.scripts.find((x) => x.id === id);
    if (s) return `${s.name} 설정`;
  }
  return internalTitle(tab.url) ?? (tab.title || (tab.url ? displayUrl(tab.url) : "새 탭"));
}

/** 탭 오른쪽 클릭 메뉴 (펼친 사이드바, 접힌 사이드바 공통) */
export function tabMenu(e: MouseEvent, tab: Tab) {
  openMenu(e, [
    tab.group === null
      ? {
          label: "새 그룹 만들기",
          run: async () => {
            // 그룹 이름은 펼친 사이드바에서 바로 고친다
            if (ui.sidebarCollapsed) await setSidebarCollapsed(false);
            const g = await host.groupCreate(tab.id, "새 그룹").catch(report);
            if (typeof g === "number") ui.renaming = { kind: "group", id: g };
          },
        }
      : { label: "그룹에서 빼기", run: () => host.tabUngroup(tab.id).catch(report) },
    {
      label: tab.keepAlive ? "항상 유지 끄기" : "항상 유지 (재우거나 닫지 않음)",
      run: () => host.tabSetKeepAlive(tab.id, !tab.keepAlive).catch(report),
    },
    { label: "탭 닫기", run: () => host.tabClose(tab.id).catch(report) },
  ]);
}

/** 사이드바 접기·펼치기 (spec 4.6, Ctrl+B). 호스트가 세션에 남기고 sidebar_collapsed로 알려 온다 */
export async function setSidebarCollapsed(collapsed: boolean) {
  ui.menu = null;
  ui.scriptsOpen = false;
  ui.sidebarCollapsed = collapsed;
  await host7.sidebarSetCollapsed(collapsed).catch(report);
}
