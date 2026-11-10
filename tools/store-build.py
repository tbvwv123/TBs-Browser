"""스크립트 저장소(tbvwv123/TBs-Browser-scripts)의 내용을 만든다 (spec 6.8).

쓰는 법 (저장소 맨 위 폴더에서):
    python tools/store-build.py <저장소를 받아 둔 폴더>

하는 일:
    1. scripts-examples/의 스크립트를 <폴더>/scripts/<id>/로 복사한다 (원본)
    2. 스크립트마다 <폴더>/dist/<id>.zip을 만든다 (zip 맨 위에 script.json)
    3. <폴더>/index.json을 만든다. 앱은 이 목록과 zip만 받는다

올리기 전에 버전을 올려야 이미 설치한 사람에게 업데이트가 간다.
"""

import base64
import json
import shutil
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "scripts-examples"
MIME = {".svg": "image/svg+xml", ".png": "image/png"}
# 사용자 값은 넣지 않는다
SKIP = {"settings.json", "storage.json", "settings.json.bak"}


def files_of(d: Path):
    return sorted(p for p in d.rglob("*") if p.is_file() and p.name not in SKIP)


def build(out: Path):
    (out / "scripts").mkdir(parents=True, exist_ok=True)
    (out / "dist").mkdir(parents=True, exist_ok=True)
    entries = []
    for d in sorted(p for p in SOURCE.iterdir() if (p / "script.json").exists()):
        m = json.loads((d / "script.json").read_text(encoding="utf-8"))
        sid = m.get("id") or d.name
        dst = out / "scripts" / sid
        if dst.exists():
            shutil.rmtree(dst)
        shutil.copytree(d, dst, ignore=shutil.ignore_patterns(*SKIP))
        z = out / "dist" / f"{sid}.zip"
        with zipfile.ZipFile(z, "w", zipfile.ZIP_DEFLATED) as zf:
            for f in files_of(d):
                info = zipfile.ZipInfo(f.relative_to(d).as_posix(), date_time=(2026, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                zf.writestr(info, f.read_bytes())
        icon = None
        if m.get("icon"):
            p = d / m["icon"]
            icon = f"data:{MIME.get(p.suffix.lower(), 'application/octet-stream')};base64,{base64.b64encode(p.read_bytes()).decode()}"
        entries.append({
            "id": sid,
            "name": m.get("name", sid),
            "version": m.get("version", "0"),
            "description": m.get("description", ""),
            "author": m.get("author", ""),
            "matches": m.get("matches", []),
            "permissions": m.get("permissions", []),
            "icon": icon,
            "zip": f"dist/{sid}.zip",
            "size": z.stat().st_size,
        })
        print(f"{sid} {m.get('version')} {z.stat().st_size} bytes")
    entries.sort(key=lambda e: e["name"])
    (out / "index.json").write_text(json.dumps({"version": 1, "scripts": entries}, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    build(Path(sys.argv[1]))
