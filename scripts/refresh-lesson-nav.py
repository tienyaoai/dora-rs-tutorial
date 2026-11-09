#!/usr/bin/env python3
"""Refresh lesson navigation blocks and TOC anchors (layout pass)."""
from __future__ import annotations

import re
from pathlib import Path

import importlib.util

_spec = importlib.util.spec_from_file_location(
    "check", Path(__file__).resolve().parent / "check-markdown-nav.py"
)
_check = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_check)
github_heading_slug = _check.github_heading_slug

LESSONS = Path(__file__).resolve().parents[1] / "docs" / "lessons"

NEXT_HREF = {
    1: "lesson-02.md",
    2: "lesson-03.md",
    3: "lesson-04.md",
    4: "lesson-05.md",
    5: "lesson-06.md",
    6: "lesson-07.md",
    7: "lesson-08.md",
    8: "lesson-09.md",
}


def nav_block(n: int) -> str:
    lines = ["**導覽**", "", "- [返回教材總覽](../../README.md)"]
    if n > 1:
        lines.append(f"- 上一課：[第 {n - 1} 篇](lesson-{n - 1:02d}.md)")
    if n < 9:
        lines.append(f"- 下一課：[第 {n + 1} 篇 →]({NEXT_HREF[n]})")
    else:
        lines.append(
            "- 下一課：第 10 篇**尚未發布**（僅規劃；零拷貝大資料等見"
            " [課程大綱](../../README.md#課程大綱20-篇規劃)）"
        )
    if n == 2:
        lines.append(
            "- 本課範例：[取得官方 rust-dataflow](../../examples/lesson-02/README.md)"
        )
    else:
        lines.append(
            f"- 本課範例：[examples/lesson-{n:02d}/](../../examples/lesson-{n:02d}/)"
        )
    lines.append("")
    return "\n".join(lines)


def build_toc(body: str) -> str:
    headings = re.findall(r"^## (.+)$", body, re.MULTILINE)
    lines = ["## 目錄", ""]
    for h in headings:
        if h == "目錄":
            continue
        slug = github_heading_slug(h)
        lines.append(f"- [{h}](#{slug})")
    lines.append("")
    return "\n".join(lines)


def extract_body(text: str) -> str:
    m = re.search(r"^## 重點說明", text, re.MULTILINE)
    if not m:
        raise ValueError("missing ## 重點說明")
    body = text[m.start() :]
    body = re.sub(r"\n\*\*導覽：\*\*[^\n]+\n", "", body)
    body = re.sub(r"\n---\n[\s\S]*$", "", body)
    return body.rstrip() + "\n"


LESSON2_NOTE = (
    "> **第 2 課範例說明：** 本 repo **沒有**可獨立 `cargo build` 的 `lesson-02` 專案。"
    "請依下文在 **dora-rs 官方 monorepo**（tag `v1.1.0`）內編譯 `examples/rust-dataflow/`。"
    "步驟摘要見 [examples/lesson-02/README.md](../../examples/lesson-02/README.md)。\n\n"
)


def extract_header(text: str, n: int) -> str:
    m = re.search(r"^## ", text, re.MULTILINE)
    if not m:
        raise ValueError("missing ## section")
    pre = text[: m.start()]
    pre = re.sub(r"^\*\*導覽：\*\*[^\n]*\n", "", pre, flags=re.MULTILINE)
    pre = re.sub(
        r"^\*\*導覽\*\*\n(?:- .+\n)*\n?",
        "",
        pre,
        flags=re.MULTILINE,
    )
    pre = re.sub(
        r"^- \[返回教材總覽\][^\n]*\n(?:- .+\n)*\n?",
        "",
        pre,
        flags=re.MULTILINE,
    )
    if n == 2 and "第 2 課範例說明" not in pre:
        pre = pre.rstrip() + "\n\n" + LESSON2_NOTE
    return pre


def refresh_lesson(path: Path, n: int) -> None:
    text = path.read_text(encoding="utf-8")
    header = extract_header(text, n)
    body = extract_body(text)
    toc = build_toc(body)
    out = header + nav_block(n) + "\n" + toc + body + "\n---\n\n" + nav_block(n)
    path.write_text(out, encoding="utf-8")


def main() -> None:
    for n in range(1, 10):
        refresh_lesson(LESSONS / f"lesson-{n:02d}.md", n)


if __name__ == "__main__":
    main()
