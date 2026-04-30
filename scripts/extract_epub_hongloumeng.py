#!/usr/bin/env python3
"""Extract text, notes, assets, and metadata from the local Hongloumeng EPUB."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import posixpath
import re
import shutil
import sys
import warnings
import zipfile
from pathlib import Path
from typing import Any

from bs4 import BeautifulSoup
from bs4 import XMLParsedAsHTMLWarning
from ebooklib import ITEM_DOCUMENT, ITEM_IMAGE, ITEM_STYLE
from ebooklib import epub


warnings.filterwarnings("ignore", category=XMLParsedAsHTMLWarning)


CHINESE_DIGITS = {
    "零": 0,
    "〇": 0,
    "○": 0,
    "一": 1,
    "二": 2,
    "三": 3,
    "四": 4,
    "五": 5,
    "六": 6,
    "七": 7,
    "八": 8,
    "九": 9,
}


def ensure_dir(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=True)


def write_text(path: Path, text: str) -> None:
    ensure_dir(path.parent)
    path.write_text(text, encoding="utf-8")


def write_bytes(path: Path, data: bytes) -> None:
    ensure_dir(path.parent)
    path.write_bytes(data)


def write_json(path: Path, data: Any) -> None:
    ensure_dir(path.parent)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


def safe_filename(value: str, max_len: int = 90) -> str:
    value = value.strip().replace("\u3000", " ")
    value = re.sub(r"\s+", "_", value)
    value = re.sub(r'[\\/:*?"<>|#%&{}$!`@+=，。；：、（）()]', "_", value)
    value = re.sub(r"_+", "_", value).strip("._ ")
    return (value[:max_len] or "untitled").strip("._ ")


def chinese_number_to_int(value: str) -> int | None:
    value = value.strip()
    if not value:
        return None
    if any(ch in value for ch in "零〇○") and "十" not in value and "百" not in value:
        digits = [CHINESE_DIGITS.get(ch) for ch in value]
        if any(digit is None for digit in digits):
            return None
        return int("".join(str(digit) for digit in digits))
    if "十" in value:
        left, _, right = value.partition("十")
        tens = CHINESE_DIGITS.get(left, 1) if left else 1
        ones = CHINESE_DIGITS.get(right, 0) if right else 0
        return tens * 10 + ones
    if "百" in value:
        left, _, right = value.partition("百")
        hundreds = CHINESE_DIGITS.get(left, 1) if left else 1
        tail = chinese_number_to_int(right) or 0
        return hundreds * 100 + tail
    if all(ch in CHINESE_DIGITS for ch in value):
        return int("".join(str(CHINESE_DIGITS[ch]) for ch in value))
    return None


def chapter_numbers_from_title(title: str) -> list[int]:
    match = re.match(r"^第([一二三四五六七八九十百零〇○]+)回(?:至([一二三四五六七八九十百零〇○]+)回)?", title)
    if not match:
        return []
    first = chinese_number_to_int(match.group(1))
    second = chinese_number_to_int(match.group(2)) if match.group(2) else None
    if first is None:
        return []
    if second is not None and second >= first:
        return list(range(first, second + 1))
    return [first]


def chapter_prefix(numbers: list[int], fallback_index: int) -> str:
    if not numbers:
        return f"section-{fallback_index:03d}"
    if len(numbers) == 1:
        return f"{numbers[0]:03d}"
    return f"{numbers[0]:03d}-{numbers[-1]:03d}"


def metadata_to_dict(book: epub.EpubBook) -> dict[str, list[dict[str, Any]]]:
    result: dict[str, list[dict[str, Any]]] = {}
    for namespace, values in book.metadata.items():
        for key, entries in values.items():
            out_key = f"{namespace}{key}"
            result[out_key] = [
                {"value": value, "attributes": dict(attrs)} for value, attrs in entries
            ]
    return result


def item_type_name(item: Any) -> str:
    type_id = item.get_type()
    if type_id == ITEM_DOCUMENT:
        return "document"
    if type_id == ITEM_IMAGE:
        return "image"
    if type_id == ITEM_STYLE:
        return "style"
    return str(type_id)


def parse_toc(epub_path: Path) -> list[dict[str, Any]]:
    with zipfile.ZipFile(epub_path) as zf:
        data = zf.read("toc.ncx")
    soup = BeautifulSoup(data, "xml")

    def parse_nav(nav: Any, depth: int) -> dict[str, Any]:
        label_tag = nav.find("navLabel", recursive=False)
        text_tag = label_tag.find("text", recursive=False) if label_tag else None
        content_tag = nav.find("content", recursive=False)
        src = content_tag.get("src", "") if content_tag else ""
        children = [parse_nav(child, depth + 1) for child in nav.find_all("navPoint", recursive=False)]
        return {
            "id": nav.get("id", ""),
            "play_order": int(nav.get("playOrder", "0") or 0),
            "label": text_tag.get_text(strip=True) if text_tag else "",
            "src": src,
            "file": src.split("#", 1)[0],
            "fragment": src.split("#", 1)[1] if "#" in src else "",
            "depth": depth,
            "children": children,
        }

    nav_map = soup.find("navMap")
    if not nav_map:
        return []
    return [parse_nav(nav, 1) for nav in nav_map.find_all("navPoint", recursive=False)]


def flatten_toc(toc: list[dict[str, Any]]) -> list[dict[str, Any]]:
    result: list[dict[str, Any]] = []

    def visit(entry: dict[str, Any]) -> None:
        flat = {k: v for k, v in entry.items() if k != "children"}
        result.append(flat)
        for child in entry.get("children", []):
            visit(child)

    for item in toc:
        visit(item)
    return result


def resolve_epub_path(current_name: str, href: str) -> str:
    if not href:
        return ""
    path = href.split("#", 1)[0]
    if not path:
        path = current_name
    elif not path.startswith("/"):
        path = posixpath.normpath(posixpath.join(posixpath.dirname(current_name), path))
    return path.lstrip("/")


def normalize_href(current_name: str, href: str) -> dict[str, str]:
    if not href:
        return {"href": "", "file": "", "fragment": ""}
    file_part, fragment = (href.split("#", 1) + [""])[:2] if "#" in href else (href, "")
    file_name = resolve_epub_path(current_name, file_part)
    return {"href": href, "file": file_name, "fragment": fragment}


def soup_from_bytes(data: bytes) -> BeautifulSoup:
    return BeautifulSoup(data, "lxml")


def text_from_tag(tag: Any, current_name: str, image_mode: str) -> str:
    soup = soup_from_bytes(str(tag).encode("utf-8"))
    for img in soup.find_all("img"):
        src = resolve_epub_path(current_name, img.get("src", ""))
        if image_mode == "markdown":
            replacement = f"![{img.get('alt', '')}](../{src})"
        else:
            replacement = f"[[image:{src}]]"
        img.replace_with(replacement)
    for anchor in soup.find_all("a"):
        if anchor.get("id") and not anchor.get("href") and not anchor.get_text(strip=True):
            anchor.decompose()
    text = soup.get_text("", strip=True)
    text = text.replace("\xa0", " ").replace("\u3000", " ")
    text = re.sub(r"[ \t\r\f\v]+", " ", text)
    return text.strip()


def extract_images_from_tag(tag: Any, current_name: str) -> list[dict[str, str]]:
    images = []
    for img in tag.find_all("img"):
        src = resolve_epub_path(current_name, img.get("src", ""))
        images.append(
            {
                "src": src,
                "alt": img.get("alt", ""),
                "class": " ".join(img.get("class", [])),
            }
        )
    return images


def extract_note_references(tag: Any, current_name: str) -> list[dict[str, str]]:
    refs: list[dict[str, str]] = []
    for link in tag.find_all("a", href=True):
        href = link.get("href", "")
        if "#m" not in href:
            continue
        target = normalize_href(current_name, href)
        previous_anchor = link.find_previous("a", id=True)
        refs.append(
            {
                "label": link.get_text("", strip=True),
                "href": href,
                "target_file": target["file"],
                "target_id": target["fragment"],
                "source_anchor_id": previous_anchor.get("id", "") if previous_anchor else "",
            }
        )
    return refs


def parse_note(note_tag: Any, current_name: str, index: int) -> dict[str, Any]:
    note_id = ""
    id_anchor = note_tag.find("a", id=True)
    if id_anchor:
        note_id = id_anchor.get("id", "")
    backlink = note_tag.find("a", href=True)
    href = backlink.get("href", "") if backlink else ""
    label = backlink.get_text("", strip=True) if backlink else ""
    full_text = text_from_tag(note_tag, current_name, "text")
    body = full_text
    if label and body.startswith(label):
        body = body[len(label) :].strip()
    target = normalize_href(current_name, href)
    return {
        "index": index,
        "id": note_id,
        "label": label,
        "backlink_href": href,
        "backlink_file": target["file"],
        "backlink_id": target["fragment"],
        "text": body,
        "full_text": full_text,
        "html": str(note_tag),
        "images": extract_images_from_tag(note_tag, current_name),
    }


def parse_document(
    item: Any,
    spine_index: int,
    toc_entries: list[dict[str, Any]],
) -> dict[str, Any]:
    current_name = item.get_name()
    soup = soup_from_bytes(item.get_content())
    title_tag = soup.find(["h1", "h2", "h3", "title"])
    title_from_html = title_tag.get_text(" ", strip=True) if title_tag else ""
    toc_labels = [entry["label"] for entry in toc_entries if entry.get("label")]
    title = next((label for label in toc_labels if label.startswith("第") and "回" in label), "")
    if not title:
        title = toc_labels[0] if toc_labels else title_from_html

    body = soup.find("body")
    notes: list[dict[str, Any]] = []
    blocks: list[dict[str, Any]] = []
    links: list[dict[str, str]] = []
    images: list[dict[str, str]] = []
    note_index = 0
    block_index = 0
    if body:
        for tag in body.find_all(["h1", "h2", "h3", "p", "div"], recursive=False):
            classes = tag.get("class", [])
            if "note" in classes:
                note_index += 1
                notes.append(parse_note(tag, current_name, note_index))
                continue
            block_index += 1
            kind = tag.name
            block_text = text_from_tag(tag, current_name, "text")
            block_md = text_from_tag(tag, current_name, "markdown")
            block_images = extract_images_from_tag(tag, current_name)
            block_refs = extract_note_references(tag, current_name)
            blocks.append(
                {
                    "index": block_index,
                    "tag": tag.name,
                    "class": " ".join(classes),
                    "kind": "heading" if kind.startswith("h") else "paragraph",
                    "text": block_text,
                    "markdown": block_md,
                    "html": str(tag),
                    "note_refs": block_refs,
                    "images": block_images,
                }
            )
            images.extend(block_images)
            for link in tag.find_all("a", href=True):
                target = normalize_href(current_name, link.get("href", ""))
                links.append(
                    {
                        "text": link.get_text("", strip=True),
                        "href": link.get("href", ""),
                        "target_file": target["file"],
                        "target_id": target["fragment"],
                    }
                )

    chapter_numbers = chapter_numbers_from_title(title)
    return {
        "spine_index": spine_index,
        "item_id": item.get_id(),
        "file": current_name,
        "title": title,
        "title_from_html": title_from_html,
        "toc_entries": toc_entries,
        "chapter_numbers": chapter_numbers,
        "is_chapter": bool(chapter_numbers),
        "block_count": len(blocks),
        "note_count": len(notes),
        "image_count": len(images),
        "link_count": len(links),
        "blocks": blocks,
        "notes": notes,
        "images": images,
        "links": links,
    }


def render_section_txt(section: dict[str, Any]) -> str:
    lines = [
        section["title"] or section["file"],
        f'EPUB file: {section["file"]}',
        "",
    ]
    for block in section["blocks"]:
        if not block["text"]:
            continue
        lines.append(block["text"])
        lines.append("")
    if section["notes"]:
        lines.append("注释/校记")
        lines.append("")
        for note in section["notes"]:
            label = note["label"] or note["id"] or str(note["index"])
            lines.append(f'{label} {note["text"]}'.strip())
            lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def render_section_md(section: dict[str, Any]) -> str:
    title = section["title"] or section["file"]
    lines = [
        f"# {title}",
        "",
        f'- EPUB file: `{section["file"]}`',
        f'- Notes: {section["note_count"]}',
        f'- Images: {section["image_count"]}',
        "",
    ]
    for block in section["blocks"]:
        text = block["markdown"]
        if not text:
            continue
        if block["kind"] == "heading":
            lines.append(f"## {text}")
        else:
            lines.append(text)
        lines.append("")
    if section["notes"]:
        lines.append("## 注释/校记")
        lines.append("")
        for note in section["notes"]:
            label = note["label"] or note["id"] or str(note["index"])
            lines.append(f'- **{label}** {note["text"]}'.rstrip())
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def build_stem(section: dict[str, Any]) -> str:
    prefix = chapter_prefix(section["chapter_numbers"], section["spine_index"])
    title = safe_filename(section["title"] or section["file"])
    return f"{prefix}_{title}"


def copy_epub_payload(epub_path: Path, raw_dir: Path) -> list[str]:
    if raw_dir.exists():
        shutil.rmtree(raw_dir)
    ensure_dir(raw_dir)
    names: list[str] = []
    with zipfile.ZipFile(epub_path) as zf:
        for info in zf.infolist():
            if info.is_dir():
                continue
            target = raw_dir / info.filename
            write_bytes(target, zf.read(info.filename))
            names.append(info.filename)
    return names


def run(args: argparse.Namespace) -> int:
    epub_path = Path(args.epub)
    output = Path(args.out)
    if not epub_path.exists():
        raise FileNotFoundError(epub_path)

    ensure_dir(output)
    raw_files = copy_epub_payload(epub_path, output / "raw_epub")

    book = epub.read_epub(str(epub_path))
    toc = parse_toc(epub_path)
    flat_toc = flatten_toc(toc)
    toc_by_file: dict[str, list[dict[str, Any]]] = {}
    for entry in flat_toc:
        toc_by_file.setdefault(entry["file"], []).append(entry)

    manifest = []
    for item in book.get_items():
        content = item.get_content()
        manifest.append(
            {
                "id": item.get_id(),
                "file": item.get_name(),
                "media_type": getattr(item, "media_type", ""),
                "type": item_type_name(item),
                "bytes": len(content),
            }
        )
        if item.get_type() == ITEM_IMAGE:
            write_bytes(output / item.get_name(), content)
        elif item.get_type() == ITEM_STYLE:
            write_bytes(output / item.get_name(), content)

    item_by_id = {item.get_id(): item for item in book.get_items()}
    sections: list[dict[str, Any]] = []
    spine_docs = []
    for spine_index, (item_id, linear) in enumerate(book.spine, start=1):
        item = item_by_id.get(item_id)
        if not item or item.get_type() != ITEM_DOCUMENT:
            continue
        toc_entries = toc_by_file.get(item.get_name(), [])
        section = parse_document(item, spine_index, toc_entries)
        sections.append(section)
        spine_docs.append(
            {
                "spine_index": spine_index,
                "item_id": item_id,
                "linear": linear,
                "file": item.get_name(),
                "title": section["title"],
                "is_chapter": section["is_chapter"],
                "chapter_numbers": section["chapter_numbers"],
            }
        )

    chapter_sections = [section for section in sections if section["is_chapter"]]
    note_records: list[dict[str, Any]] = []
    for section in sections:
        for note in section["notes"]:
            note_records.append(
                {
                    "section_file": section["file"],
                    "section_title": section["title"],
                    "chapter_numbers": section["chapter_numbers"],
                    **{k: v for k, v in note.items() if k != "html"},
                }
            )

    for section in sections:
        stem = build_stem(section)
        write_json(output / "sections_json" / f"{stem}.json", section)
        write_text(output / "sections_txt" / f"{stem}.txt", render_section_txt(section))
        write_text(output / "sections_md" / f"{stem}.md", render_section_md(section))
        if section["is_chapter"]:
            write_json(output / "chapters_json" / f"{stem}.json", section)
            write_text(output / "chapters_txt" / f"{stem}.txt", render_section_txt(section))
            write_text(output / "chapters_md" / f"{stem}.md", render_section_md(section))

    combined_chapters_txt = "\n\n".join(render_section_txt(section).rstrip() for section in chapter_sections) + "\n"
    combined_chapters_md = "\n\n".join(render_section_md(section).rstrip() for section in chapter_sections) + "\n"
    combined_all_txt = "\n\n".join(render_section_txt(section).rstrip() for section in sections) + "\n"
    combined_all_md = "\n\n".join(render_section_md(section).rstrip() for section in sections) + "\n"

    write_text(output / "combined" / "hongloumeng_chapters.txt", combined_chapters_txt)
    write_text(output / "combined" / "hongloumeng_chapters.md", combined_chapters_md)
    write_json(output / "combined" / "hongloumeng_chapters.json", chapter_sections)
    write_text(output / "combined" / "hongloumeng_all_sections.txt", combined_all_txt)
    write_text(output / "combined" / "hongloumeng_all_sections.md", combined_all_md)
    write_json(output / "combined" / "hongloumeng_all_sections.json", sections)

    metadata = metadata_to_dict(book)
    report = {
        "epub": str(epub_path),
        "output": str(output),
        "generated_at": dt.datetime.now(dt.timezone.utc).astimezone().isoformat(timespec="seconds"),
        "raw_files": len(raw_files),
        "manifest_items": len(manifest),
        "documents": sum(1 for item in manifest if item["type"] == "document"),
        "images": sum(1 for item in manifest if item["type"] == "image"),
        "styles": sum(1 for item in manifest if item["type"] == "style"),
        "sections": len(sections),
        "chapter_sections": len(chapter_sections),
        "chapter_numbers_covered": sorted(
            {number for section in chapter_sections for number in section["chapter_numbers"]}
        ),
        "notes": len(note_records),
        "image_references_in_documents": sum(section["image_count"] for section in sections),
    }
    write_json(output / "metadata" / "metadata.json", metadata)
    write_json(output / "metadata" / "manifest.json", manifest)
    write_json(output / "metadata" / "toc.json", toc)
    write_json(output / "metadata" / "toc_flat.json", flat_toc)
    write_json(output / "metadata" / "spine.json", spine_docs)
    write_json(output / "metadata" / "notes.json", note_records)
    write_json(output / "metadata" / "extraction_report.json", report)

    print(
        "done: "
        f'{report["sections"]} sections, '
        f'{report["chapter_sections"]} chapter documents, '
        f'{len(report["chapter_numbers_covered"])} chapter numbers, '
        f'{report["notes"]} notes, '
        f'{report["images"]} image files -> {output}',
        flush=True,
    )
    return 0


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--epub",
        default="books/红楼梦 (曹雪芹, 高鹗, 程伟元).epub",
        help="Path to the EPUB file.",
    )
    parser.add_argument("--out", default="downloads/epub_hongloumeng")
    return parser.parse_args(argv)


if __name__ == "__main__":
    raise SystemExit(run(parse_args(sys.argv[1:])))
