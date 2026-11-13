"""스모크 시험: 개발용 빌드를 가상 데스크톱 2에 포커스 없이 띄우고 개발용 훅(src-tauri/src/dev.rs)으로 확인한다.

python tools/smoke.py               빌드하고 시험
python tools/smoke.py --no-build    빌드하지 않고 시험
python tools/smoke.py --keep        데이터 폴더와 로그를 남긴다
python tools/smoke.py --only 탭     이름에 그 글자가 든 확인만 (시작·마무리 확인은 늘 한다)
"""

import argparse
import ctypes
import functools
import http.server
import json
import os
import re
import shutil
import subprocess
import sys
import threading
import time
import uuid
import winreg
from ctypes import wintypes
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET_DIR = ROOT / "target-dev"
EXE = TARGET_DIR / "debug" / "tbs-browser.exe"
FIXTURES = ROOT / "tools" / "smoke"
CREATE_NO_WINDOW = 0x08000000


class Fail(Exception):
    pass


# ───────────── Windows ─────────────

def desktop2_guid() -> str:
    """두 번째 가상 데스크톱의 GUID"""
    key = r"Software\Microsoft\Windows\CurrentVersion\Explorer\VirtualDesktops"
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key) as k:
            ids, _ = winreg.QueryValueEx(k, "VirtualDesktopIDs")
    except OSError:
        ids = b""
    if len(ids) < 32:
        raise Fail("가상 데스크톱 2가 없습니다. 작업 보기(Win+Tab)에서 데스크톱을 하나 더 만든 뒤 다시 실행해 주세요.")
    return str(uuid.UUID(bytes_le=bytes(ids[16:32])))


class PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [
        ("dwSize", wintypes.DWORD),
        ("cntUsage", wintypes.DWORD),
        ("th32ProcessID", wintypes.DWORD),
        ("th32DefaultHeapID", ctypes.c_size_t),
        ("th32ModuleID", wintypes.DWORD),
        ("cntThreads", wintypes.DWORD),
        ("th32ParentProcessID", wintypes.DWORD),
        ("pcPriClassBase", ctypes.c_long),
        ("dwFlags", wintypes.DWORD),
        ("szExeFile", ctypes.c_wchar * 260),
    ]


kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
kernel32.OpenProcess.restype = wintypes.HANDLE


def processes() -> dict[int, tuple[int, str]]:
    """pid → (부모 pid, 실행 파일 이름)"""
    snap = kernel32.CreateToolhelp32Snapshot(0x2, 0)
    out = {}
    e = PROCESSENTRY32W()
    e.dwSize = ctypes.sizeof(e)
    ok = kernel32.Process32FirstW(snap, ctypes.byref(e))
    while ok:
        out[e.th32ProcessID] = (e.th32ParentProcessID, e.szExeFile)
        ok = kernel32.Process32NextW(snap, ctypes.byref(e))
    kernel32.CloseHandle(snap)
    return out


def descendants(root: int) -> dict[int, str]:
    procs = processes()
    found, todo = {}, [root]
    while todo:
        p = todo.pop()
        for pid, (ppid, name) in procs.items():
            if ppid == p and pid not in found and pid != root:
                found[pid] = name
                todo.append(pid)
    return found


def terminate(pid: int):
    h = kernel32.OpenProcess(0x0001, False, pid)
    if h:
        kernel32.TerminateProcess(h, 1)
        kernel32.CloseHandle(h)


def visible_windows(pid: int) -> int:
    """그 프로세스의 보이는 최상위 창 수"""
    count = 0

    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def each(hwnd, _):
        nonlocal count
        owner = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            count += 1
        return True

    user32.EnumWindows(each, 0)
    return count


# ───────────── 시험용 웹 서버 ─────────────

class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


class QuietServer(http.server.ThreadingHTTPServer):
    # 앱을 끝낼 때 끊긴 연결
    def handle_error(self, request, client_address):
        pass


def start_server() -> http.server.ThreadingHTTPServer:
    handler = functools.partial(QuietHandler, directory=str(FIXTURES / "pages"))
    server = QuietServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


# ───────────── 앱 ─────────────

class App:
    def __init__(self, port: int):
        self.port = port
        self.dir = ROOT / ".dev-data" / f"smoke-{os.getpid()}"
        self.data = self.dir / "data"
        self.hook_dir = self.dir / "hook"
        self.downloads = self.dir / "downloads"
        self.log = self.dir / "app.log"
        self.proc: subprocess.Popen | None = None
        self.log_file = None
        self.children: dict[int, str] = {}
        self.rid = 0

    def url(self, path: str) -> str:
        return f"http://127.0.0.1:{self.port}/{path}"

    # 시작·끝

    def prepare(self):
        """새 데이터 폴더: 광고 차단은 끄고 목록·자원은 받지 않게 미리 채운다"""
        if self.dir.exists():
            shutil.rmtree(self.dir)
        for d in (self.data / "state", self.data / "adblock", self.data / "scripts", self.hook_dir, self.downloads):
            d.mkdir(parents=True)
        src = (ROOT / "src-tauri" / "src" / "adblock.rs").read_text(encoding="utf-8")
        lists = re.findall(r'ListDef \{\s*id: "([^"]+)"', src)
        ubo = re.search(r'const UBO_VERSION: &str = "([^"]+)"', src)
        now = int(time.time() * 1000)
        settings = {
            "version": 1,
            "restoreTabs": False,
            "downloadDir": str(self.downloads),
            "adblock": {"enabled": False, "lists": {i: False for i in lists}, "disabledSites": []},
        }
        write_json(self.data / "state" / "settings.json", settings)
        write_json(self.data / "adblock" / "meta.json", {"version": 1, "lists": {"resources": {"updated": now, "rules": 0}}})
        write_json(self.data / "adblock" / "resources.json", [])
        if ubo:
            noop = {"name": "tbs-smoke-noop.js", "aliases": [], "kind": {"mime": "application/javascript"}, "content": "KCk9Pnt9", "dependencies": []}
            write_json(self.data / "adblock" / "scriptlets.json", {"version": ubo.group(1), "made": now, "resources": [noop]})
        shutil.copytree(FIXTURES / "scripts" / "smoke-fixture", self.data / "scripts" / "smoke-fixture")

    def start(self):
        env = dict(os.environ)
        env.pop("TBS_DEV_FORCE_UPDATE", None)
        env.update(TBS_DEV_HOOK=str(self.hook_dir), TBS_DEV_DESKTOP=desktop2_guid(), TBS_DATA_DIR=str(self.data))
        self.log_file = open(self.log, "wb")
        self.proc = subprocess.Popen(
            [str(EXE)], cwd=str(ROOT), env=env, stdin=subprocess.DEVNULL,
            stdout=self.log_file, stderr=subprocess.STDOUT, creationflags=CREATE_NO_WINDOW,
        )
        end = time.monotonic() + 90
        while time.monotonic() < end:
            try:
                if self.eval("ui", "!!(window.__TAURI_INTERNALS__ && document.querySelector('[data-tab-id]'))", timeout=5):
                    self.track()
                    return
            except Fail:
                if self.proc.poll() is not None:
                    raise
            time.sleep(0.3)
        raise Fail("앱 화면이 90초 안에 뜨지 않았습니다")

    def track(self):
        """앱이 띄운 자식 프로세스를 기억해 둔다 (끝낼 때 남은 것이 없는지 본다)"""
        if self.proc and self.proc.poll() is None:
            self.children.update(descendants(self.proc.pid))

    def stop(self) -> list[str]:
        """앱과 그 자식만 끝낸다. 남은 프로세스를 돌려준다"""
        left = []
        if self.proc:
            self.track()
            if self.proc.poll() is None:
                subprocess.run(["taskkill", "/PID", str(self.proc.pid), "/T", "/F"], capture_output=True, creationflags=CREATE_NO_WINDOW)
                try:
                    self.proc.wait(10)
                except subprocess.TimeoutExpired:
                    terminate(self.proc.pid)
            end = time.monotonic() + 10
            while True:
                procs = processes()
                alive = {p: n for p, n in self.children.items() if procs.get(p, (0, ""))[1] == n}
                if not alive or time.monotonic() > end:
                    break
                for p in alive:
                    terminate(p)
                time.sleep(0.3)
            left = [f"{n}({p})" for p, n in alive.items()]
        if self.log_file:
            self.log_file.close()
        return left

    def cleanup(self):
        end = time.monotonic() + 20
        while self.dir.exists():
            try:
                shutil.rmtree(self.dir)
            except OSError:
                if time.monotonic() > end:
                    raise
                time.sleep(0.5)

    # 훅

    def hook(self, cmd: str, timeout: float = 30) -> str:
        done = self.hook_dir / "cmd-done.txt"
        done.unlink(missing_ok=True)
        tmp = self.hook_dir / "cmd.tmp"
        tmp.write_bytes(cmd.encode("utf-8"))
        os.replace(tmp, self.hook_dir / "cmd.txt")
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if self.proc.poll() is not None:
                raise Fail(f"앱이 꺼졌습니다 (종료 코드 {self.proc.returncode})")
            try:
                first = done.read_bytes()
                time.sleep(0.03)
                if done.read_bytes() == first:
                    done.unlink(missing_ok=True)
                    return first.decode("utf-8", "replace")
            except OSError:
                time.sleep(0.05)
        raise Fail(f"훅이 답하지 않았습니다: {cmd[:80]}")

    def eval(self, webview: str, expr: str, timeout: float = 30):
        """웹뷰에서 식을 돌린다 (사용자 동작으로, Promise는 기다린다)"""
        out = self.hook(f"gesture {webview} {expr}", timeout)
        try:
            d = json.loads(out)
        except ValueError:
            raise Fail(f"{webview}에서 돌리지 못했습니다: {out[:200]}") from None
        if "exceptionDetails" in d:
            ex = d["exceptionDetails"]
            raise Fail(f"{webview} 스크립트 오류: {ex.get('exception', {}).get('description', ex.get('text', ''))[:200]}")
        return d.get("result", {}).get("value")

    def call(self, command: str, args: dict | None = None, webview: str = "ui", timeout: float = 30) -> dict:
        """앱 명령을 부른다: {"ok": true, "value": …} 또는 {"ok": false, "error": …}"""
        expr = (
            f"window.__TAURI_INTERNALS__.invoke({json.dumps(command)}, {json.dumps(args or {})})"
            ".then(r => JSON.stringify({ok: true, value: r === undefined ? null : r}), e => JSON.stringify({ok: false, error: String(e)}))"
        )
        return json.loads(self.eval(webview, expr, timeout))

    def invoke(self, command: str, args: dict | None = None, webview: str = "ui", timeout: float = 30):
        r = self.call(command, args, webview, timeout)
        if not r["ok"]:
            raise Fail(f"{command}: {r['error']}")
        return r["value"]

    def state(self) -> dict:
        return json.loads(self.hook("state x"))

    def tab(self, tid: int) -> dict | None:
        return next((t for t in self.state()["tabs"] if t["id"] == tid), None)

    def labels(self) -> list[str]:
        return re.findall(r'"([^"]+)"', self.hook("labels x"))

    def has_webview(self, tid: int) -> bool:
        return f"tab-{tid}" in self.labels()

    def ui(self, expr: str):
        return self.eval("ui", expr)

    # 탭

    def loaded(self, tid: int, url: str, timeout: float = 20) -> dict:
        def ok():
            t = self.tab(tid)
            return t if t and t["url"] == url and not t["loading"] and t["title"] else None
        return wait_until(ok, timeout, f"탭 {tid}이(가) {url}을(를) 불러오기")

    def open(self, path: str) -> int:
        """새 탭에서 시험 페이지를 열고 다 불러올 때까지 기다린다"""
        before = {t["id"] for t in self.state()["tabs"]}
        self.invoke("tab_open_url", {"input": self.url(path)})
        new = [t["id"] for t in self.state()["tabs"] if t["id"] not in before]
        if len(new) != 1:
            raise Fail(f"새 탭이 {len(new)}개 생겼습니다")
        self.loaded(new[0], self.url(path))
        return new[0]

    def close(self, *ids: int):
        for i in ids:
            if self.tab(i):
                self.invoke("tab_close", {"id": i})

    def page_request(self, tid: int, script: str, kind: str, data: dict) -> dict:
        """페이지에서 사이트 스크립트의 부탁을 보내고 답을 받는다"""
        self.rid += 1
        msg = json.dumps({"__tbs_site_script": {"script": script, "rid": self.rid, "kind": kind, "data": data}})
        expr = (
            "new Promise(r => { const on = e => { const d = e.data && e.data.__tbs_site_script_reply;"
            f" if (d && d.rid === {self.rid}) {{ chrome.webview.removeEventListener('message', on); r(JSON.stringify(d)); }} }};"
            f" chrome.webview.addEventListener('message', on); chrome.webview.postMessage({json.dumps(msg)});"
            " setTimeout(() => r('null'), 10000); })"
        )
        reply = json.loads(self.eval(f"tab-{tid}", expr))
        if reply is None:
            raise Fail(f"{kind} 부탁에 답이 없습니다")
        return reply

    def set_settings(self, **changes):
        s = self.invoke("settings_get")
        for k, v in changes.items():
            if isinstance(v, dict):
                s[k] = {**s[k], **v}
            else:
                s[k] = v
        return self.invoke("settings_set", {"value": s})

    def use_download_dir(self, name: str) -> Path:
        d = self.downloads / name
        d.mkdir(parents=True, exist_ok=True)
        if Path(self.set_settings(downloadDir=str(d))["downloadDir"]) != d:
            raise Fail("다운로드 폴더 설정이 바뀌지 않았습니다")
        return d

    def script(self, sid: str) -> dict | None:
        return next((s for s in self.invoke("scripts_list") if s["id"] == sid), None)

    def ensure_scripts(self):
        """시험용 스크립트 둘이 켜져 있게 한다"""
        target = self.data / "scripts" / "smoke-noperm"
        if not target.exists():
            shutil.copytree(FIXTURES / "scripts" / "smoke-noperm", target)
        if not self.script("smoke-noperm"):
            self.invoke("scripts_reload")
        s = self.script("smoke-fixture")
        if s and not s["enabled"]:
            self.invoke("script_set_enabled", {"id": "smoke-fixture", "enabled": True, "confirmed": True})

    def alerts(self, webview: str = "ui") -> list[str]:
        return self.eval(webview, "[...document.querySelectorAll('[role=alert], .notice, .error')].map(e => e.textContent.trim()).filter(Boolean)")

    def popup_text(self) -> str:
        return self.eval("popup", "document.body.innerText") or ""


def write_json(path: Path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


def wait_until(fn, timeout: float, what: str):
    end = time.monotonic() + timeout
    last = None
    while True:
        last = fn()
        if last:
            return last
        if time.monotonic() > end:
            raise Fail(f"{what}: {timeout:g}초 안에 되지 않았습니다")
        time.sleep(0.2)


def expect(cond, why: str):
    if not cond:
        raise Fail(why)


def index_of(app: App, tid: int) -> int:
    return [t["id"] for t in app.state()["tabs"]].index(tid)


# ───────────── 확인 ─────────────

def check_startup(app: App):
    s = app.state()
    expect(len(s["tabs"]) == 1 and s["tabs"][0]["url"] == "" and s["active"] == s["tabs"][0]["id"], f"빈 탭 하나가 아닙니다: {s['tabs']}")
    ui = app.ui("({rows: document.querySelectorAll('[data-tab-id]').length, newtab: !!document.querySelector('.newtab')})")
    expect(ui == {"rows": 1, "newtab": True}, f"화면이 다릅니다: {ui}")
    expect(not app.alerts(), f"알림이 떠 있습니다: {app.alerts()}")
    expect(not any(l.startswith("tab-") for l in app.labels()), "빈 탭에 웹뷰가 있습니다")
    wait_until(lambda: "광고 차단 엔진 준비됨" in read_log(app), 10, "광고 차단 엔진 준비")
    log = read_log(app)
    expect("광고 차단 목록 갱신 실패" not in log and "스크립틀릿을 만들지 못" not in log, "광고 차단이 목록을 받으려 했습니다")
    expect("adblock-scriptlets" not in app.labels(), "스크립틀릿용 웹뷰가 떴습니다")


READ_ONLY = [
    ("get_state", None), ("bookmarks_get", None), ("take_notices", None), ("settings_get", None),
    ("downloads_get", None), ("fullscreen_state", None), ("scripts_list", None), ("sidebar_get", None),
    ("permission_requests", None), ("update_get", None), ("ext_list", None), ("adblock_lists", None),
    ("adblock_status", {"tab": None}), ("top_sites", None), ("history_search", {"query": "", "before": None}),
    ("suggest", {"query": "smoke"}), ("script_get", {"id": "smoke-fixture"}),
]


def check_commands(app: App):
    bad = [f"{c}: {r['error']}" for c, a in READ_ONLY if not (r := app.call(c, a))["ok"]]
    expect(not bad, "; ".join(bad))
    expect(app.invoke("take_notices") == [], "알림이 남아 있습니다")
    expect(app.invoke("script_get", {"id": "smoke-fixture"}), "script_get이 시험용 스크립트를 찾지 못했습니다")
    for c, a in [("ext_list", None), ("scripts_list", None), ("adblock_status", {"tab": None})]:
        r = app.call(c, a, webview="popup")
        expect(r["ok"], f"팝업에서 {c}: {r.get('error')}")
    n = len(app.state()["tabs"])
    r = app.call("tab_create", webview="popup")
    expect(not r["ok"], "팝업에서 tab_create가 불렸습니다")
    expect(len(app.state()["tabs"]) == n, "팝업에서 탭이 생겼습니다")


def check_tabs(app: App):
    first = app.state()["active"]
    app.invoke("navigate", {"id": first, "input": app.url("a.html")})
    app.loaded(first, app.url("a.html"))
    expect(app.has_webview(first), "이동한 탭에 웹뷰가 없습니다")
    blank = app.invoke("tab_create")
    s = app.state()
    expect(s["active"] == blank and s["tabs"][0]["id"] == blank and s["tabs"][0]["url"] == "", "새 탭이 맨 위의 빈 탭이 아닙니다")
    # 주소창처럼 http:// 없이 넣는다
    before = {t["id"] for t in s["tabs"]}
    app.invoke("tab_open_url", {"input": f"127.0.0.1:{app.port}/b.html"})
    s = app.state()
    b = s["tabs"][0]["id"]
    expect(b not in before and s["active"] == b, "새 탭에서 열기가 맨 위 탭을 고르지 않았습니다")
    app.loaded(b, app.url("b.html"))
    app.invoke("tab_activate", {"id": first})
    expect(app.state()["active"] == first, "탭 고르기가 되지 않았습니다")
    app.invoke("tab_close", {"id": b})
    expect(not app.tab(b), "닫은 탭이 남아 있습니다")
    wait_until(lambda: not app.has_webview(b), 5, "닫은 탭의 웹뷰 닫기")
    app.invoke("run_shortcut", {"name": "reopen_tab"})
    s = app.state()
    back = next((t for t in s["tabs"] if t["url"] == app.url("b.html")), None)
    expect(back and s["active"] == back["id"], "닫은 탭이 되살아나지 않았습니다")
    app.loaded(back["id"], app.url("b.html"))
    app.close(back["id"], blank)


def check_groups(app: App):
    a = app.open("a.html")
    b = app.open("b.html")
    gid = app.invoke("group_create", {"tab": a, "title": "스모크 묶음"})
    s = app.state()
    expect(any(g["id"] == gid and g["title"] == "스모크 묶음" for g in s["groups"]), f"그룹이 없습니다: {s['groups']}")
    expect(app.tab(a)["group"] == gid, "탭이 그룹에 들지 않았습니다")
    app.invoke("group_rename", {"id": gid, "title": "스모크 이름"})
    expect(any(g["id"] == gid and g["title"] == "스모크 이름" for g in app.state()["groups"]), "그룹 이름이 바뀌지 않았습니다")
    wait_until(lambda: "스모크 이름" in app.ui("document.body.innerText"), 5, "사이드바에 그룹 이름 보이기")
    app.invoke("tab_move", {"id": b, "before": a, "group": gid})
    expect(app.tab(b)["group"] == gid and index_of(app, b) == index_of(app, a) - 1, "탭이 그룹 안 그 자리로 옮겨지지 않았습니다")
    app.invoke("tab_set_keep_alive", {"id": a, "keep": True})
    expect(app.tab(a)["keepAlive"], "항상 유지가 켜지지 않았습니다")
    app.invoke("tab_set_keep_alive", {"id": a, "keep": False})
    expect(not app.tab(a)["keepAlive"], "항상 유지가 꺼지지 않았습니다")
    app.invoke("group_ungroup", {"id": gid})
    s = app.state()
    expect(not s["groups"] and app.tab(a)["group"] is None and app.tab(b)["group"] is None, "그룹이 풀리지 않았습니다")
    app.close(a, b)


def check_internal(app: App):
    app.invoke("run_shortcut", {"name": "history"})
    s = app.state()
    h = next((t for t in s["tabs"] if t["url"] == "tbs://history"), None)
    expect(h and s["active"] == h["id"], "방문 기록 탭이 열리지 않았습니다")
    expect(not app.has_webview(h["id"]), "내부 페이지에 웹뷰가 있습니다")
    wait_until(lambda: app.ui("document.querySelector('.page-title')?.textContent") == "방문 기록", 5, "방문 기록 화면 그리기")
    t = app.open("a.html")
    app.invoke("navigate", {"id": t, "input": "tbs://settings"})
    wait_until(lambda: not app.has_webview(t), 5, "내부 페이지로 가며 웹뷰 닫기")
    expect(app.tab(t)["url"] == "tbs://settings", "탭 주소가 내부 페이지가 아닙니다")
    wait_until(lambda: app.ui("document.querySelector('.page-title')?.textContent") == "설정", 5, "설정 화면 그리기")
    app.invoke("navigate", {"id": t, "input": app.url("b.html")})
    app.loaded(t, app.url("b.html"))
    expect(app.has_webview(t), "웹 주소로 돌아와도 웹뷰가 없습니다")
    app.close(h["id"], t)


def check_history_nav(app: App):
    t = app.open("a.html")
    app.eval(f"tab-{t}", "location.href = '/b.html?inpage'; 1")
    app.loaded(t, app.url("b.html?inpage"))
    wait_until(lambda: app.tab(t)["canGoBack"], 5, "뒤로 가기 켜짐")
    app.invoke("go_back", {"id": t})
    app.loaded(t, app.url("a.html"))
    wait_until(lambda: app.tab(t)["canGoForward"], 5, "앞으로 가기 켜짐")
    app.invoke("go_forward", {"id": t})
    app.loaded(t, app.url("b.html?inpage"))
    app.close(t)


def check_window_open(app: App):
    t = app.open("a.html")
    expect(app.eval(f"tab-{t}", "window.__w = window.open('/b.html?popup'); !!window.__w") is True, "window.open이 창을 돌려주지 않았습니다")
    url = app.url("b.html?popup")
    p = wait_until(lambda: next((x["id"] for x in app.state()["tabs"] if x["url"] == url), None), 10, "새 창 탭 생기기")
    app.loaded(p, url)
    s = app.state()
    expect(s["active"] == p and index_of(app, p) == index_of(app, t) - 1, "새 창 탭이 연 탭 바로 위에서 선택되지 않았습니다")
    app.eval(f"tab-{t}", "window.__w.close(); 1")
    wait_until(lambda: not app.tab(p), 5, "window.close()로 탭 닫기")
    expect(app.state()["active"] == t, "닫은 뒤 연 탭이 선택되지 않았습니다")
    app.close(t)


def check_home(app: App):
    app.ensure_scripts()
    t = app.open("a.html")
    r = app.page_request(t, "smoke-fixture", "tabs", {"action": "home"})
    expect(r["ok"], f"홈 부탁이 거절됐습니다: {r.get('error')}")
    wait_until(lambda: (x := app.tab(t))["url"] == "" and x["canGoBack"], 5, "홈(새 탭 화면)으로 덮기")
    expect(app.has_webview(t), "홈으로 가며 웹뷰가 닫혔습니다")
    wait_until(lambda: app.ui("!!document.querySelector('.newtab')"), 5, "새 탭 화면 보이기")
    app.invoke("go_back", {"id": t})
    wait_until(lambda: (x := app.tab(t))["url"] == app.url("a.html") and x["canGoForward"], 5, "뒤로: 밑의 페이지로")
    app.invoke("go_forward", {"id": t})
    wait_until(lambda: app.tab(t)["url"] == "", 5, "앞으로: 다시 홈")
    app.invoke("navigate", {"id": t, "input": app.url("b.html")})
    app.loaded(t, app.url("b.html"))
    expect(not app.tab(t)["canGoForward"], "새 주소로 간 뒤에도 앞으로 가기가 켜져 있습니다")
    expect(not app.ui("!!document.querySelector('.newtab')"), "새 주소로 간 뒤에도 새 탭 화면이 보입니다")
    app.close(t)


def check_scripts(app: App):
    app.ensure_scripts()
    for sid in ("smoke-fixture", "smoke-noperm"):
        s = app.script(sid)
        expect(s and s["enabled"] and not s["error"], f"{sid}: {s}")
    t = app.open("a.html")
    wait_until(lambda: app.eval(f"tab-{t}", "document.documentElement.dataset.smokeScript || ''") == "1", 5, "스크립트가 페이지에 들기")
    app.invoke("script_set_enabled", {"id": "smoke-fixture", "enabled": False})
    expect(not app.script("smoke-fixture")["enabled"], "스크립트가 꺼지지 않았습니다")
    # tabs·storage 권한이 있어 확인 없이는 켜지 않고 켜기 확인을 띄운다
    app.invoke("script_set_enabled", {"id": "smoke-fixture", "enabled": True})
    expect(not app.script("smoke-fixture")["enabled"], "확인 없이 켜졌습니다")
    wait_until(lambda: "스모크 스크립트" in app.popup_text(), 5, "켜기 확인 띄우기")
    app.invoke("script_set_enabled", {"id": "smoke-fixture", "enabled": True, "confirmed": True}, webview="popup")
    app.invoke("popup_hide", webview="popup")
    expect(app.script("smoke-fixture")["enabled"], "확인한 뒤에도 켜지지 않았습니다")
    app.invoke("script_set_pinned", {"id": "smoke-fixture", "pinned": True})
    expect(app.script("smoke-fixture")["pinned"], "고정되지 않았습니다")
    app.invoke("script_set_pinned", {"id": "smoke-fixture", "pinned": False})
    expect(not app.script("smoke-fixture")["pinned"], "고정이 풀리지 않았습니다")
    app.invoke("script_set_value", {"id": "smoke-fixture", "key": "count", "value": 5})
    expect(app.invoke("script_get", {"id": "smoke-fixture"})["values"].get("count") == 5, "설정 값이 바뀌지 않았습니다")
    for c, a in [("script_set_value", {"id": "smoke-none", "key": "count", "value": 1}), ("script_set_pinned", {"id": "smoke-none", "pinned": True})]:
        r = app.call(c, a)
        expect(not r["ok"] and "스크립트를 찾지 못했습니다" in r["error"], f"{c}: 없는 스크립트에 {r}")
    app.close(t)


def check_requests(app: App):
    app.ensure_scripts()
    d = app.use_download_dir("requests")
    t = app.open("a.html")
    r = app.page_request(t, "smoke-fixture", "storage", {"op": "set", "key": "smoke", "value": {"n": 1}})
    expect(r["ok"], f"storage set: {r}")
    r = app.page_request(t, "smoke-fixture", "storage", {"op": "get", "key": "smoke"})
    expect(r["ok"] and r["result"] == {"n": 1}, f"storage get: {r}")
    r = app.page_request(t, "smoke-fixture", "saveFile", {"name": "smoke-save.txt", "text": "안녕 smoke"})
    expect(r["ok"], f"saveFile: {r}")
    p = Path(r["result"]["path"])
    expect(p.parent == d and p.read_text(encoding="utf-8") == "안녕 smoke", f"saveFile 파일이 다릅니다: {p}")
    r = app.page_request(t, "smoke-fixture", "screenshot", {})
    expect(r["ok"], f"screenshot: {r}")
    p = Path(r["result"]["path"])
    expect(p.parent == d and p.stat().st_size > 0 and p.read_bytes()[:8] == b"\x89PNG\r\n\x1a\n", f"스크린샷이 PNG가 아닙니다: {p}")
    r = app.page_request(t, "smoke-noperm", "storage", {"op": "get"})
    expect(not r["ok"] and "부탁할 수 없습니다" in r["error"], f"권한 없는 스크립트의 부탁이 거절되지 않았습니다: {r}")
    r = app.page_request(t, "smoke-fixture", "nope", {})
    expect(not r["ok"] and "부탁할 수 없는 일" in r["error"], f"모르는 부탁이 거절되지 않았습니다: {r}")
    app.close(t)


def check_download(app: App):
    d = app.use_download_dir("page")
    t = app.open("a.html")
    app.eval(f"tab-{t}", "(() => { const a = document.createElement('a'); a.href = URL.createObjectURL(new Blob(['hello smoke'], {type: 'text/plain'})); a.download = 'smoke-download.txt'; document.body.appendChild(a); a.click(); return 1; })()")

    def done():
        return next((x for x in app.invoke("downloads_get") if Path(x["path"]).name == "smoke-download.txt" and x["state"] == "done"), None)

    item = wait_until(done, 15, "다운로드 끝나기")
    p = Path(item["path"])
    expect(p.parent == d and p.read_text(encoding="utf-8") == "hello smoke", f"받은 파일이 다릅니다: {p}")
    app.close(t)


def find_bookmark(items: list, url: str):
    for n in items:
        if n["type"] == "link" and n["url"] == url:
            return n
        if n["type"] == "folder" and (f := find_bookmark(n["items"], url)):
            return f
    return None


def check_bookmarks_theme(app: App):
    url, title = app.url("a.html"), "스모크 북마크"
    bid = app.invoke("bookmark_add", {"parent": None, "before": None, "title": title, "url": url, "icon": None})
    found = find_bookmark(app.invoke("bookmarks_get")["items"], url)
    expect(found and found["id"] == bid, "추가한 북마크가 없습니다")
    wait_until(lambda: title in app.ui("document.body.innerText"), 5, "사이드바에 북마크 보이기")
    expect(app.invoke("bookmark_toggle", {"title": title, "url": url, "icon": None}) is False, "별 버튼이 북마크를 빼지 않았습니다")
    expect(not find_bookmark(app.invoke("bookmarks_get")["items"], url), "뺀 북마크가 남아 있습니다")
    expect(app.invoke("bookmark_toggle", {"title": title, "url": url, "icon": None}) is True, "별 버튼이 북마크를 넣지 않았습니다")
    found = find_bookmark(app.invoke("bookmarks_get")["items"], url)
    expect(found, "넣은 북마크가 없습니다")
    app.invoke("bookmark_remove", {"id": found["id"]})
    expect(not find_bookmark(app.invoke("bookmarks_get")["items"], url), "지운 북마크가 남아 있습니다")
    for theme in ("dark", "light"):
        expect(app.set_settings(theme=theme)["theme"] == theme, f"테마가 {theme}로 저장되지 않았습니다")
        wait_until(lambda: app.ui("document.documentElement.dataset.theme") == theme, 5, f"화면 테마 {theme}")


def check_popup(app: App):
    base = visible_windows(app.proc.pid)
    items = [{"label": "스모크 항목", "danger": False, "icon": None}]
    app.invoke("popup_show", {"kind": "menu", "x": 100, "y": 100, "w": 180, "h": 80, "data": {"theme": "light", "items": items}})
    wait_until(lambda: "스모크 항목" in app.popup_text(), 5, "팝업에 메뉴 항목 그리기")
    wait_until(lambda: visible_windows(app.proc.pid) == base + 1, 5, "팝업 창 보이기")
    app.invoke("popup_hide")
    wait_until(lambda: visible_windows(app.proc.pid) == base, 5, "팝업 창 숨기기")


def check_sidebar(app: App):
    extra = [app.invoke("tab_create"), app.invoke("tab_create")]
    n = len(app.state()["tabs"])
    try:
        for collapsed, root in ((True, "nav.collapsed"), (False, "nav.sidebar")):
            app.invoke("sidebar_set_collapsed", {"collapsed": collapsed})
            expect(app.invoke("sidebar_get") is collapsed, "사이드바 상태가 바뀌지 않았습니다")
            wait_until(lambda: app.ui(f"document.querySelectorAll('{root} [data-tab-id]').length") == n, 5, f"{root}의 탭 {n}개")
    finally:
        app.invoke("sidebar_set_collapsed", {"collapsed": False})
        app.close(*extra)


def check_memory(app: App):
    tabs = [app.open("a.html"), app.open("b.html"), app.open("a.html?m")]
    background = tabs[:2]
    try:
        app.set_settings(memory={"maxAwakeTabs": 1})
        # 20초마다 본다 (memory.rs의 TICK)
        wait_until(lambda: not any(app.has_webview(t) for t in background), 50, "뒤 탭의 웹뷰 버리기")
        expect(all(app.tab(t) for t in tabs), "버린 탭이 목록에서 사라졌습니다")
        expect(app.has_webview(tabs[2]), "보고 있는 탭의 웹뷰가 버려졌습니다")
        app.invoke("tab_activate", {"id": tabs[0]})
        wait_until(lambda: app.has_webview(tabs[0]), 5, "버린 탭을 고르면 다시 열기")
        app.loaded(tabs[0], app.url("a.html"))
        expect(app.eval(f"tab-{tabs[0]}", "document.title") == "Smoke A", "다시 연 페이지가 다릅니다")
    finally:
        app.set_settings(memory={"maxAwakeTabs": 12})
        app.close(*tabs)


def check_extension(app: App):
    name = "TBs Smoke Extension"
    out = app.hook(f"ext_install x {FIXTURES / 'extension'}")
    expect(out.startswith("prepared "), f"설치 준비: {out}")
    wait_until(lambda: name in app.popup_text(), 10, "설치 확인 띄우기")
    app.invoke("ext_install_confirm", {"accept": True}, webview="popup", timeout=90)

    def entry():
        return next((e for e in app.invoke("ext_list") if e["name"] == name), None)

    e = wait_until(entry, 10, "확장 목록에 들기")
    expect(e["id"] and e["enabled"] and not e["error"], f"설치한 확장: {e}")
    eid = e["id"]
    expect(eid in app.hook("ext_list ui", 40), "WebView2 프로필에 확장이 없습니다")
    t = app.open("a.html")
    wait_until(lambda: app.eval(f"tab-{t}", "document.documentElement.dataset.smokeExt || ''") == "1", 15, "확장 콘텐츠 스크립트가 페이지에 들기")
    for on in (False, True):
        app.invoke("ext_set_enabled", {"id": eid, "enabled": on}, timeout=60)
        expect(entry()["enabled"] is on, f"확장이 {'켜지지' if on else '꺼지지'} 않았습니다")
    app.invoke("ext_remove", {"id": eid}, timeout=60)
    expect(not entry(), "지운 확장이 목록에 남아 있습니다")
    expect(eid not in app.hook("ext_list ui", 40), "지운 확장이 WebView2 프로필에 남아 있습니다")
    app.close(t)


def read_log(app: App) -> str:
    try:
        return app.log.read_bytes().decode("utf-8", "replace")
    except OSError:
        return ""


def check_final(app: App):
    expect(not app.alerts(), f"UI에 알림이 떠 있습니다: {app.alerts()}")
    expect(not app.alerts("popup"), f"팝업에 오류가 떠 있습니다: {app.alerts('popup')}")
    bad = [l.strip() for l in read_log(app).splitlines() if "[tbs ERROR]" in l or "panicked" in l or "[tbs WARN] UI:" in l]
    expect(not bad, "로그: " + " / ".join(bad[:5]))
    app.track()


CHECKS = [
    ("시작 화면", check_startup, True),
    ("읽기 명령과 팝업 권한", check_commands, False),
    ("탭: 이동·새 탭·고르기·닫기·되살리기", check_tabs, False),
    ("탭 그룹과 항상 유지", check_groups, False),
    ("내부 페이지", check_internal, False),
    ("페이지 안 이동과 뒤로·앞으로", check_history_nav, False),
    ("window.open과 window.close", check_window_open, False),
    ("홈", check_home, False),
    ("사이트 스크립트", check_scripts, False),
    ("페이지의 부탁", check_requests, False),
    ("다운로드", check_download, False),
    ("북마크와 테마", check_bookmarks_theme, False),
    ("팝업 창", check_popup, False),
    ("사이드바 접기", check_sidebar, False),
    ("메모리 정책", check_memory, False),
    ("크롬 확장", check_extension, False),
    ("마무리: 알림·로그 오류 없음", check_final, True),
]


def build():
    print("빌드", flush=True)
    for cmd, env in (("npm run build:ui", None), ("cargo build --manifest-path src-tauri/Cargo.toml", {**os.environ, "CARGO_TARGET_DIR": str(TARGET_DIR)})):
        r = subprocess.run(cmd, shell=True, cwd=ROOT, env=env, capture_output=True)
        if r.returncode != 0:
            sys.stdout.write((r.stdout + r.stderr).decode("utf-8", "replace"))
            raise Fail(f"빌드 실패: {cmd}")


def main() -> int:
    for s in (sys.stdout, sys.stderr):
        s.reconfigure(encoding="utf-8")
    ap = argparse.ArgumentParser(description="TB's Browser 스모크 시험")
    ap.add_argument("--no-build", action="store_true", help="빌드하지 않는다")
    ap.add_argument("--keep", action="store_true", help="데이터 폴더와 로그를 남긴다")
    ap.add_argument("--only", metavar="글자", help="이름에 그 글자가 든 확인만 (시작·마무리 확인은 늘 한다)")
    args = ap.parse_args()
    checks = [c for c in CHECKS if c[2] or not args.only or args.only.lower() in c[0].lower()]
    started = time.monotonic()
    passed = failed = 0
    server = app = None
    try:
        desktop2_guid()
        if not args.no_build:
            build()
        if not EXE.exists():
            raise Fail(f"실행 파일이 없습니다: {EXE}")
        server = start_server()
        app = App(server.server_address[1])
        app.prepare()
        app.start()
        for name, fn, _ in checks:
            try:
                fn(app)
                passed += 1
                print(f"통과  {name}", flush=True)
            except Fail as e:
                failed += 1
                print(f"실패  {name}: {e}", flush=True)
    except Fail as e:
        failed += 1
        print(f"실패  준비: {e}", flush=True)
    except KeyboardInterrupt:
        failed += 1
        print("중단", flush=True)
    finally:
        if app:
            left = app.stop()
            if left:
                failed += 1
                print(f"실패  끝내기: 남은 프로세스 {', '.join(left)}", flush=True)
            if args.keep:
                print(f"남김  {app.dir}", flush=True)
            else:
                app.cleanup()
        if server:
            server.shutdown()
    total = passed + failed
    print(f"{'통과' if failed == 0 else '실패'}: {total}개 중 {passed}개 통과, {time.monotonic() - started:.0f}초", flush=True)
    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
