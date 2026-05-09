#!/usr/bin/env python3
"""Download MediaWiki/Wikisource pages into a normalized source snapshot."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
from html.parser import HTMLParser
import json
import re
import shutil
import ssl
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any


DEFAULT_API_URL = "https://zh.wikisource.org/w/api.php"
USER_AGENT = "TonglingyuSourceBuilder/0.1 (local research source snapshot)"
BLOCK_TAGS = {
    "blockquote",
    "dd",
    "div",
    "dt",
    "figcaption",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "li",
    "p",
    "pre",
    "td",
    "th",
}
SKIP_TAGS = {"head", "script", "style", "table"}


def now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).astimezone().isoformat(timespec="seconds")


def ensure_dir(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)


def write_text(path: Path, text: str) -> None:
    ensure_dir(path.parent)
    path.write_text(text, encoding="utf-8")


def write_json(path: Path, data: Any) -> None:
    ensure_dir(path.parent)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def write_jsonl(path: Path, records: list[dict[str, Any]]) -> None:
    ensure_dir(path.parent)
    with path.open("w", encoding="utf-8") as handle:
        for record in records:
            handle.write(json.dumps(record, ensure_ascii=False, sort_keys=True) + "\n")


def slugify(value: str, fallback: str = "wiki-source") -> str:
    value = value.strip().replace("\u3000", " ")
    value = re.sub(r"\s+", "_", value)
    value = re.sub(r'[\\/:*?"<>|#%&{}$!`@+=，。；：、（）()\[\]]', "_", value)
    value = re.sub(r"_+", "_", value).strip("._ ")
    return value[:120].strip("._ ") or fallback


def normalize_spaces(value: str) -> str:
    value = value.replace("\xa0", " ").replace("\u3000", " ")
    value = re.sub(r"[ \t\r\f\v]+", " ", value)
    value = re.sub(r" *\n+ *", "\n", value)
    return value.strip()


def sha256_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


class WikiHtmlParser(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.blocks: list[dict[str, Any]] = []
        self.links: list[dict[str, str]] = []
        self._parts: list[str] = []
        self._current_tag = "p"
        self._skip_stack: list[str] = []
        self._link_stack: list[dict[str, Any]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        tag = tag.lower()
        attrs_dict = {key.lower(): value or "" for key, value in attrs}
        classes = set(attrs_dict.get("class", "").split())
        if tag in SKIP_TAGS or classes & {"mw-editsection", "noprint", "metadata"}:
            self._skip_stack.append(tag)
            return
        if self._skip_stack:
            return
        if tag in BLOCK_TAGS:
            self._flush()
            self._current_tag = tag
            return
        if tag == "br":
            self._parts.append("\n")
            return
        if tag == "a":
            href = attrs_dict.get("href", "")
            self._link_stack.append({"href": href, "text": []})

    def handle_endtag(self, tag: str) -> None:
        tag = tag.lower()
        if self._skip_stack and self._skip_stack[-1] == tag:
            self._skip_stack.pop()
            return
        if self._skip_stack:
            return
        if tag == "a" and self._link_stack:
            link = self._link_stack.pop()
            text = normalize_spaces("".join(link["text"]))
            if link["href"] or text:
                self.links.append({"href": link["href"], "text": text})
            return
        if tag in BLOCK_TAGS:
            self._flush()

    def handle_data(self, data: str) -> None:
        if self._skip_stack:
            return
        self._parts.append(data)
        for link in self._link_stack:
            link["text"].append(data)

    def close(self) -> None:
        super().close()
        self._flush()

    def _flush(self) -> None:
        text = normalize_spaces("".join(self._parts))
        self._parts = []
        if not text:
            return
        kind = "heading" if self._current_tag in {"h1", "h2", "h3", "h4", "h5", "h6"} else "paragraph"
        self.blocks.append(
            {
                "index": len(self.blocks) + 1,
                "tag": self._current_tag,
                "kind": kind,
                "text": text,
            }
        )


def api_get(api_url: str, params: dict[str, Any], insecure_skip_tls_verify: bool = False) -> dict[str, Any]:
    query = urllib.parse.urlencode({**params, "format": "json", "formatversion": "2"})
    request = urllib.request.Request(
        f"{api_url}?{query}",
        headers={"User-Agent": USER_AGENT, "Accept": "application/json"},
    )
    context = ssl._create_unverified_context() if insecure_skip_tls_verify else None
    with urllib.request.urlopen(request, timeout=60, context=context) as response:
        data = response.read()
    return json.loads(data.decode("utf-8"))


def list_prefix_pages(
    api_url: str,
    prefix: str,
    limit: int = 0,
    insecure_skip_tls_verify: bool = False,
) -> list[str]:
    titles: list[str] = []
    params: dict[str, Any] = {
        "action": "query",
        "list": "allpages",
        "apnamespace": 0,
        "apprefix": prefix,
        "aplimit": "max",
    }
    while True:
        payload = api_get(api_url, params, insecure_skip_tls_verify)
        for page in payload.get("query", {}).get("allpages", []):
            title = page.get("title", "")
            if title:
                titles.append(title)
                if limit and len(titles) >= limit:
                    return titles
        continuation = payload.get("continue", {})
        if "apcontinue" not in continuation:
            break
        params["apcontinue"] = continuation["apcontinue"]
    return titles


def fetch_page(api_url: str, title: str, insecure_skip_tls_verify: bool = False) -> dict[str, Any]:
    query_payload = api_get(
        api_url,
        {
            "action": "query",
            "prop": "info|revisions",
            "inprop": "url",
            "rvprop": "ids|timestamp|content",
            "rvslots": "main",
            "titles": title,
        },
        insecure_skip_tls_verify,
    )
    pages = query_payload.get("query", {}).get("pages", [])
    if not pages or pages[0].get("missing"):
        return {"title": title, "missing": True}
    page = pages[0]
    revision = (page.get("revisions") or [{}])[0]
    slot = revision.get("slots", {}).get("main", {})
    wikitext = slot.get("content", "")

    parse_payload = api_get(
        api_url,
        {
            "action": "parse",
            "page": page["title"],
            "prop": "text|displaytitle|sections",
            "disableeditsection": 1,
            "redirects": 1,
        },
        insecure_skip_tls_verify,
    )
    parsed = parse_payload.get("parse", {})
    html = parsed.get("text", "")
    parser = WikiHtmlParser()
    parser.feed(html)
    parser.close()
    return {
        "title": page["title"],
        "pageid": page.get("pageid"),
        "fullurl": page.get("fullurl", ""),
        "revision_id": revision.get("revid"),
        "parent_id": revision.get("parentid"),
        "revision_timestamp": revision.get("timestamp", ""),
        "wikitext_sha256": sha256_text(wikitext),
        "display_title": parsed.get("displaytitle", page["title"]),
        "sections": parsed.get("sections", []),
        "wikitext": wikitext,
        "html": html,
        "blocks": parser.blocks,
        "links": parser.links,
        "missing": False,
    }


def collect_titles(args: argparse.Namespace) -> list[str]:
    titles: list[str] = []
    for page in args.page:
        titles.append(page)
    for pages_file in args.pages_file:
        for line in Path(pages_file).read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                titles.append(line)
    remaining_limit = args.limit if args.limit else 0
    for prefix in args.prefix:
        prefix_titles = list_prefix_pages(
            args.api_url,
            prefix,
            remaining_limit,
            args.insecure_skip_tls_verify,
        )
        titles.extend(prefix_titles)
        if remaining_limit:
            remaining_limit = max(remaining_limit - len(prefix_titles), 0)
    deduped: list[str] = []
    seen = set()
    for title in titles:
        if title not in seen:
            deduped.append(title)
            seen.add(title)
    if args.limit:
        return deduped[: args.limit]
    return deduped


def render_document_txt(document: dict[str, Any]) -> str:
    lines = [document["title"], f"Source URL: {document.get('fullurl', '')}", ""]
    for block in document["blocks"]:
        lines.append(block["text"])
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def render_document_md(document: dict[str, Any]) -> str:
    lines = [f"# {document['title']}", "", f"- Source URL: {document.get('fullurl', '')}", ""]
    for block in document["blocks"]:
        if block["kind"] == "heading":
            lines.append(f"## {block['text']}")
        else:
            lines.append(block["text"])
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def run(args: argparse.Namespace) -> int:
    titles = collect_titles(args)
    if not titles:
        raise ValueError("no pages requested; pass --page, --prefix, or --pages-file")

    source_id = slugify(args.source_id or args.title or titles[0], "wiki-source")
    output_root = Path(args.out).expanduser().resolve() / source_id
    if output_root.exists() and any(output_root.iterdir()):
        if not args.overwrite:
            raise FileExistsError(f"{output_root} already exists; pass --overwrite to replace it")
        shutil.rmtree(output_root)
    ensure_dir(output_root)

    fetched_at = now_iso()
    documents: list[dict[str, Any]] = []
    blocks: list[dict[str, Any]] = []
    pages: list[dict[str, Any]] = []
    missing: list[dict[str, Any]] = []

    for index, title in enumerate(titles, start=1):
        page = fetch_page(args.api_url, title, args.insecure_skip_tls_verify)
        if page.get("missing"):
            missing.append({"title": title})
            continue
        section_id = f"{source_id}:page:{index:04d}"
        page_blocks = []
        for block in page["blocks"]:
            block_id = f"{section_id}:block:{block['index']:04d}"
            record = {
                "source_id": source_id,
                "section_id": section_id,
                "block_id": block_id,
                "section_index": index,
                "block_index": block["index"],
                "kind": block["kind"],
                "tag": block["tag"],
                "text": block["text"],
                "source_title": page["title"],
                "source_url": page["fullurl"],
                "revision_id": page["revision_id"],
            }
            blocks.append(record)
            page_blocks.append(record)
        document = {
            "source_id": source_id,
            "section_id": section_id,
            "section_index": index,
            "title": page["title"],
            "display_title": page["display_title"],
            "pageid": page["pageid"],
            "fullurl": page["fullurl"],
            "revision_id": page["revision_id"],
            "revision_timestamp": page["revision_timestamp"],
            "wikitext_sha256": page["wikitext_sha256"],
            "sections": page["sections"],
            "links": page["links"],
            "blocks": page_blocks,
        }
        documents.append(document)
        pages.append({key: value for key, value in document.items() if key not in {"blocks", "links"}})
        write_text(output_root / "raw" / f"{index:04d}_{slugify(page['title'])}.wiki", page["wikitext"])
        write_text(output_root / "raw_html" / f"{index:04d}_{slugify(page['title'])}.html", page["html"])
        if args.delay and index < len(titles):
            time.sleep(args.delay)

    source = {
        "source_id": source_id,
        "source_category": args.source_category,
        "format": "mediawiki",
        "title": args.title or source_id,
        "work": args.work or args.title or source_id,
        "edition": args.edition,
        "language": args.language,
        "api_url": args.api_url,
        "requested_pages": args.page,
        "requested_prefixes": args.prefix,
        "fetched_at": fetched_at,
        "notes": args.notes,
    }
    report = {
        "source_id": source_id,
        "output": str(output_root),
        "requested_titles": len(titles),
        "documents": len(documents),
        "blocks": len(blocks),
        "missing": len(missing),
    }

    write_json(output_root / "metadata" / "source.json", source)
    write_json(output_root / "metadata" / "pages.json", pages)
    write_json(output_root / "metadata" / "missing_pages.json", missing)
    write_json(output_root / "metadata" / "extraction_report.json", report)
    write_jsonl(output_root / "documents" / "documents.jsonl", documents)
    write_jsonl(output_root / "documents" / "blocks.jsonl", blocks)
    write_text(
        output_root / "combined" / "all_sections.txt",
        "\n\n".join(render_document_txt(document).rstrip() for document in documents) + "\n",
    )
    write_text(
        output_root / "combined" / "all_sections.md",
        "\n\n".join(render_document_md(document).rstrip() for document in documents) + "\n",
    )

    print(
        f"done: source_id={source_id} documents={report['documents']} "
        f"blocks={report['blocks']} missing={report['missing']} -> {output_root}",
        flush=True,
    )
    return 0


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--api-url", default=DEFAULT_API_URL)
    parser.add_argument("--out", default="resources/sources/wiki")
    parser.add_argument("--source-id")
    parser.add_argument(
        "--source-category",
        default="base_material",
        choices=(
            "base_material",
            "extended_base_material",
            "commentary_material",
            "research_material",
            "style_material",
            "evaluation_material",
        ),
    )
    parser.add_argument("--title", default="")
    parser.add_argument("--work", default="")
    parser.add_argument("--edition", default="")
    parser.add_argument("--language", default="zh")
    parser.add_argument("--notes", default="")
    parser.add_argument("--page", action="append", default=[], help="MediaWiki page title. May be repeated.")
    parser.add_argument("--prefix", action="append", default=[], help="Download namespace-0 pages with this prefix.")
    parser.add_argument("--pages-file", action="append", default=[], help="UTF-8 file with one page title per line.")
    parser.add_argument("--limit", type=int, default=0, help="Limit collected pages for smoke tests.")
    parser.add_argument("--delay", type=float, default=0.2, help="Delay between page fetches.")
    parser.add_argument("--overwrite", action="store_true")
    parser.add_argument(
        "--insecure-skip-tls-verify",
        action="store_true",
        help="Debug-only fallback for local Python installations missing CA certificates.",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    return run(parse_args(argv))


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
