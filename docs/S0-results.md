# 0단계 기술 검증 결과

Nov 10, 2026 · docs/spec.md 9장 0단계, 10장 항목 시험

## 요약

| 번호 | 확인할 것 | 결과 | 한 줄 근거 |
| --- | --- | --- | --- |
| 1 | 자식 웹뷰 여러 개 배치·크기·보이기·숨기기 | **됨** | UI 웹뷰 1 + 콘텐츠 웹뷰 2를 한 창에 놓고 위치·크기·보이기·숨기기를 바꿨다 |
| 2 | 네이티브 WebView2 API 호출 | **됨** | `TrySuspend`, `AddScriptToExecuteOnDocumentCreated`, `WebResourceRequested`, `CapturePreview` 모두 성공 |
| 3 | 환경·사용자 데이터 폴더 공유, 로그인 공유 | **됨** (설정 필요) | 브라우저 프로세스·데이터 폴더가 같고 쿠키가 공유된다. 환경 객체를 하나로 쓰려면 `with_environment`가 필요하다 |
| 4 | 페이지 카드 12px 둥근 모서리 | **됨** (계단 현상) | 창 영역 잘라내기로 된다. 가장자리가 매끄럽지 않다. 비주얼 호스팅은 시험하지 않았다 |
| 5 | 콘텐츠 위 팝업 | **됨** | 자식 웹뷰 방식과 투명 팝업 창 방식 둘 다 된다. 팝업 창은 활성화 없이 띄워야 포커스가 유지된다 |
| 6 | 창 틀(드래그, 크기 조절, 더블클릭 최대화, 스냅 레이아웃) | **됨** (우회 2개) | 위쪽 변 크기 조절과 스냅 레이아웃은 우회 코드를 넣어야 된다 |
| 7 | 페이지에 포커스가 있을 때 단축키 | **됨** | `AcceleratorKeyPressed`로 5.6의 단축키 16개를 모두 받았다 |
| 8 | `TrySuspend` 조건과 줄어드는 메모리 | **됨** | 보이는 웹뷰는 실패한다. 숨긴 뒤 성공했고, 유튜브 렌더러가 241→128MB로 줄었다 |
| 9 | 캐시·서비스 워커 사이트에서 리소스 바꿔치기 | **일부만 됨 → 대안** | HTTP 캐시에 있는 파일은 캐시를 비워야 바뀐다. 서비스 워커를 거친 요청은 바꿔치지 못했다 |
| 10 | 웹 페이지가 앱 명령을 부를 수 없는지 | **됨** | 원격·로컬 페이지, 앱 주소로 이동한 콘텐츠 웹뷰 모두 앱 명령이 거부됐다 |
| 11 | 비밀번호 저장·자동 입력 | **됨** | "암호를 저장하시겠습니까?" 창이 뜨고, 다시 열면 자동 입력된다 |
| 12 | DRM 영상 재생 | **됨** (넷플릭스는 미확인) | Widevine·PlayReady를 지원한다. Widevine 시험 영상이 재생됐다. 넷플릭스는 계정이 필요해 사용자 확인이 필요하다 |
| 13 | 압축 해제 확장 불러오기 | **됨** | 확장이 등록됐고 콘텐츠 스크립트가 페이지에서 실행됐다 |

결론: Tauri 2 + `unstable`로 진행할 수 있다. `wry`·`tao` 직접 사용이나 `webview2-com` 직접 제어로 바꿀 필요는 없다. 다만 아래 "spec에 반영할 것"의 우회와 설정이 필요하다.

## 시험 환경

| 항목 | 값 |
| --- | --- |
| OS | Windows 11 Home 10.0.26100, 화면 2560×1400, 배율 100% |
| WebView2 런타임 | 157.0.4341.42 (Evergreen) |
| Rust | 1.99.0, `stable-x86_64-pc-windows-msvc` (VS 2022 빌드 도구) |
| 고정한 버전 | `tauri =2.12.5`(`unstable`), `tauri-build =2.7.4`, `webview2-com =0.39.1`, `windows =0.62.2`. 잠금 파일 기준 `tauri-runtime-wry 2.12.3`, `wry 0.57.1`, `tao 0.37.2` |
| 시험 앱 | `spikes/s0/` (버리는 코드. 본 기능 코드가 아니다) |
| 데이터 폴더 | `spikes/s0/.data/webview` (실제 경로 `%LOCALAPPDATA%\TBs Browser\`는 건드리지 않았다) |

Tauri 3.0 알파가 나와 있지만 spec대로 2.x 최신 안정판을 썼다.

시험 방법은 세 가지다. 앱이 스스로 도는 자동 시험(`S0_AUTORUN=1`), 앱의 시험 전용 명령 통로(`out/cmd.txt`), PowerShell로 실제 마우스·키 입력과 화면 캡처(`tools/gui.ps1`). 원본 기록은 `spikes/s0/out/`에 있다. `report-autorun.jsonl`, `events*.log`, `shot-*.png`.

## 항목별 결과

### 1. 한 창에 여러 웹뷰 — 됨

- `WindowBuilder`로 장식 없는 창을 만들고 `Window::add_child`로 UI 웹뷰(창 전체, `auto_resize`)와 콘텐츠 웹뷰 `c1`(example.com), `c2`(wikipedia.org)를 얹었다.
- UI가 카드 사각형을 `set_content_bounds`로 알리면 호스트가 `set_position`·`set_size`로 옮긴다. 값이 그대로 반영됐다. (카드 276,50 / 996×742, 임의 이동 300,120 / 400×300)
- `show()`·`hide()`를 네 번 번갈아 했고, 숨긴 쪽 페이지는 `document.visibilityState = "hidden"`이 됐다.
- 나중에 만든 웹뷰가 위에 놓인다. 콘텐츠 웹뷰를 UI 웹뷰 다음에 만들어야 한다.

### 2. 네이티브 WebView2 API — 됨

`Webview::with_webview`가 주는 `PlatformWebview::controller()`와 `environment()`에서 `webview2-com`으로 바로 불렀다.

| API | 결과 |
| --- | --- |
| `AddScriptToExecuteOnDocumentCreated` | 등록 성공. 다음 문서에서 페이지 스크립트보다 먼저 실행됐다 |
| `CapturePreview`(PNG) | 46,513바이트 PNG 저장 (`out/capture-c1.png`) |
| `TrySuspend` / `Resume` / `IsSuspended` | 호출됨. 조건은 8번 |
| `add_WebResourceRequested` + 필터 | 호출됨. 동작은 9번 |
| `SetMemoryUsageTargetLevel`, `ClearBrowsingData`, `AddBrowserExtension`, `AcceleratorKeyPressed`, 설정(`ICoreWebView2Settings4`) | 모두 호출됨 |

주의: 비동기 완료 처리기는 메인 스레드에서 돈다. 메인 스레드에서 결과를 기다리면 멈춘다. 시험 앱은 별도 스레드에서 채널로 기다렸다.

### 3. 환경·데이터 폴더 공유 — 됨 (설정 필요)

- UI·c1·c2의 `BrowserProcessId`가 모두 같았고(한 브라우저 프로세스), `UserDataFolder`도 같았다.
- c1에서 만든 쿠키와 `localStorage`를 c2에서 읽었다. 한 탭에서 로그인하면 다른 탭도 로그인 상태가 된다.
- 다만 Tauri(wry)는 웹뷰마다 `CreateCoreWebView2EnvironmentWithOptions`를 따로 불러 **환경 객체가 웹뷰마다 다르다**. 같은 폴더·같은 옵션이라 프로세스는 공유되지만, spec 7장의 "환경 하나"를 글자 그대로 지키려면 `WebviewBuilder::with_environment(env)`로 첫 웹뷰의 환경을 넘긴다. 시험해 보니 c1과 새 웹뷰의 환경 포인터가 같았다. 이 빌더는 메인 스레드에서 만들어야 한다(`with_webview` 안에서 `add_child`).
- 모든 웹뷰의 환경 옵션이 같아야 한다. 확장 켜기(13번)도 UI 웹뷰와 팝업 웹뷰까지 똑같이 켰다.

### 4. 페이지 카드 둥근 모서리 — 됨 (계단 현상), 비주얼 호스팅 미시험

- 콘텐츠 웹뷰를 담은 자식 창(`WRY_WEBVIEW`, `controller.ParentWindow()`)에 `SetWindowRgn(CreateRoundRectRgn(..., 24, 24))`를 걸었다. 12px 둥근 모서리가 나온다 (`out/shot-main-round12.png`).
- 창 영역은 안티앨리어싱이 없다. 밝은 페이지는 뒤의 UI 카드(흰 바탕, 둥근 모서리)와 섞여 거의 티가 안 난다. **어두운 페이지에서는 모서리 계단이 보인다** (`out/zoom-dark-topleft-r12-1x.png`).
- 크기가 바뀔 때마다 영역을 다시 걸어야 한다. 시험 앱은 `set_content_bounds`마다 다시 건다.
- 비주얼 호스팅(`CompositionController` + DirectComposition 둥근 잘라내기)은 시험하지 않았다. Tauri는 비주얼 호스팅을 지원하지 않아서, 쓰려면 콘텐츠 웹뷰를 `webview2-com`으로 직접 만들고 마우스·키 입력도 직접 넘겨야 한다. 이 정도면 2장의 "앱 틀" 결정이 바뀐다.
- 배율 100%에서만 확인했다. 125%·150%에서는 확인하지 않았다.

### 5. 콘텐츠 위 팝업 — 됨

두 방식을 시험했다.

| 방식 | 결과 | 한계 |
| --- | --- | --- |
| 자식 웹뷰(`popup`)를 콘텐츠 웹뷰보다 나중에 만들어 위에 둔다 | 콘텐츠와 UI 위에 겹쳐 보인다 (`shot-popup-child.png`) | 불투명한 사각형만 된다. 바깥으로 퍼지는 그림자가 안 되고 모서리는 4번과 같은 계단 현상이 생긴다 |
| 투명한 최상위 팝업 창(`WebviewWindowBuilder`, `transparent`, `decorations(false)`, `owner_raw(메인 창)`) | 둥근 모서리가 매끄럽고 그림자도 그려진다 (`shot-popup-window-noact.png`) | `show()`는 팝업 창을 활성화해 포커스를 빼앗는다. `WS_EX_NOACTIVATE` + `ShowWindow(SW_SHOWNOACTIVATE)`로 띄우면 포커스가 유지됐다(입력 칸에 계속 타이핑됨) |

- `parent_raw`를 쓰면 자식 창(WS_CHILD)이 되어 엉뚱한 곳에 뜬다. 소유 창은 `owner_raw`로 지정한다.
- 소유 창은 메인 창을 따라 움직이지 않는다. 메인 창이 움직이거나 크기가 바뀌면 호스트가 팝업 위치를 다시 잡거나 닫아야 한다. 이 동작은 시험하지 않았다.
- 시안의 메뉴 팝업 그림자(`0 8px 24px`)까지 맞추려면 투명 팝업 창 방식이 맞다.

### 6. 창 틀 — 됨 (우회 2개)

| 동작 | 결과 |
| --- | --- |
| 툴바 빈 곳 끌어 이동 (`data-tauri-drag-region`) | 됨 |
| 빈 곳 더블클릭 최대화·복원 | 됨 |
| 화면 왼쪽 끝으로 끌어 반쪽 붙이기 | 됨 |
| 왼쪽·오른쪽·아래 변, 모서리 크기 조절 | 됨 (창 바깥의 보이지 않는 테두리) |
| **위쪽 변 크기 조절** | **처음엔 안 됨.** Tauri는 `WebviewWindow`일 때만 크기 조절용 테두리 창을 붙이고, `add_child`로 만든 웹뷰에는 붙이지 않는다. 웹뷰를 다 만든 뒤 `set_decorations(true)` → `set_decorations(false)`를 부르면 테두리 창이 붙고 위쪽 변도 된다 |
| **스냅 레이아웃(최대화 버튼에 마우스 올리기)** | **기본으로는 안 됨.** 최대화 버튼 자리에 작은 자식 창을 올리고 `WM_NCHITTEST`에 `HTMAXBUTTON`을 돌려주니 Windows 11 스냅 메뉴가 떴다 (`shot-snap-flyout3.png`). 그 창을 누르면 최대화·복원된다 |

- 반투명 레이어드 자식 창(`WS_EX_LAYERED`)으로 덮개를 만들려 했으나 생성이 실패했다(HRESULT `0x800700CB`). 평범한 자식 창은 된다.
- 덮개 창이 마우스를 가져가므로 최대화 버튼의 마우스 올림 표시는 호스트가 UI에 이벤트로 알려 줘야 한다. 아직 만들지 않았다.
- 창을 최대화하면 덮개 위치가 바뀐다. 크기 변경 이벤트마다 다시 놓는다.

### 7. 페이지에 포커스가 있을 때 단축키 — 됨

- 콘텐츠 웹뷰 컨트롤러에 `AcceleratorKeyPressed` 처리기를 걸고, 페이지 안을 누른 뒤 실제 키 입력을 보냈다.
- 5.6의 키 중 Ctrl+T, Ctrl+W, Ctrl+Shift+T, Ctrl+Tab, Ctrl+Shift+Tab, Ctrl+L, Ctrl+R, F5, Alt+←, Alt+→, Ctrl+D, Ctrl+H, Ctrl+J, Ctrl+B, Ctrl+Shift+A, F11을 **모두** 호스트가 받았다.
- `SetHandled(true)`로 처리한 키는 페이지 스크립트에 가지 않았다(페이지는 Ctrl·Alt 키 자체만 봤다).
- 처리하지 않은 Ctrl+F는 WebView2 기본 찾기 막대를 띄웠다 (`shot-after-ctrlf.png`). F12는 보내지 않았다(기본 동작이 개발자 도구).
- 포커스가 UI 웹뷰에 있을 때는 UI의 `keydown`이 Ctrl+T, Ctrl+W, Ctrl+Tab, F5를 받았다. 두 경로를 다 처리해야 한다.

### 8. `TrySuspend` 조건과 메모리 — 됨

유튜브 첫 화면을 띄운 탭(c3)으로 쟀다. 메모리는 이 앱에 속한 모든 프로세스(`GetProcessInfos` + 호스트)의 private bytes 합이다.

| 단계 | 결과 | 합계 | 유튜브 렌더러 |
| --- | --- | --- | --- |
| 보이는 상태에서 `TrySuspend` | **즉시 실패** `0x8007139F`(잘못된 상태) | — | — |
| 숨기고 깨어 있음 | — | 774.0MB | 241.6MB |
| 숨긴 뒤 `TrySuspend` | 성공, `IsSuspended = true` | 649.9MB | 128.3MB |
| `Resume` 뒤 2초 | 성공. 카운터 값이 그대로 남아 있었다 | 655.3MB | 133.9MB |
| 숨김 + `MemoryUsageTargetLevel = Low` | private은 그대로, working set 1,040 → 818MB | 647.8MB | 128.5MB |
| 웹뷰 닫기(버림) | 렌더러가 바로 끝나지 않는다. 20~30초 뒤 종료 | 591 → 356MB (30초 뒤) | 209 → 0 |

- 재우는 동안 페이지 타이머가 멈췄다(5초 동안 카운터 3 → 3). 깨운 뒤 페이지 상태는 그대로였다.
- `TrySuspend`는 웹뷰를 숨긴 뒤에만 성공한다. spec 7장의 규칙과 같다.
- 버림은 Tauri `close()`만으로 충분하다. 네이티브 `controller.Close()`를 먼저 불러도 해제 시점은 같았다. 같은 사이트를 다시 열면 남아 있던 렌더러를 재사용하기도 했다.
- 30개 탭 측정(7장)은 6단계에서 한다.

### 9. 리소스 바꿔치기 — 일부만 됨 → 대안 필요

로컬 시험 서버(`testsite/server.py`)에서 빨간 PNG를 내고, 호스트가 파란 PNG로 바꿔치는지 픽셀 색으로 확인했다.

| 경우 | 결과 |
| --- | --- |
| 캐시에 없는 새 요청 | **바뀜** (처리기 1회 호출) |
| HTTP 캐시에 있는 파일 (`max-age=3600`) | **안 바뀜.** 처리기가 아예 불리지 않는다. 새로고침해도 같다 |
| 디스크 캐시를 비운 뒤 (`ClearBrowsingData(DISK_CACHE)`) | **바뀜** |
| 서비스 워커가 `fetch()`로 대신 받는 요청 | **안 바뀜.** 기본 필터, `AddWebResourceRequestedFilterWithRequestSourceKinds(ALL)` 둘 다 처리기가 불리지 않았다 |
| 서비스 워커의 Cache Storage에서 내는 응답 | **안 바뀜** (네트워크로 가지 않으니 당연하다) |

- 실제 대상인 chess.com을 열어 보니 서비스 워커가 없었고, 기물은 `assets-themes.chess.com`의 PNG를 CSS 배경으로 쓴다. 6.5의 체스 스크립트에는 바꿔치기(캐시 비우기 포함)와 스타일 덮어쓰기가 둘 다 통할 조건이다. 실제 바꿔치기는 5단계에서 확인한다.
- 대안: 바꿔치기가 있는 스크립트를 켜거나 고칠 때 디스크 캐시를 비운다. 단, 이 API는 프로필 전체의 캐시를 비운다. 서비스 워커를 쓰는 사이트에는 spec대로 스타일 덮어쓰기를 쓴다.
- 덧붙임(10단계, WebView2 157): 광고 차단 시험에서는 서비스 워커의 `fetch()` 요청이 `AddWebResourceRequestedFilterWithRequestSourceKinds(ALL)` 필터에 걸려 막혔다(`docs/S10-adblock.md` 3장). 리소스 바꿔치기로는 다시 확인하지 않았다.

### 10. 콘텐츠 웹뷰에서 앱 명령 — 됨

조건: `build.rs`에서 `AppManifest::commands`로 앱 명령에 권한을 요구하고, capability 범위를 `"windows"`가 아닌 `"webviews": ["ui"]`로 잡았다. (`windows`로 잡으면 같은 창의 콘텐츠 웹뷰도 포함된다.)

| 콘텐츠 웹뷰의 페이지 | 앱 명령 `s0_ping`, `show_tab` | 기본 명령 `app.version`, `window.is_maximized` | `ipc.localhost`로 직접 fetch |
| --- | --- | --- | --- |
| https://example.com | 거부 | 거부 | 실패 |
| http://127.0.0.1:8765 (로컬) | 거부 | 거부 | 실패 |
| http://tauri.localhost (앱 주소로 이동) | 거부 | 거부 | 실패 |
| UI 웹뷰 (대조군) | **허용** | — | — |

- 호스트 기록에도 `s0_ping`을 부른 것은 UI 웹뷰뿐이었다.
- 콘텐츠 페이지에서도 `window.__TAURI_INTERNALS__`와 `window.__TAURI__` 객체는 보인다. 명령은 막히지만 Tauri 앱이라는 것은 드러난다. 본 구현에서는 `withGlobalTauri`를 끈다. 이때 `__TAURI_INTERNALS__`가 남는지는 확인하지 않았다.
- 콘텐츠 웹뷰가 `http://tauri.localhost`(내부 UI)로 이동할 수 있었다. 명령은 막히지만, 본 구현에서는 `on_navigation`으로 콘텐츠 웹뷰의 앱 주소 이동을 막는다.

### 11. 비밀번호 저장·자동 입력 — 됨

- `ICoreWebView2Settings4`의 `IsPasswordAutosaveEnabled`와 `IsGeneralAutofillEnabled`를 켰다.
- 로컬 로그인 페이지에 실제 키 입력으로 시험용 아이디·비밀번호를 넣고 제출하니 Edge와 같은 "암호를 저장하시겠습니까?" 창이 한국어로 떴다 (`shot-password-after-submit.png`).
- 저장한 뒤 다시 열자 두 칸이 자동으로 채워졌고 "저장된 암호" 목록이 떴다 (`shot-password-autofill.png`).
- 설정은 웹뷰마다 따로다. 콘텐츠 웹뷰를 만들 때마다 켠다.

### 12. DRM — 됨 (넷플릭스는 사용자 확인 필요)

- 지원하는 키 시스템: `com.widevine.alpha`, `com.microsoft.playready`, `com.microsoft.playready.recommendation`, `...recommendation.3000`(하드웨어), `org.w3.clearkey`.
- Shaka Player 공개 시험 영상(Widevine 암호화, 구글 시험 라이선스 서버)을 로컬 페이지에서 틀었다. 라이선스를 받고 재생 시간이 7.8초까지 진행됐다(영상 폭 768px).
- 화면 캡처에는 영상이 검게 찍혔다 (`shot-drm-widevine.png`). 보호된 영상이 캡처에서 가려지는 것인지, 실제 화면도 검은지는 확인하지 못했다.
- 넷플릭스는 계정이 필요해 시험하지 않았다.

### 13. 압축 해제 확장 — 됨

- 모든 웹뷰에 `browser_extensions_enabled(true)`를 주고, 프로필의 `AddBrowserExtension`에 시험 확장 폴더(`testext/`, MV3 콘텐츠 스크립트)를 넘겼다. 확장 ID가 돌아왔고, 콘텐츠 스크립트가 로컬 페이지에서 실행됐다.
- Tauri의 `extensions_path`는 등록 실패를 무시한다. 본 구현에서는 `AddBrowserExtension`을 직접 불러 결과를 확인한다.
- 확장 아이콘과 팝업은 확인하지 않았다. spec 6.6대로 뜨지 않는 것으로 본다.

## spec에 반영할 것 (사용자 확인 후)

0단계 결과로 바뀌는 구현 방식이다. 2장의 결정은 바뀌지 않는다. 고쳐도 되면 spec을 먼저 고치고 1단계로 간다.

1. **2장 앱 틀**: Tauri 2 + `unstable` 확정. 버전은 위 "고정한 버전"으로 고정한다.
2. **3장 통신·보안 경계**: 앱 명령은 `AppManifest::commands`로 권한을 요구하고, capability는 `webviews` 범위로 UI 웹뷰와 내부 페이지에만 준다. 콘텐츠 웹뷰는 `on_navigation`으로 앱 주소 이동을 막는다.
3. **3장 배치**: 모든 웹뷰는 `with_environment`로 환경 객체 하나를 같이 쓴다. 웹뷰를 다 만든 뒤 장식 끄기를 한 번 더 불러 위쪽 변 크기 조절을 켠다. 최대화 버튼 자리에 스냅 레이아웃용 자식 창을 둔다.
4. **3장 미리 알려진 어려움 3번(팝업)**: 주소창 자동완성과 메뉴는 활성화 없이 띄우는 투명 팝업 창으로 만든다.
5. **6.2 리소스 바꿔치기**: 바꿔치기가 있는 스크립트를 켜거나 고치면 디스크 캐시를 비운다. 서비스 워커 사이트는 스타일 덮어쓰기를 쓴다.
6. **7장 버림**: 웹뷰를 닫아도 렌더러 메모리는 20~30초 뒤에 풀린다. 측정할 때 이 시간을 둔다.
7. **3장 미리 알려진 어려움 2번(둥근 모서리)**: 창 영역 잘라내기로 12px 모서리를 낸다. 가장자리 계단 현상은 받아들인다. 대체안은 각진 모서리이고, 설정 하나로 바꿀 수 있게 한다.

## 사용자가 해 볼 시험

1. 프로젝트 폴더(`TBs Browser`)에서 시험 앱을 실행한다. Windows PowerShell 5.1에서도 그대로 된다. 처음 빌드는 몇 분 걸린다.

   ```bash
   cargo +stable-x86_64-pc-windows-msvc run --manifest-path spikes/s0/Cargo.toml
   ```

2. 왼쪽 "탭 1", "탭 2"를 번갈아 눌러 example.com과 위키백과가 번갈아 보이는지 본다. (9장의 0단계 시험)
3. 창 틀: 툴바 빈 곳을 끌어 옮기기, 더블클릭, 네 변 끌기, 최대화 버튼에 마우스 올려 스냅 메뉴 보기.
4. 페이지 안을 누르고 Ctrl+T, Ctrl+W 등을 누르면 왼쪽 기록에 호스트가 받은 키가 찍힌다.
5. 넷플릭스(또는 다른 DRM 서비스)는 이 앱 c1 탭에서 로그인해 영상이 화면에 보이는지 확인해야 한다. 시험 앱에는 주소창이 없다. 필요하면 다음 단계 전에 주소 입력을 붙여 주겠다.

자동 시험을 다시 돌리려면 `testsite/server.py`를 먼저 띄우고 `S0_AUTORUN=1`을 주어 실행한다. 결과는 `spikes/s0/out/report.jsonl`에 쌓인다.

## 남은 문제

- **둥근 모서리의 계단 현상 — 결정됨 (Nov 10, 2026)**: 창 영역 잘라내기(A)로 간다. 거슬리면 각진 모서리(C)로 바꾼다. 비주얼 호스팅(B)은 하지 않는다. 모서리 처리는 한 곳에서 켜고 끌 수 있게 만들어 C로 바로 바꿀 수 있게 한다.
- **DRM 화면 표시**: 사용자가 넷플릭스를 구독하지 않는다. 화면에 보이는지는 무료로 Widevine을 쓰는 Spotify 웹 플레이어나 시험 영상 페이지로 확인한다.
- **확인하지 못한 것**: 비주얼 호스팅, 배율 125%·150%, 넷플릭스 실제 재생과 DRM 영상의 화면 표시, 팝업 창이 메인 창 이동을 따라가는지, `withGlobalTauri`를 껐을 때 콘텐츠 페이지에 남는 객체, 서비스 워커 요청을 가로챌 다른 방법.
- **chess.com 실제 바꿔치기**: 조건은 맞지만 실제로 기물을 바꿔 보지는 않았다. 5단계에서 확인한다.
