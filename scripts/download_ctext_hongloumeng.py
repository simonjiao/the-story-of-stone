#!/usr/bin/env python3
"""Download and structure ctext.org Hongloumeng chapters.

The script intentionally uses one request at a time and a configurable delay.
ctext.org asks users not to bulk-download large numbers of pages; keeping raw
HTML cached locally avoids repeated requests when re-running parsers.
"""

from __future__ import annotations

import argparse
import datetime as dt
import html
from html.parser import HTMLParser
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any


BASE_URL = "https://ctext.org/"
START_URL = "https://ctext.org/hongloumeng/zhs"
USER_AGENT = (
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/124.0 Safari/537.36"
)


class CaptchaError(RuntimeError):
    pass


class TextExtractor(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.parts: list[str] = []
        self.skip_depth = 0

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag in {"script", "style"}:
            self.skip_depth += 1
            return
        if self.skip_depth:
            return
        if tag == "br":
            self.parts.append("\n")

    def handle_endtag(self, tag: str) -> None:
        if tag in {"script", "style"} and self.skip_depth:
            self.skip_depth -= 1

    def handle_data(self, data: str) -> None:
        if not self.skip_depth:
            self.parts.append(data)


def html_to_text(fragment: str) -> str:
    fragment = re.sub(r"(?is)<br\s*/?>", "\n", fragment)
    parser = TextExtractor()
    parser.feed(fragment)
    text = "".join(parser.parts)
    text = text.replace("\xa0", " ").replace("\u3000", " ")
    text = re.sub(r"[ \t\r\f\v]+", " ", text)
    lines = [line.strip() for line in text.splitlines()]
    text = "\n".join(line for line in lines if line)
    text = re.sub(r"\n{3,}", "\n\n", text)
    return html.unescape(text).strip()


def normalize_url(url: str) -> str:
    url = html.unescape(url)
    return urllib.parse.urljoin(BASE_URL, url)


def safe_filename(value: str, max_len: int = 80) -> str:
    value = value.strip().replace("\u3000", "_")
    value = re.sub(r"\s+", "_", value)
    value = re.sub(r'[\\/:*?"<>|#%&{}$!`@+=]', "_", value)
    value = re.sub(r"_+", "_", value).strip("._ ")
    return (value[:max_len] or "untitled").strip("._ ")


def fetch_url(url: str, retries: int = 3, timeout: int = 30) -> str:
    last_error: Exception | None = None
    for attempt in range(1, retries + 1):
        request = urllib.request.Request(
            url,
            headers={
                "User-Agent": USER_AGENT,
                "Accept-Language": "zh-CN,zh;q=0.9,en;q=0.6",
            },
        )
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                data = response.read()
                charset = response.headers.get_content_charset() or "utf-8"
                return data.decode(charset, errors="replace")
        except (urllib.error.URLError, TimeoutError) as exc:
            last_error = exc
            if attempt < retries:
                time.sleep(2 * attempt)
    raise RuntimeError(f"failed to fetch {url}: {last_error}")


def is_challenge_page(text: str) -> bool:
    markers = [
        "Please confirm that you are human",
        "Access unavailable",
        "captcha.pl",
        "敬請輸入認證圖案",
    ]
    return any(marker in text for marker in markers)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def write_json(path: Path, data: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def read_or_fetch(path: Path, url: str, force: bool) -> tuple[str, bool]:
    if path.exists() and not force:
        cached = path.read_text(encoding="utf-8")
        if not is_challenge_page(cached):
            return cached, False
        print(f"warning: ignoring cached challenge page {path}", file=sys.stderr)
    text = fetch_url(url)
    if is_challenge_page(text):
        raise CaptchaError(
            f"ctext returned a human-verification page for {url}; "
            "no cache file was written"
        )
    write_text(path, text)
    return text, True


def extract_meta(html_text: str, name: str) -> str:
    pattern = rf'<meta\s+name="{re.escape(name)}"\s+content="([^"]*)"'
    match = re.search(pattern, html_text, flags=re.I)
    return html.unescape(match.group(1)).strip() if match else ""


def extract_links(fragment: str) -> list[dict[str, str]]:
    links: list[dict[str, str]] = []
    seen: set[tuple[str, str]] = set()
    for match in re.finditer(r'(?is)<a\b[^>]*href="([^"]+)"[^>]*>(.*?)</a>', fragment):
        url = normalize_url(match.group(1))
        text = html_to_text(match.group(2))
        key = (url, text)
        if key in seen:
            continue
        seen.add(key)
        links.append({"text": text, "url": url})
    return links


def parse_toc(main_html: str) -> list[dict[str, Any]]:
    chapters: list[dict[str, Any]] = []
    menu_nodes: dict[int, str] = {}
    for menu_match in re.finditer(
        r'(?is)<a\b[^>]*id="m(\d+)"[^>]*href="hongloumeng/ch(\d+)/zhs"',
        main_html,
    ):
        menu_nodes[int(menu_match.group(2))] = menu_match.group(1)

    pattern = re.compile(
        r'(?is)　?\s*(\d+)\.\s*'
        r'<a\s+href="(hongloumeng/ch(\d+)/zhs)"[^>]*>(.*?)</a>'
    )
    for match in pattern.finditer(main_html):
        number = int(match.group(1))
        url_number = int(match.group(3))
        if number != url_number:
            continue
        link_html = match.group(4)
        title_en = ""
        trans_match = re.search(
            r'(?is)<span\s+class="translationtitle"[^>]*>(.*?)</span>', link_html
        )
        if trans_match:
            title_en = html_to_text(trans_match.group(1))
            link_html = link_html[: trans_match.start()] + link_html[trans_match.end() :]
        title_zh = html_to_text(link_html)
        chapter_node_id = menu_nodes.get(number, "")
        chapter = {
            "number": number,
            "slug": f"ch{number}",
            "title_zh": title_zh,
            "title_en": title_en,
            "source_url": normalize_url(match.group(2)),
            "chapter_node_id": chapter_node_id,
        }
        if chapter_node_id:
            chapter["compact_source_url"] = normalize_url(
                f"text.pl?node={chapter_node_id}&if=gb&remap=gb&menu=none&ref=show"
            )
        chapters.append(chapter)

    by_number: dict[int, dict[str, Any]] = {}
    for chapter in chapters:
        by_number[chapter["number"]] = chapter
    return [by_number[num] for num in sorted(by_number)]


def extract_context_links(main_html: str) -> dict[str, list[dict[str, str]]]:
    links = extract_links(main_html)
    windows = [
        {"text": "window.open", "url": normalize_url(match.group(1))}
        for match in re.finditer(r"window\.open\('([^']+)'", main_html)
    ]
    links.extend(windows)

    categories = {
        "discussion_links": [],
        "library_links": [],
        "related_texts": [],
        "media_links": [],
        "translation_info": [],
        "search_links": [],
        "source_reference_links": [],
    }
    seen: dict[str, set[str]] = {key: set() for key in categories}

    def add(category: str, link: dict[str, str]) -> None:
        key = link["url"]
        if key not in seen[category]:
            seen[category].add(key)
            categories[category].append(link)

    for link in links:
        url = link["url"]
        if "discuss.pl" in url and (
            "bookid=103345" in url or "thread=" in url or "board=" in url
        ):
            add("discussion_links", link)
        if "library.pl" in url and ("node=103345" in url or "res=4652" in url):
            add("library_links", link)
        if "wiki.pl" in url and "res=" in url:
            add("related_texts", link)
        if "media.pl" in url and "id=107" in url:
            add("media_links", link)
        if "instructions/translation" in url or "faq/zhs#translations" in url:
            add("translation_info", link)
        if "search.pl" in url and "node=103345" in url:
            add("search_links", link)
        if "text.pl?node=103345" in url:
            add("source_reference_links", link)
    return categories


def parse_chapter(chapter: dict[str, Any], html_text: str) -> dict[str, Any]:
    ctp_urn = extract_meta(html_text, "ctp-urn")
    chapter_node_id = chapter.get("chapter_node_id", "")
    node_match = re.search(r'id="listscans(\d+)"', html_text)
    if node_match:
        chapter_node_id = node_match.group(1)
    else:
        node_match = re.search(r'<div id="comm(\d+)"></div>\s*<table border="0">', html_text)
        if node_match:
            chapter_node_id = node_match.group(1)

    library_links = []
    for link in extract_links(html_text):
        if "library.pl" in link["url"] and link["url"] not in {x["url"] for x in library_links}:
            library_links.append(link)

    paragraphs: list[dict[str, Any]] = []
    row_pattern = re.compile(
        r'(?is)<tr\s+id="n(\d+)">(.*?)</tr>\s*<tr>(.*?)</tr>'
    )
    for row_index, match in enumerate(row_pattern.finditer(html_text), start=1):
        node_id = match.group(1)
        chinese_row = match.group(2)
        english_row = match.group(3)
        cells = re.findall(r"(?is)<td\b[^>]*>(.*?)</td>", chinese_row)
        english_cells = re.findall(r"(?is)<td\b[^>]*>(.*?)</td>", english_row)
        if len(cells) < 3:
            continue

        first_cell = cells[0]
        paragraph_number = ""
        paragraph_match = re.search(
            r'(?is)<a\b[^>]*class="popup"[^>]*>(.*?)</a>', first_cell
        )
        if paragraph_match:
            paragraph_number = html_to_text(paragraph_match.group(1))

        dictionary_url = ""
        dictionary_match = re.search(r'(?is)href="([^"]*dictionary\.pl[^"]*)"', first_cell)
        if dictionary_match:
            dictionary_url = normalize_url(dictionary_match.group(1))

        label = html_to_text(cells[1])
        chinese_html = cells[2]
        comment_container_ids = re.findall(r'id="(comm\d+)"', chinese_html)
        inline_notes = []
        for note_match in re.finditer(
            r'(?is)<span\b[^>]*class="([^"]*inlinecomment[^"]*)"[^>]*>(.*?)</span>',
            chinese_html,
        ):
            inline_notes.append(
                {
                    "class": html.unescape(note_match.group(1)),
                    "text": html_to_text(note_match.group(2)),
                }
            )
        chinese_html = re.sub(r'(?is)<div\s+id="comm\d+"></div>', "", chinese_html)
        chinese_text = html_to_text(chinese_html)

        english_text = ""
        if english_cells:
            english_text = html_to_text(english_cells[-1])

        if not chinese_text and not english_text:
            continue

        paragraphs.append(
            {
                "row_index": row_index,
                "paragraph_number": paragraph_number,
                "node_id": node_id,
                "anchor_url": f'{chapter["source_url"]}#n{node_id}',
                "label": label,
                "text": chinese_text,
                "english_translation": english_text,
                "dictionary_url": dictionary_url,
                "comment_container_ids": comment_container_ids,
                "commentary_url": normalize_url(
                    f"commentary.pl?node={node_id}&if=gb&remap=gb"
                ),
                "commentary_status": "link_preserved_not_bulk_fetched",
                "inline_notes": inline_notes,
            }
        )

    return {
        **chapter,
        "ctp_urn": ctp_urn,
        "chapter_node_id": chapter_node_id,
        "library_links": library_links,
        "paragraph_count": len(paragraphs),
        "paragraphs": paragraphs,
    }


def chapter_stem(chapter: dict[str, Any]) -> str:
    title = safe_filename(chapter["title_zh"])
    return f'{chapter["number"]:03d}_{title}'


def render_chapter_txt(chapter: dict[str, Any]) -> str:
    lines = [
        f'第{chapter["number"]:03d}回 {chapter["title_zh"]}',
        f'来源: {chapter["source_url"]}',
        f'CTP URN: {chapter.get("ctp_urn", "")}',
        f'章节节点: {chapter.get("chapter_node_id", "")}',
        "",
    ]
    for paragraph in chapter["paragraphs"]:
        number = (
            f'p{paragraph["paragraph_number"]}'
            if paragraph["paragraph_number"]
            else f'row{paragraph["row_index"]}'
        )
        lines.append(f'[{number} | n{paragraph["node_id"]}]')
        lines.append(paragraph["text"])
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def render_chapter_md(chapter: dict[str, Any], include_english: bool) -> str:
    lines = [
        f'# 第{chapter["number"]:03d}回 {chapter["title_zh"]}',
        "",
        f'- Source: {chapter["source_url"]}',
        f'- CTP URN: {chapter.get("ctp_urn", "")}',
        f'- Chapter node: {chapter.get("chapter_node_id", "")}',
        f'- Paragraphs: {chapter["paragraph_count"]}',
    ]
    if chapter.get("title_en"):
        lines.append(f'- English title: {chapter["title_en"]}')
    if chapter.get("library_links"):
        lines.append("- Library/source scans:")
        for link in chapter["library_links"]:
            label = link["text"] or link["url"]
            lines.append(f'  - [{label}]({link["url"]})')
    lines.append("")
    lines.append("## Text")
    lines.append("")

    for paragraph in chapter["paragraphs"]:
        number = (
            f'p{paragraph["paragraph_number"]}'
            if paragraph["paragraph_number"]
            else f'row{paragraph["row_index"]}'
        )
        lines.append(f'### {number} | n{paragraph["node_id"]}')
        lines.append("")
        lines.append(paragraph["text"])
        lines.append("")
        if paragraph.get("inline_notes"):
            lines.append("Inline notes:")
            for note in paragraph["inline_notes"]:
                lines.append(f'- {note["class"]}: {note["text"]}')
            lines.append("")
        detail_bits = [
            f'[anchor]({paragraph["anchor_url"]})',
            f'[dictionary]({paragraph["dictionary_url"]})'
            if paragraph.get("dictionary_url")
            else "",
            f'[commentary endpoint]({paragraph["commentary_url"]})',
        ]
        detail_bits = [bit for bit in detail_bits if bit]
        lines.append("Links: " + " | ".join(detail_bits))
        lines.append("")
        if include_english and paragraph.get("english_translation"):
            lines.append("<details><summary>English translation from ctext</summary>")
            lines.append("")
            lines.append(paragraph["english_translation"])
            lines.append("")
            lines.append("</details>")
            lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def make_source_notice(
    output_root: Path,
    chapters: list[dict[str, Any]],
    context_links: dict[str, Any],
    commentary_probe: dict[str, Any] | None,
) -> str:
    timestamp = dt.datetime.now(dt.timezone.utc).astimezone().isoformat(timespec="seconds")
    total_paragraphs = sum(chapter.get("paragraph_count", 0) for chapter in chapters)
    probe_text = "not tested"
    if commentary_probe:
        probe_text = (
            f'{commentary_probe["url"]} returned '
            f'{commentary_probe["response_preview"]!r}'
        )
    return f"""# ctext Hongloumeng Download Notes

Generated: {timestamp}
Source: {START_URL}
Output root: {output_root}

Downloaded chapters: {len(chapters)}
Parsed text units: {total_paragraphs}

The ctext page footer asks users not to use automated software to download
large numbers of pages. This run used a single sequential request stream and
caches raw HTML in `raw_html/` so later parser runs do not need to revisit the
site.

Preserved elements:

- chapter title, ctext source URL, CTP URN, and chapter node id;
- raw source HTML for each chapter;
- per-paragraph node id, paragraph number, anchor URL, dictionary URL, and
  dynamic commentary endpoint URL;
- any inline comment spans present in the HTML;
- ctext English translations, stored in JSON and Markdown;
- electronic library/source-scan links where ctext exposes them;
- book-level discussion, library, related-text, media, translation-info, and
  search links in `metadata/context_links.json`.

Commentary note:

The static chapter HTML contains empty `comm...` containers for dynamic
commentary. The JavaScript endpoint was probed but not called for every
paragraph, to avoid thousands of extra requests. Probe result: {probe_text}.
The endpoint links are still preserved for targeted follow-up.

Useful context counts:

- discussion links: {len(context_links.get("discussion_links", []))}
- library links: {len(context_links.get("library_links", []))}
- related text links: {len(context_links.get("related_texts", []))}
- media links: {len(context_links.get("media_links", []))}
- translation info links: {len(context_links.get("translation_info", []))}
"""


def probe_commentary(first_chapter: dict[str, Any]) -> dict[str, Any] | None:
    paragraphs = first_chapter.get("paragraphs") or []
    if not paragraphs:
        return None
    url = paragraphs[0]["commentary_url"]
    response = fetch_url(url)
    return {
        "url": url,
        "node_id": paragraphs[0]["node_id"],
        "response_preview": response[:200],
        "meaning": (
            "ctext JavaScript treats the first byte as a status marker; "
            "a response of '1' means success with empty commentary HTML."
        ),
    }


def run(args: argparse.Namespace) -> int:
    output_root = Path(args.out)
    raw_dir = output_root / "raw_html"
    json_dir = output_root / "chapters_json"
    txt_dir = output_root / "chapters_txt"
    md_dir = output_root / "chapters_md"
    metadata_dir = output_root / "metadata"
    combined_dir = output_root / "combined"

    for directory in [raw_dir, json_dir, txt_dir, md_dir, metadata_dir, combined_dir]:
        directory.mkdir(parents=True, exist_ok=True)

    main_path = raw_dir / "000_hongloumeng_index.html"
    if args.offline_existing:
        if not main_path.exists():
            raise RuntimeError(f"offline mode needs cached index: {main_path}")
        main_html = main_path.read_text(encoding="utf-8")
        fetched_main = False
    else:
        main_html, fetched_main = read_or_fetch(main_path, START_URL, args.force)
        if fetched_main and args.delay:
            time.sleep(args.delay)

    chapters = parse_toc(main_html)
    if not chapters:
        raise RuntimeError("no chapters found in table of contents")
    if args.start:
        chapters = [chapter for chapter in chapters if chapter["number"] >= args.start]
    if args.limit:
        chapters = chapters[: args.limit]

    context_links = extract_context_links(main_html)
    write_json(metadata_dir / "toc.json", chapters)
    write_json(metadata_dir / "context_links.json", context_links)

    parsed_chapters: list[dict[str, Any]] = []
    missing_chapters: list[dict[str, Any]] = []
    for index, chapter in enumerate(chapters, start=1):
        raw_path = raw_dir / f'{chapter["number"]:03d}_{chapter["slug"]}.html'
        if args.offline_existing:
            if not raw_path.exists():
                missing_chapters.append({**chapter, "reason": "raw_html_missing"})
                print(
                    f'[{index:03d}/{len(chapters):03d}] missing ch{chapter["number"]}',
                    flush=True,
                )
                continue
            chapter_html = raw_path.read_text(encoding="utf-8")
            if is_challenge_page(chapter_html):
                missing_chapters.append({**chapter, "reason": "raw_html_challenge_page"})
                print(
                    f'[{index:03d}/{len(chapters):03d}] blocked-cache ch{chapter["number"]}',
                    flush=True,
                )
                continue
            fetched = False
        else:
            chapter_html, fetched = read_or_fetch(raw_path, chapter["source_url"], args.force)
        parsed = parse_chapter(chapter, chapter_html)
        if parsed["paragraph_count"] == 0:
            raise RuntimeError(
                f'chapter {chapter["number"]} parsed with zero paragraphs; '
                "inspect the cached raw HTML before continuing"
            )

        stem = chapter_stem(parsed)
        write_json(json_dir / f"{stem}.json", parsed)
        write_text(txt_dir / f"{stem}.txt", render_chapter_txt(parsed))
        write_text(md_dir / f"{stem}.md", render_chapter_md(parsed, args.include_english))
        parsed_chapters.append(parsed)

        status = "fetched" if fetched else "cached"
        print(
            f'[{index:03d}/{len(chapters):03d}] {status} ch{chapter["number"]}: '
            f'{parsed["paragraph_count"]} paragraphs',
            flush=True,
        )
        if fetched and args.delay and index < len(chapters):
            time.sleep(args.delay)

    commentary_probe = None
    if args.probe_commentary and parsed_chapters:
        if args.delay:
            time.sleep(args.delay)
        commentary_probe = probe_commentary(parsed_chapters[0])
        write_json(metadata_dir / "commentary_probe.json", commentary_probe)

    write_json(combined_dir / "hongloumeng_all.json", parsed_chapters)
    write_text(
        combined_dir / "hongloumeng_all.txt",
        "\n\n".join(render_chapter_txt(chapter).rstrip() for chapter in parsed_chapters)
        + "\n",
    )
    write_text(
        combined_dir / "hongloumeng_all.md",
        "\n\n".join(
            render_chapter_md(chapter, args.include_english).rstrip()
            for chapter in parsed_chapters
        )
        + "\n",
    )

    report = {
        "source_url": START_URL,
        "output_root": str(output_root),
        "chapters": len(parsed_chapters),
        "missing_chapters": len(missing_chapters),
        "paragraphs": sum(chapter["paragraph_count"] for chapter in parsed_chapters),
        "generated_at": dt.datetime.now(dt.timezone.utc)
        .astimezone()
        .isoformat(timespec="seconds"),
        "delay_seconds": args.delay,
        "include_english": args.include_english,
        "probe_commentary": bool(args.probe_commentary),
        "offline_existing": bool(args.offline_existing),
    }
    write_json(metadata_dir / "download_report.json", report)
    write_json(metadata_dir / "missing_chapters.json", missing_chapters)
    write_text(
        metadata_dir / "source_notice.md",
        make_source_notice(output_root, parsed_chapters, context_links, commentary_probe),
    )
    print(
        f'done: {report["chapters"]} chapters, {report["paragraphs"]} paragraphs, '
        f'{report["missing_chapters"]} missing -> {output_root}',
        flush=True,
    )
    return 0


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", default="resources/cache/ctext_hongloumeng")
    parser.add_argument("--delay", type=float, default=2.0)
    parser.add_argument("--start", type=int, default=1)
    parser.add_argument("--limit", type=int, default=0)
    parser.add_argument("--force", action="store_true")
    parser.add_argument(
        "--offline-existing",
        action="store_true",
        help="Parse only cached raw HTML, skip missing chapters, and make combined files.",
    )
    parser.add_argument("--no-english", dest="include_english", action="store_false")
    parser.add_argument("--probe-commentary", action="store_true")
    parser.set_defaults(include_english=True)
    return parser.parse_args(argv)


if __name__ == "__main__":
    try:
        raise SystemExit(run(parse_args(sys.argv[1:])))
    except CaptchaError as exc:
        print(f"stopped: {exc}", file=sys.stderr)
        raise SystemExit(2)
