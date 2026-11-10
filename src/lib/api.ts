// 호스트와 주고받는 명령과 이벤트. 상태의 원본은 호스트에 있고 UI는 받아서 그리기만 한다 (spec 3장).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type TabId = number;
export type GroupId = number;

export interface Tab {
  id: TabId;
  title: string;
  url: string;
  favicon: string | null;
  loading: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  group: GroupId | null;
  /** 소리를 내는 중 (spec 4.5) */
  audible: boolean;
  /** "항상 유지" (spec 7장) */
  keepAlive: boolean;
}

export interface Group {
  id: GroupId;
  title: string;
  collapsed: boolean;
}

export interface Snapshot {
  tabs: Tab[];
  groups: Group[];
  active: TabId | null;
}

export type BookmarkNode =
  | { type: "link"; id: string; title: string; url: string; icon?: string }
  | { type: "folder"; id: string; title: string; open: boolean; items: BookmarkNode[] };

export interface Bookmarks {
  version: number;
  collapsed: boolean;
  items: BookmarkNode[];
}

type Opt<T> = T | null;

export const host = {
  getState: () => invoke<Snapshot>("get_state"),
  tabCreate: () => invoke<TabId>("tab_create"),
  tabClose: (id: TabId) => invoke<void>("tab_close", { id }),
  tabActivate: (id: TabId) => invoke<void>("tab_activate", { id }),
  navigate: (id: TabId, input: string) => invoke<void>("navigate", { id, input }),
  goBack: (id: TabId) => invoke<void>("go_back", { id }),
  goForward: (id: TabId) => invoke<void>("go_forward", { id }),
  reload: (id: TabId) => invoke<void>("reload", { id }),
  stop: (id: TabId) => invoke<void>("stop", { id }),
  setContentBounds: (r: DOMRect, radius: number) =>
    invoke<void>("set_content_bounds", { x: r.left, y: r.top, w: r.width, h: r.height, radius }),
  setMaxButtonBounds: (r: DOMRect) => invoke<void>("set_max_button_bounds", { x: r.left, y: r.top, w: r.width, h: r.height }),

  tabOpenUrl: (input: string) => invoke<void>("tab_open_url", { input }),
  tabMove: (id: TabId, before: Opt<TabId>, group: Opt<GroupId>) => invoke<void>("tab_move", { id, before, group }),
  groupCreate: (tab: TabId, title: string) => invoke<GroupId | null>("group_create", { tab, title }),
  groupRename: (id: GroupId, title: string) => invoke<void>("group_rename", { id, title }),
  groupSetCollapsed: (id: GroupId, collapsed: boolean) => invoke<void>("group_set_collapsed", { id, collapsed }),
  groupUngroup: (id: GroupId) => invoke<void>("group_ungroup", { id }),
  tabUngroup: (id: TabId) => invoke<void>("tab_ungroup", { id }),
  tabSetKeepAlive: (id: TabId, keep: boolean) => invoke<void>("tab_set_keep_alive", { id, keep }),

  bookmarksGet: () => invoke<Bookmarks>("bookmarks_get"),
  takeNotices: () => invoke<string[]>("take_notices"),
  bookmarkAdd: (parent: Opt<string>, before: Opt<string>, title: string, url: string, icon: Opt<string>) =>
    invoke<string>("bookmark_add", { parent, before, title, url, icon }),
  bookmarkToggle: (title: string, url: string, icon: Opt<string>) => invoke<boolean>("bookmark_toggle", { title, url, icon }),
  bookmarkAddFolder: (parent: Opt<string>, before: Opt<string>, title: string) =>
    invoke<string>("bookmark_add_folder", { parent, before, title }),
  bookmarkRename: (id: string, title: string) => invoke<void>("bookmark_rename", { id, title }),
  bookmarkRemove: (id: string) => invoke<void>("bookmark_remove", { id }),
  bookmarkMove: (id: string, parent: Opt<string>, before: Opt<string>) => invoke<void>("bookmark_move", { id, parent, before }),
  bookmarkSetOpen: (id: string, open: boolean) => invoke<void>("bookmark_set_open", { id, open }),
  bookmarksSetCollapsed: (collapsed: boolean) => invoke<void>("bookmarks_set_collapsed", { collapsed }),
};

export function onTabsChanged(f: (s: Snapshot) => void): Promise<UnlistenFn> {
  return listen<Snapshot>("tabs_changed", (e) => f(e.payload));
}

export function onTabUpdated(f: (t: Tab) => void): Promise<UnlistenFn> {
  return listen<Tab>("tab_updated", (e) => f(e.payload));
}

export function onBookmarksChanged(f: (b: Bookmarks) => void): Promise<UnlistenFn> {
  return listen<Bookmarks>("bookmarks_changed", (e) => f(e.payload));
}

export function onMaxButtonHover(f: (hover: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>("max_button_hover", (e) => f(e.payload));
}

export function onWindowMaximized(f: (maximized: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>("window_maximized", (e) => f(e.payload));
}

/** 사람이 읽는 주소: %EC%84… 같은 인코딩을 한글로 풀어 보여준다. 풀 수 없으면 그대로 둔다. */
export function readableUrl(url: string): string {
  try {
    return decodeURI(url);
  } catch {
    return url;
  }
}

/** 주소창 평소 표시: https://를 뺀다 (spec 5.2) */
export function displayUrl(url: string): string {
  const u = readableUrl(url);
  return u.startsWith("https://") ? u.slice("https://".length) : u;
}

/** 주소가 북마크에 있는지 (별 버튼) */
export function isBookmarked(items: BookmarkNode[], url: string): boolean {
  return items.some((n) => (n.type === "link" ? n.url === url : isBookmarked(n.items, url)));
}

/** 폴더 안 북마크 수 (하위 폴더 포함) */
export function countLinks(items: BookmarkNode[]): number {
  return items.reduce((n, x) => n + (x.type === "link" ? 1 : countLinks(x.items)), 0);
}

/** `parent` 폴더(null이면 맨 위)의 자식 목록 */
export function childrenOf(items: BookmarkNode[], parent: string | null): BookmarkNode[] | null {
  if (parent === null) return items;
  for (const n of items) {
    if (n.type !== "folder") continue;
    if (n.id === parent) return n.items;
    const found = childrenOf(n.items, parent);
    if (found) return found;
  }
  return null;
}

// ───── 4단계: 설정, 방문 기록, 다운로드, 단축키, 팝업 ─────

export type Theme = "light" | "dark" | "system";
export type SearchEngine = "google" | "naver" | "bing" | "duckduckgo";

export interface Settings {
  version: number;
  theme: Theme;
  searchEngine: SearchEngine;
  downloadDir: string;
  restoreTabs: boolean;
  memory: { suspendAfterMinutes: number; discardAfterMinutes: number; maxAwakeTabs: number };
  /** 사이트별 권한 (spec 5.5): 사이트 → 권한 이름 → "allow" | "block" */
  permissions: Record<string, Record<string, "allow" | "block">>;
}

export interface Visit {
  id: number;
  url: string;
  title: string;
  time: number;
}

export interface Suggestion {
  url: string;
  title: string;
  kind: "bookmark" | "history";
}

export interface Download {
  id: number;
  url: string;
  path: string;
  state: "progress" | "done" | "cancelled" | "failed";
  received: number;
  total: number;
  time: number;
}

export const host4 = {
  settingsGet: () => invoke<Settings>("settings_get"),
  settingsSet: (value: Settings) => invoke<Settings>("settings_set", { value }),
  pickDownloadDir: () => invoke<string | null>("pick_download_dir"),
  historySearch: (query: string, before: number | null) => invoke<Visit[]>("history_search", { query, before }),
  historyDelete: (id: number) => invoke<void>("history_delete", { id }),
  historyClear: () => invoke<void>("history_clear"),
  suggest: (query: string) => invoke<Suggestion[]>("suggest", { query }),
  downloadsGet: () => invoke<Download[]>("downloads_get"),
  downloadOpen: (id: number) => invoke<void>("download_open", { id }),
  downloadShow: (id: number) => invoke<void>("download_show", { id }),
  downloadCancel: (id: number) => invoke<void>("download_cancel", { id }),
  runShortcut: (name: string) => invoke<void>("run_shortcut", { name }),
  popupShow: (kind: PopupKind, r: { x: number; y: number; w: number; h: number }, data: unknown) =>
    invoke<void>("popup_show", { kind, ...r, data: { theme: document.documentElement.dataset.theme ?? "light", ...(data as object) } }),
  popupHide: () => invoke<void>("popup_hide"),
  popupPick: (url: string, newTab = false) => invoke<void>("popup_pick", { url, newTab }),
  popupMenuPick: (index: number) => invoke<void>("popup_menu_pick", { index }),
  fullscreenState: () => invoke<boolean>("fullscreen_state"),
};

export type PopupKind = "suggest" | "downloads" | "menu" | "bookmarks";

// ───── 7단계: 접힌 사이드바, 새 탭 화면, 테마, 권한 ─────

export interface TopSite {
  url: string;
  title: string;
  favicon: string | null;
}

/** 한 탭의 한 사이트가 기다리는 권한 질문 (spec 5.5) */
export interface PermissionRequest {
  tab: TabId;
  site: string;
  kinds: PermissionKind[];
}

export type PermissionKind = "camera" | "microphone" | "notifications" | "geolocation";

export const PERMISSION_NAMES: Record<PermissionKind, string> = {
  camera: "카메라",
  microphone: "마이크",
  notifications: "알림",
  geolocation: "위치",
};

export const host8 = {
  updateGet: () => invoke<string | null>("update_get"),
  updateInstall: () => invoke<void>("update_install"),
  logUi: (message: string) => invoke<void>("log_ui", { message }),
};

export const host7 = {
  sidebarGet: () => invoke<boolean>("sidebar_get"),
  sidebarSetCollapsed: (collapsed: boolean) => invoke<void>("sidebar_set_collapsed", { collapsed }),
  topSites: () => invoke<TopSite[]>("top_sites"),
  permissionRequests: () => invoke<PermissionRequest[]>("permission_requests"),
  permissionAnswer: (tab: TabId, site: string, allow: boolean, remember: boolean) =>
    invoke<void>("permission_answer", { tab, site, allow, remember }),
  applyTheme: (dark: boolean, system: boolean, background: [number, number, number]) =>
    invoke<void>("apply_theme", { dark, system, background }),
};

/** "https://www.example.com" → "www.example.com" (사람에게 보여 줄 사이트 이름) */
export function siteName(site: string): string {
  return site.replace(/^https?:\/\//, "");
}

export function on<T>(event: string, f: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => f(e.payload));
}

/** 내부 페이지의 이름 (탭 제목) */
export function internalTitle(url: string): string | null {
  switch (url) {
    case "tbs://history":
      return "방문 기록";
    case "tbs://downloads":
      return "다운로드";
    case "tbs://settings":
      return "설정";
  }
  return null;
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

export function formatBytes(n: number): string {
  if (n < 0) return "?";
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v < 10 ? v.toFixed(1) : Math.round(v)} ${units[i]}`;
}

/** 브라우저 단축키 이름 (spec 5.6). 호스트의 shortcuts::name과 같은 규칙 */
export function shortcutName(e: KeyboardEvent): string | null {
  const k = e.key.length === 1 ? e.key.toUpperCase() : e.key;
  const c = e.ctrlKey, s = e.shiftKey, a = e.altKey;
  if (c && !s && !a && k === "T") return "new_tab";
  if (c && !s && !a && k === "W") return "close_tab";
  if (c && s && !a && k === "T") return "reopen_tab";
  if (c && !a && k === "Tab") return s ? "prev_tab" : "next_tab";
  if (c && !s && !a && k === "L") return "focus_address";
  if ((c && !s && !a && k === "R") || (!c && !s && !a && k === "F5")) return "reload";
  if (!c && !s && a && k === "ArrowLeft") return "back";
  if (!c && !s && a && k === "ArrowRight") return "forward";
  if (c && !s && !a && k === "D") return "bookmark";
  if (c && !s && !a && k === "H") return "history";
  if (c && !s && !a && k === "J") return "downloads";
  if (c && !s && !a && k === "B") return "toggle_sidebar";
  if (c && s && !a && k === "A") return "focus_tab_search";
  if (c && !s && !a && k === "F") return "find";
  if (!c && !s && !a && k === "F11") return "fullscreen";
  if (!c && !s && !a && k === "F12") return "devtools";
  return null;
}

// ───── 5단계: 사이트 스크립트 (spec 6장) ─────

export interface ScriptInfo {
  id: string;
  name: string;
  matches: string[];
  enabled: boolean;
  pinned: boolean;
  permissions: string[];
  error: string | null;
}

export const host5 = {
  scriptsList: () => invoke<ScriptInfo[]>("scripts_list"),
  scriptSetEnabled: (id: string, enabled: boolean) => invoke<void>("script_set_enabled", { id, enabled }),
  scriptSetPinned: (id: string, pinned: boolean) => invoke<void>("script_set_pinned", { id, pinned }),
  scriptsReload: () => invoke<ScriptInfo[]>("scripts_reload"),
  scriptsOpenFolder: () => invoke<void>("scripts_open_folder"),
};

/** `*`는 아무 글자나 (spec 6.1). 호스트의 scripts::pattern_matches와 같은 규칙 */
export function patternMatches(pattern: string, text: string): boolean {
  const re = new RegExp("^" + pattern.split("*").map((x) => x.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*") + "$");
  return re.test(text);
}

/** 이 주소에 적용되는 스크립트인지 */
export function scriptApplies(s: ScriptInfo, url: string): boolean {
  return !!url && s.matches.some((p) => patternMatches(p, url));
}
