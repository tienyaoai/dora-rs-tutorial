#!/usr/bin/env python3
"""Add navigation, TOC, and repo-relative paths to lesson markdown files."""
from __future__ import annotations

import re
from pathlib import Path

LESSONS_DIR = Path(__file__).resolve().parents[1] / "docs" / "lessons"

TITLES = {
    1: "dora-rs 是什麼？跟 ROS 2 差在哪",
    2: "安裝 dora CLI，跑官方 rust-dataflow 範例",
    3: "逐行讀懂 dataflow.yml",
    4: "第一個像樣的 Rust 節點——`DoraNode` 與事件迴圈",
    5: "Arrow 資料與 metadata——除了純量，還能送什麼？",
    6: "`dora up / start / logs / down`——資料流的生命週期管理",
}


def nav_line(n: int) -> str:
    parts = ["**導覽：** [返回教材總覽](../../README.md)"]
    if n > 1:
        parts.append(f"[← 第 {n - 1} 篇](lesson-{n - 1:02d}.md)")
    if n < 6:
        parts.append(f"[第 {n + 1} 篇 →](lesson-{n + 1:02d}.md)")
    else:
        parts.append("[第 7 篇（尚未發布）](../../README.md#課程大綱20-篇規劃)")
    if n == 2:
        parts.append(
            "**範例：** [取得官方 rust-dataflow](../../examples/lesson-02/README.md)"
        )
    else:
        parts.append(f"**範例：** [examples/lesson-{n:02d}/](../../examples/lesson-{n:02d}/)")
    return " · ".join(parts) + "\n"


def build_toc(text: str) -> str:
    headings = re.findall(r"^## (.+)$", text, re.MULTILINE)
    if not headings:
        return ""
    lines = ["## 目錄", ""]
    for h in headings:
        if h == "目錄":
            continue
        anchor = re.sub(r"[^\w\u4e00-\u9fff\-` ]", "", h)
        anchor = anchor.replace("`", "").strip().replace(" ", "-").lower()
        anchor = re.sub(r"-+", "-", anchor)
        lines.append(f"- [{h}](#{anchor})")
    lines.append("")
    return "\n".join(lines)


def patch_lesson(n: int, path: Path) -> None:
    text = path.read_text(encoding="utf-8")
    # Project tree paths
    text = text.replace(f"lesson-{n:02d}/", f"examples/lesson-{n:02d}/")
    # cd commands from repo root
    text = re.sub(
        rf"^cd lesson-{n:02d}\s*$",
        f"cd examples/lesson-{n:02d}   # 在 clone 本 repo 後，於 repo 根目錄執行",
        text,
        flags=re.MULTILINE,
    )
    # Remove broken log reference
    text = text.replace(
        "；完整指令抄本見同目錄 `run-lifecycle.log`。",
        "。（完整指令抄本未納入本 repo；僅保留上文節錄。）",
    )
    # Footer next lesson links
    footers = {
        1: "下一篇：[第 2 篇：安裝 dora CLI，跑官方 rust-dataflow 範例](lesson-02.md)。",
        2: "下一篇：[第 3 篇：逐行讀懂 dataflow.yml](lesson-03.md)。",
        3: "下一篇：[第 4 篇：第一個像樣的 Rust 節點——`DoraNode` 與事件迴圈](lesson-04.md)。",
        4: "下一篇：[第 5 篇：Arrow 資料與 metadata——除了純量，還能送什麼？](lesson-05.md)。",
        5: "下一篇：[第 6 篇：`dora up / start / logs / down`——資料流的生命週期管理](lesson-06.md)。",
        6: "下一篇：第 7 篇尚未發布；佇列與背壓實測預計在第 8 篇登場（見 [課程大綱](../../README.md#課程大綱20-篇規劃)）。",
    }
    for old in re.findall(r"^下一篇：.+$", text, re.MULTILINE):
        text = text.replace(old, footers[n])

    lines = text.splitlines(keepends=True)
    if not lines:
        return
    # Insert nav + toc after first heading block (after blank line following # title)
    out: list[str] = []
    i = 0
    out.append(lines[0])
    i = 1
    while i < len(lines) and lines[i].strip() == "":
        out.append(lines[i])
        i += 1
    # skip blockquote version line if present
    while i < len(lines) and lines[i].startswith(">"):
        out.append(lines[i])
        i += 1
    if i < len(lines) and lines[i].strip() == "":
        out.append(lines[i])
        i += 1

    body = "".join(lines[i:])
    out.append("\n")
    out.append(nav_line(n))
    if n == 2:
        out.append(
            "\n> **第 2 課範例說明：** 本 repo **沒有**可獨立 `cargo build` 的 `lesson-02` 專案。"
            "請依下文在 **dora-rs 官方 monorepo**（tag `v1.1.0`）內編譯 `examples/rust-dataflow/`。"
            "步驟摘要見 [examples/lesson-02/README.md](../../examples/lesson-02/README.md)。\n\n"
        )
    toc = build_toc(body)
    if toc:
        out.append(toc)
    out.append(body)
    out.append("\n---\n\n")
    out.append(nav_line(n))
    path.write_text("".join(out), encoding="utf-8")


def main() -> None:
    for n in range(1, 7):
        patch_lesson(n, LESSONS_DIR / f"lesson-{n:02d}.md")


if __name__ == "__main__":
    main()
