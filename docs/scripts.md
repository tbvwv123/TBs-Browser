# 사이트 스크립트 만들기

TB's Browser의 사이트 스크립트 형식이다 (spec 6장). 예시는 `scripts-examples/`에 있다.

## 폴더

```
scripts\<id>\
  script.json
  main.js, style.css, 그림 파일 …
  settings.json   설정 값 (브라우저가 쓴다)
  storage.json    저장 공간 (브라우저가 쓴다)
```

`id`는 영어 소문자·숫자·`-`·`_`.

## script.json

```json
{
  "id": "pip",
  "name": "어떤 영상이든 PiP",
  "version": "1.0.0",
  "description": "영상을 PiP로 띄우기",
  "author": "TB's Browser",
  "icon": "icon.svg",
  "matches": ["https://*", "http://*"],
  "run_at": "document_end",
  "js": ["main.js"],
  "css": [],
  "resource_overrides": [],
  "permissions": [],
  "shortcuts": [{ "id": "pip", "name": "PiP 켜고 끄기", "keys": "Alt+P" }],
  "actions": [{ "id": "pip", "name": "PiP" }],
  "settings": [{ "key": "hoverButton", "type": "toggle", "label": "영상 위에 PiP 버튼 보이기", "default": true }],
  "enabled": false,
  "pinned": false
}
```

| 항목 | 값 |
| --- | --- |
| `matches` | 주소 패턴. `*`는 아무 글자. 모든 사이트는 `https://*`, `http://*` |
| `run_at` | `document_start` 또는 `document_end` |
| `js`, `css` | 폴더 안 파일. 적힌 순서대로 |
| `css` 안의 `url("tbs-file:그림.png")` | 폴더 안 그림을 데이터 URL로 바꿔 넣는다 |
| `resource_overrides` | `{ "pattern", "file" }`. 페이지가 받는 파일을 폴더 안 파일로 바꾼다 |
| `permissions` | `screenshot`, `saveFile`, `copyText`, `tabs`, `storage` |
| `icon` | 폴더 안 PNG·SVG (256KB까지). 없으면 이름 첫 글자 |
| `shortcuts` | `keys`: `Alt+P`, `Ctrl+Shift+Y`, `F9`. 글자·숫자 키에는 Ctrl이나 Alt. 브라우저 단축키와 겹치면 오류 |
| `actions` | 확장 패널의 "이 탭에서 실행" 버튼 |
| `settings` | 아래 표 |

### settings 칸

| 항목 | 값 |
| --- | --- |
| `key` | 이름 (겹치면 오류) |
| `type` | `toggle`, `number`, `text`, `list`, `select` |
| `label` | 화면에 보일 이름 |
| `default` | 기본값 |
| `options` | `select`: `[{ "value", "label" }]` |
| `section` | 묶을 작은 제목. 같은 제목이 이어지면 한 구역 |
| `unit`, `min`, `max` | `number` |
| `icon` | 줄 앞 한 색 아이콘 (글자색으로 칠한다) |
| `placeholder`, `description` | 칸 안 글자, 이름 아래 작은 글자 |

`list`는 구역 하나를 따로 차지한다.

## tbs

스크립트 코드 안에서 쓴다. 부탁은 맨 위 문서에서만 된다.

| 이름 | 권한 | 하는 일 |
| --- | --- | --- |
| `tbs.settings` | | 지금 설정 값 |
| `tbs.onSettingsChanged(f)` | | 값이 바뀌면 `f(값)` |
| `tbs.setSetting(key, value)` | `storage` | 자기 설정 값을 바꾼다 |
| `tbs.onShortcut(id, f)` | | 단축키를 누르면 `f()` (사용자 동작으로 부른다) |
| `tbs.onAction(id, f)` | | "이 탭에서 실행"을 누르면 `f()` |
| `tbs.tabs(action)` | `tabs` | `back`, `forward`, `reload`, `close`, `newTab`, `reopen`, `next`, `prev`, `home`, `fullscreen`, `maximize`, `minimize` |
| `tbs.storage.get(key)` / `set(key, value)` / `remove(key)` | `storage` | `storage.json` (5MB까지). `get()`이면 전부 |
| `tbs.request("screenshot")` | `screenshot` | 탭 화면을 PNG로 다운로드 폴더에 |
| `tbs.request("saveFile", { name, text \| base64 })` | `saveFile` | 다운로드 폴더에 저장 |
| `tbs.request("copyText", { text })` | `copyText` | 클립보드 |

모두 Promise를 돌려준다. 거절되면 오류 메시지가 온다.

## 주의

- 페이지와 같은 공간에서 돈다. 페이지도 같은 메시지를 보낼 수 있다고 보고 브라우저가 모든 부탁을 검사한다.
- Trusted Types를 쓰는 사이트(유튜브 등)에서는 `innerHTML`을 쓸 수 없다. 요소를 직접 만든다.
- 화면만 바꾸는 사이트(유튜브 등)에서는 화면 변화를 지켜보다가 다시 붙인다.

## 저장소에 올리기

저장소: [tbvwv123/TBs-Browser-scripts](https://github.com/tbvwv123/TBs-Browser-scripts)

1. `scripts-examples/<id>/`를 고치고 `version`을 올린다.
2. 저장소를 받아 둔 폴더에 `python tools/store-build.py <폴더>`. `scripts/`, `dist/<id>.zip`, `index.json`이 바뀐다.
3. 그 폴더에서 커밋하고 올린다.

브라우저는 하루에 한 번 `index.json`을 받아 새 판이 있으면 확장 패널에 "업데이트 있음"을 띄운다. 권한이 늘었으면 다시 묻는다. 설정 값과 저장 공간, 켜짐·고정은 그대로 두고 옛 폴더는 휴지통으로 옮긴다.
