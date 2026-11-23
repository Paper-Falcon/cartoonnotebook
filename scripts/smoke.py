"""接口冒烟测试：起一个独立端口的服务，跑一整套断言，失败以非零退出。

用法：
  1) 先编译产物（npm run app:build 或 cargo build --release --bin cartoonnotebook）
  2) python scripts/smoke.py
  3) 可用环境变量 SMOKE_PORT 改端口（默认 17878）

跑完自动清理临时数据目录。
"""

import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PORT = int(os.environ.get("SMOKE_PORT", "17878"))
BASE = f"http://127.0.0.1:{PORT}"

EXE_CANDIDATES = [
    os.path.join(ROOT, "src-tauri", "target", "release", "cartoonnotebook.exe"),
    os.path.join(ROOT, "src-tauri", "target", "debug", "devserver.exe"),
]

results = []


def check(name, ok, extra=""):
    results.append((name, ok, extra))
    print(("PASS  " if ok else "FAIL  ") + name + (f"   [{extra}]" if not ok else ""))


def req(path, method="GET", body=None, ctype="application/json", raw=False):
    data = None
    if isinstance(body, (dict, list)):
        data = json.dumps(body).encode("utf-8")
    elif isinstance(body, bytes):
        data = body
    r = urllib.request.Request(BASE + path, method=method, data=data)
    r.add_header("Content-Type", ctype)
    try:
        resp = urllib.request.urlopen(r, timeout=25)
        code, payload, ct = resp.status, resp.read(), resp.headers.get("Content-Type", "")
    except urllib.error.HTTPError as e:
        code, payload, ct = e.code, e.read(), e.headers.get("Content-Type", "")
    if raw:
        return code, payload, ct
    try:
        return code, json.loads(payload.decode("utf-8")), ct
    except Exception:
        return code, payload, ct


def multipart(fields, files):
    boundary = "----smoke" + str(int(time.time() * 1000))
    out = bytearray()
    for k, v in fields.items():
        out += (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'
        ).encode()
    for k, (filename, blob, mtype) in files.items():
        out += (
            f"--{boundary}\r\n"
            f'Content-Disposition: form-data; name="{k}"; filename="{filename}"\r\n'
            f"Content-Type: {mtype}\r\n\r\n"
        ).encode() + blob + b"\r\n"
    out += f"--{boundary}--\r\n".encode()
    return f"multipart/form-data; boundary={boundary}", bytes(out)


def tiny_png():
    # 1x1 PNG，不用依赖 Pillow
    base = (
        b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01"
        b"\x08\x02\x00\x00\x00\x90wS\xde\x00\x00\x00\x0cIDATx\x9cc\xf8\xcf\xc0"
        b"\x00\x00\x00\x03\x00\x01\x1a\xef,\x1d\x00\x00\x00\x00IEND\xaeB`\x82"
    )
    return base


def main():
    exe = next((p for p in EXE_CANDIDATES if os.path.isfile(p)), None)
    if not exe:
        print("没找到可执行文件，先跑 npm run app:build 或 cargo build --release --bin cartoonnotebook")
        return 2

    tmp = tempfile.mkdtemp(prefix="cn-smoke-")
    env = {**os.environ, "CARTOON_DATA_DIR": tmp, "CARTOON_PORT": str(PORT)}
    proc = subprocess.Popen([exe], cwd=os.path.dirname(exe), env=env)

    png = tiny_png()
    tmpfiles = []

    try:
        # 等服务起来
        for _ in range(30):
            try:
                code, body, _ = req("/api/health")
                if code == 200:
                    break
            except Exception:
                pass
            time.sleep(0.5)
        else:
            print(f"服务 {BASE} 没起来，端口可能被占用？可用 SMOKE_PORT 换端口")
            return 2

        print("== 基础 ==")
        code, body, _ = req("/api/health")
        check("健康检查", code == 200 and body.get("ok") is True)

        code, body, _ = req("/api/index")
        check("标题索引", code == 200 and isinstance(body, dict)
              and len(body.get("items", [])) > 1000,
              f"{code} {list(body) if isinstance(body, dict) else body}")

        # 未知的 /api 路径不能让 SPA 兜底成 HTML
        code, body, ct = req("/api/does-not-exist", raw=True)
        check("未知接口返回 JSON 404", code == 404 and b"error" in body, f"{code} {ct}")

        code, body, ct = req("/some-page", raw=True)
        check("未知页面走 SPA 兜底", code == 200 and b"html" in body, f"{code} {ct}")

        print("== 作品 ==")
        code, a, _ = req("/api/works", "POST", {"anilistId": 900001, "titleCn": "冒烟A"})
        check("创建 A", code == 200 and a["titleCn"] == "冒烟A")

        code, b, _ = req("/api/works", "POST", {"titleCn": "冒烟B"})
        check("创建 B（无 anilistId）", code == 200 and b["id"] != a["id"])

        code, w, _ = req(f"/api/works/{a['id']}", "PATCH", {
            "status": "done", "rating": 9, "progress": 12,
            "finishedAt": "", "note": "短评", "synopsis": "中文简介",
        })
        check("PATCH 全字段回显", code == 200 and w["rating"] == 9
              and w["finishedAt"] and w["synopsis"] == "中文简介", json.dumps(w)[:120])

        code, rows, _ = req("/api/works")
        check("列表 2 条", code == 200 and len(rows) == 2, str(len(rows)))

        print("== 标签 ==")
        req(f"/api/works/{a['id']}/tags", "POST", {"name": "神作"})
        req(f"/api/works/{a['id']}/tags", "POST", {"name": "热血"})
        req(f"/api/works/{b['id']}/tags", "POST", {"name": "神作"})
        code, tags, _ = req("/api/tags")
        zushuo = next((t for t in tags if t["name"] == "神作"), None)
        check("标签计数", zushuo is not None and zushuo["count"] == 2, str(tags))

        code, rows, _ = req("/api/works")
        check("作品带 tags", all(("tags" in w) for w in rows))

        code, b2, _ = req("/api/works")
        bid = [w for w in b2 if w["titleCn"] == "冒烟B"][0]["id"]
        tid = zushuo["id"]
        code, after, _ = req(f"/api/works/{bid}/tags/{tid}", "DELETE")
        check("移除标签", code == 200 and after == [], str(after))

        print("== 素材 ==")
        ctype, payload = multipart(
            {"kind": "video", "duration": "300"},
            {"file": ("fake.mp4", b"\x00" * 8192, "video/mp4")},
        )
        code, m, _ = req(f"/api/works/{a['id']}/media/upload", "POST", payload, ctype)
        check("上传视频素材", code == 200 and m["kind"] == "video"
              and m["duration"] == 300.0 and m["position"] == 0.0, json.dumps(m)[:120])
        mid = m["id"]

        code, m, _ = req(f"/api/media/{mid}", "PATCH", {"position": 123.5})
        check("记住播放位置", code == 200 and abs(m["position"] - 123.5) < 0.01, str(m["position"]))

        code, m, _ = req(f"/api/media/{mid}", "PATCH", {"episode": 2})
        check("设置集号", code == 200 and m["episode"] == 2, str(m["episode"]))

        code, m, _ = req(f"/api/media/{mid}", "PATCH", {"path": "E:/x/other.jpg"})
        check("归档素材拒绝改路径", code == 400 and "引用" in m.get("error", ""), f"{code} {m}")

        ref_file1 = os.path.join(tmp, "ref-src1.jpg")
        ref_file2 = os.path.join(tmp, "ref-src2.jpg")
        open(ref_file1, "wb").write(png)
        open(ref_file2, "wb").write(png)
        tmpfiles += [ref_file1]

        code, m, _ = req(f"/api/works/{a['id']}/media/import", "POST",
                         {"path": ref_file1.replace("\\", "/")})
        check("引用模式导入", code == 200 and m["storage"] == "ref", json.dumps(m)[:120])
        rid = m["id"]

        os.remove(ref_file1)
        code, body, _ = req(f"/api/media/{rid}/file", raw=True)
        check("文件挪走后 404", code == 404, str(code))
        code, rows, _ = req(f"/api/works/{a['id']}/media")
        lost = next((x for x in rows if x["id"] == rid), None)
        check("missing 标记", lost is not None and lost["missing"] == 1, str(lost))

        code, m, _ = req(f"/api/media/{rid}", "PATCH", {"path": ref_file2.replace("\\", "/")})
        check("重新定位", code == 200 and m["missing"] == 0 and m["path"].endswith("ref-src2.jpg"),
              json.dumps(m)[:120])

        code, body, _ = req(f"/api/media/{rid}/file", raw=True)
        check("重定位后可读", code == 200 and body == png, str(code))

        print("== 导出 ==")
        code, exp, _ = req("/api/export")
        check("生成导出包", code == 200 and exp["bytes"] > 0, json.dumps(exp)[:120])
        code, payload, ct = req("/api/export/download", raw=True)
        ok = False
        if code == 200 and payload[:2] == b"PK":
            z = zipfile.ZipFile(__import__("io").BytesIO(payload))
            ok = {"manifest.json", "library.db"} <= set(z.namelist())
        check("下载并检查 zip", ok, f"{code} {ct}")

        # 用刚下载的备份走一遍恢复：包先就位，重启后生效
        ctype, payload = multipart({}, {"file": ("backup.zip", payload, "application/zip")})
        code, body, _ = req("/api/restore", "POST", payload, ctype)
        check("上传恢复包", code == 200 and body.get("requiresRestart") is True, f"{code} {body}")
        check("待恢复目录就位",
              os.path.isfile(os.path.join(tmp, ".restore-pending", "library.db")))
        code, body, _ = req("/api/restore", "POST", b"not a zip", "application/zip")
        check("坏包被拒绝", code == 400, f"{code} {body}")

        print("== 收尾 ==")
        for wid in (a["id"], b["id"]):
            code, _, _ = req(f"/api/works/{wid}", "DELETE")
            check(f"删除作品 {wid}", code == 200)
        code, rows, _ = req("/api/works")
        check("删完为空", code == 200 and rows == [], str(rows))

    except Exception as e:
        check("流程中断", False, repr(e))
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()
        for f in tmpfiles:
            try:
                os.remove(f)
            except OSError:
                pass
        shutil.rmtree(tmp, ignore_errors=True)

    failed = [r for r in results if not r[1]]
    print(f"\n共 {len(results)} 项断言，失败 {len(failed)}")
    for name, _, extra in failed:
        print("  FAIL", name, extra)
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
