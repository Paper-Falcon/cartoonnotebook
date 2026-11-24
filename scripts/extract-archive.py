"""从 Bangumi Archive dump zip 里只提取 subject.jsonlines（流式，内存友好）"""
import sys, zipfile

if len(sys.argv) != 3:
    print("用法: extract-archive.py <dump.zip> <输出.jsonl>")
    sys.exit(1)

zip_path, out_path = sys.argv[1], sys.argv[2]
import json as _json

with zipfile.ZipFile(zip_path) as z:
    names = [n for n in z.namelist() if n.endswith("subject.jsonlines")]
    if not names:
        print("zip 里没有 subject.jsonlines，现有文件:", z.namelist()[:20])
        sys.exit(2)
    kept = 0
    seen = 0
    with z.open(names[0]) as src, open(out_path, "wb") as dst:
        for raw in src:
            seen += 1
            try:
                s = _json.loads(raw)
            except Exception:
                continue
            if s.get("type") != 2:
                continue
            # meta_tags 里的「中国」标签是 bgm.tv 的权威国别信号
            tags = s.get("meta_tags") or []
            if not isinstance(tags, list) or "中国" not in tags:
                continue
            dst.write(raw)
            kept += 1
    print(f"扫描 {seen} 条，动画 {kept} 条国漫已过滤写入")
print("提取完成:", out_path)
