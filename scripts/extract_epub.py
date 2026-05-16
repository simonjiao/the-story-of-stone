#!/usr/bin/env python3
"""Extract a generic EPUB into a normalized source snapshot."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import posixpath
import re
import shutil
import sys
import zipfile
from html.parser import HTMLParser
from pathlib import Path
from typing import Any
from urllib.parse import unquote, urldefrag
from xml.etree import ElementTree as ET


DOCUMENT_MEDIA_TYPES = {"application/xhtml+xml", "text/html", "application/xml"}
ASSET_MEDIA_PREFIXES = ("image/", "font/", "audio/", "video/")
BLOCK_TAGS = {
    "address",
    "article",
    "aside",
    "blockquote",
    "caption",
    "dd",
    "div",
    "dt",
    "figcaption",
    "figure",
    "footer",
    "header",
    "li",
    "main",
    "nav",
    "p",
    "pre",
    "section",
    "td",
    "th",
}
HEADING_TAGS = {"h1", "h2", "h3", "h4", "h5", "h6"}
SKIP_TAGS = {"head", "script", "style"}


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


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def slugify(value: str, fallback: str = "source") -> str:
    value = value.strip().replace("\u3000", " ")
    value = re.sub(r"\s+", "_", value)
    value = re.sub(r'[\\/:*?"<>|#%&{}$!`@+=，。；：、（）()\[\]]', "_", value)
    value = re.sub(r"_+", "_", value).strip("._ ")
    return value[:120].strip("._ ") or fallback


def strip_ns(tag: str) -> str:
    return tag.rsplit("}", 1)[-1] if "}" in tag else tag


def xml_text(element: ET.Element | None) -> str:
    if element is None:
        return ""
    return "".join(element.itertext()).strip()


def decode_bytes(data: bytes) -> str:
    for encoding in ("utf-8-sig", "utf-8", "gb18030"):
        try:
            return data.decode(encoding)
        except UnicodeDecodeError:
            continue
    return data.decode("utf-8", errors="replace")


def normalize_spaces(value: str) -> str:
    value = value.replace("\xa0", " ").replace("\u3000", " ")
    value = re.sub(r"[ \t\r\f\v]+", " ", value)
    value = re.sub(r" *\n+ *", "\n", value)
    return value.strip()


def resolve_epub_path(current_name: str, href: str) -> str:
    href = (href or "").strip()
    if not href:
        return ""
    href, _fragment = urldefrag(unquote(href))
    if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", href):
        return href
    if not href:
        return current_name
    if href.startswith("/"):
        return posixpath.normpath(href.lstrip("/"))
    return posixpath.normpath(posixpath.join(posixpath.dirname(current_name), href)).lstrip("/")


def join_epub_path(base_dir: str, href: str) -> str:
    return posixpath.normpath(posixpath.join(base_dir, unquote(href))).lstrip("/")


class HtmlBlockParser(HTMLParser):
    def __init__(self, current_name: str) -> None:
        super().__init__(convert_charrefs=True)
        self.current_name = current_name
        self.blocks: list[dict[str, Any]] = []
        self.assets: list[dict[str, str]] = []
        self.links: list[dict[str, str]] = []
        self._parts: list[str] = []
        self._current_tag = "p"
        self._current_kind = "paragraph"
        self._skip_depth = 0
        self._link_stack: list[dict[str, Any]] = []
        self._ruby_stack: list[dict[str, Any]] = []
        self._pending_rare_char_annotations: list[dict[str, str]] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        tag = tag.lower()
        attrs_dict = {key.lower(): value or "" for key, value in attrs}
        if tag in SKIP_TAGS:
            self._skip_depth += 1
            return
        if self._skip_depth:
            return
        if tag == "ruby":
            self._ruby_stack.append(
                {
                    "base_parts": [],
                    "pronunciation_parts": [],
                    "in_rt": False,
                    "rp_depth": 0,
                }
            )
            return
        if self._ruby_stack and tag == "rt":
            self._ruby_stack[-1]["in_rt"] = True
            return
        if self._ruby_stack and tag == "rp":
            self._ruby_stack[-1]["rp_depth"] += 1
            return
        if tag in HEADING_TAGS:
            self._flush()
            self._current_tag = tag
            self._current_kind = "heading"
            return
        if tag in BLOCK_TAGS:
            self._flush()
            self._current_tag = tag
            self._current_kind = "paragraph"
            return
        if tag == "br":
            self._parts.append("\n")
            return
        if tag == "img":
            src = resolve_epub_path(self.current_name, attrs_dict.get("src", ""))
            alt = attrs_dict.get("alt", "")
            if src:
                self.assets.append({"src": src, "alt": alt})
                self._parts.append(f" [[asset:{src}]] ")
            return
        if tag == "a":
            href = resolve_epub_path(self.current_name, attrs_dict.get("href", ""))
            self._link_stack.append({"href": href, "text": []})

    def handle_endtag(self, tag: str) -> None:
        tag = tag.lower()
        if tag in SKIP_TAGS and self._skip_depth:
            self._skip_depth -= 1
            return
        if self._skip_depth:
            return
        if self._ruby_stack and tag == "rt":
            self._ruby_stack[-1]["in_rt"] = False
            return
        if self._ruby_stack and tag == "rp":
            self._ruby_stack[-1]["rp_depth"] = max(self._ruby_stack[-1]["rp_depth"] - 1, 0)
            return
        if self._ruby_stack and tag == "ruby":
            ruby = self._ruby_stack.pop()
            glyph = normalize_spaces("".join(ruby["base_parts"]))
            pronunciation = normalize_spaces("".join(ruby["pronunciation_parts"]))
            rendered = f"{glyph}（{pronunciation}）" if glyph and pronunciation else glyph or pronunciation
            if self._ruby_stack:
                self._add_ruby_text(rendered)
            else:
                self._parts.append(rendered)
                if glyph:
                    self._pending_rare_char_annotations.append(
                        {
                            "glyph": glyph,
                            "pronunciation": pronunciation,
                            "source": "ruby",
                            "rendered": rendered,
                        }
                    )
            return
        if tag == "a" and self._link_stack:
            link = self._link_stack.pop()
            text = normalize_spaces("".join(link["text"]))
            if link["href"] or text:
                self.links.append({"href": link["href"], "text": text})
            return
        if tag in HEADING_TAGS or tag in BLOCK_TAGS:
            self._flush()

    def handle_data(self, data: str) -> None:
        if self._skip_depth:
            return
        if self._ruby_stack:
            self._add_ruby_text(data)
            for link in self._link_stack:
                link["text"].append(data)
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
            self._pending_rare_char_annotations = []
            return
        block = {
            "index": len(self.blocks) + 1,
            "tag": self._current_tag,
            "kind": self._current_kind,
            "text": text,
        }
        if self._pending_rare_char_annotations:
            block["rare_char_annotations"] = self._pending_rare_char_annotations
            self._pending_rare_char_annotations = []
        self.blocks.append(block)

    def _add_ruby_text(self, data: str) -> None:
        if not self._ruby_stack:
            self._parts.append(data)
            return
        ruby = self._ruby_stack[-1]
        if ruby["rp_depth"]:
            return
        if ruby["in_rt"]:
            ruby["pronunciation_parts"].append(data)
        else:
            ruby["base_parts"].append(data)


def read_container_rootfile(zf: zipfile.ZipFile) -> str:
    try:
        data = zf.read("META-INF/container.xml")
    except KeyError as exc:
        raise ValueError("EPUB is missing META-INF/container.xml") from exc
    root = ET.fromstring(data)
    for element in root.iter():
        if strip_ns(element.tag) == "rootfile":
            full_path = element.attrib.get("full-path", "").strip()
            if full_path:
                return full_path
    raise ValueError("EPUB container.xml does not define a rootfile")


def parse_opf(zf: zipfile.ZipFile, opf_path: str) -> dict[str, Any]:
    root = ET.fromstring(zf.read(opf_path))
    opf_dir = posixpath.dirname(opf_path)

    metadata: dict[str, list[dict[str, Any]]] = {}
    manifest: dict[str, dict[str, Any]] = {}
    spine: list[dict[str, Any]] = []
    spine_toc_id = ""

    for element in root.iter():
        name = strip_ns(element.tag)
        if name == "metadata":
            for child in list(element):
                key = strip_ns(child.tag)
                metadata.setdefault(key, []).append(
                    {
                        "text": xml_text(child),
                        "attributes": dict(child.attrib),
                    }
                )
        elif name == "item":
            item_id = element.attrib.get("id", "")
            href = element.attrib.get("href", "")
            if not item_id or not href:
                continue
            file_name = join_epub_path(opf_dir, href)
            manifest[item_id] = {
                "id": item_id,
                "href": href,
                "file": file_name,
                "media_type": element.attrib.get("media-type", ""),
                "properties": element.attrib.get("properties", ""),
            }
        elif name == "spine":
            spine_toc_id = element.attrib.get("toc", "")
        elif name == "itemref":
            spine.append(
                {
                    "idref": element.attrib.get("idref", ""),
                    "linear": element.attrib.get("linear", "yes"),
                    "properties": element.attrib.get("properties", ""),
                }
            )

    return {
        "opf_path": opf_path,
        "opf_dir": opf_dir,
        "metadata": metadata,
        "manifest": manifest,
        "spine": spine,
        "spine_toc_id": spine_toc_id,
    }


def first_metadata(metadata: dict[str, list[dict[str, Any]]], key: str) -> str:
    values = metadata.get(key, [])
    if not values:
        return ""
    return str(values[0].get("text", "")).strip()


def parse_ncx_toc(zf: zipfile.ZipFile, ncx_path: str) -> list[dict[str, Any]]:
    try:
        root = ET.fromstring(zf.read(ncx_path))
    except KeyError:
        return []

    def parse_nav_point(element: ET.Element, depth: int) -> dict[str, Any]:
        label = ""
        src = ""
        for child in list(element):
            name = strip_ns(child.tag)
            if name == "navLabel":
                label = xml_text(child)
            elif name == "content":
                src = resolve_epub_path(ncx_path, child.attrib.get("src", ""))
        children = [
            parse_nav_point(child, depth + 1)
            for child in list(element)
            if strip_ns(child.tag) == "navPoint"
        ]
        return {
            "id": element.attrib.get("id", ""),
            "label": label,
            "src": src,
            "depth": depth,
            "children": children,
        }

    nav_map = next((element for element in root.iter() if strip_ns(element.tag) == "navMap"), None)
    if nav_map is None:
        return []
    return [parse_nav_point(element, 1) for element in list(nav_map) if strip_ns(element.tag) == "navPoint"]


def parse_nav_links(zf: zipfile.ZipFile, nav_path: str) -> list[dict[str, str]]:
    try:
        text = decode_bytes(zf.read(nav_path))
    except KeyError:
        return []
    parser = HtmlBlockParser(nav_path)
    parser.feed(text)
    parser.close()
    return parser.links


def render_document_md(document: dict[str, Any]) -> str:
    lines = [f"# {document['title']}", "", f"- Source file: `{document['file']}`", ""]
    for block in document["blocks"]:
        text = block["text"]
        if block["kind"] == "heading":
            lines.append(f"## {text}")
        else:
            lines.append(text)
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def render_document_txt(document: dict[str, Any]) -> str:
    lines = [document["title"], f"Source file: {document['file']}", ""]
    for block in document["blocks"]:
        lines.append(block["text"])
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def build_snapshot(args: argparse.Namespace) -> int:
    epub_path = Path(args.epub).expanduser().resolve()
    if not epub_path.exists():
        raise FileNotFoundError(epub_path)
    if not zipfile.is_zipfile(epub_path):
        raise ValueError(f"not a valid EPUB/ZIP file: {epub_path}")

    source_id = slugify(args.source_id or epub_path.stem, "epub-source")
    output_root = Path(args.out).expanduser().resolve() / source_id
    if output_root.exists() and any(output_root.iterdir()):
        if not args.overwrite:
            raise FileExistsError(f"{output_root} already exists; pass --overwrite to replace it")
        shutil.rmtree(output_root)
    ensure_dir(output_root)

    input_hash = sha256_file(epub_path)
    extracted_at = now_iso()

    with zipfile.ZipFile(epub_path) as zf:
        opf_path = read_container_rootfile(zf)
        package = parse_opf(zf, opf_path)
        metadata = package["metadata"]
        manifest = package["manifest"]
        spine = package["spine"]

        title = args.title or first_metadata(metadata, "title") or epub_path.stem
        language = args.language or first_metadata(metadata, "language")
        creator = first_metadata(metadata, "creator")

        nav_item = next(
            (item for item in manifest.values() if "nav" in item.get("properties", "").split()),
            None,
        )
        ncx_item = manifest.get(package["spine_toc_id"], {})
        toc = {
            "nav": parse_nav_links(zf, nav_item["file"]) if nav_item else [],
            "ncx": parse_ncx_toc(zf, ncx_item.get("file", "")) if ncx_item else [],
        }

        documents: list[dict[str, Any]] = []
        blocks: list[dict[str, Any]] = []
        assets: list[dict[str, Any]] = []

        for spine_index, itemref in enumerate(spine, start=1):
            item = manifest.get(itemref["idref"])
            if not item:
                continue
            media_type = item.get("media_type", "")
            if media_type not in DOCUMENT_MEDIA_TYPES:
                continue
            try:
                html_text = decode_bytes(zf.read(item["file"]))
            except KeyError:
                continue
            parser = HtmlBlockParser(item["file"])
            parser.feed(html_text)
            parser.close()
            first_heading = next((block["text"] for block in parser.blocks if block["kind"] == "heading"), "")
            document_title = first_heading or item["href"] or item["id"]
            section_id = f"{source_id}:section:{spine_index:04d}"
            document_blocks = []
            for block in parser.blocks:
                block_id = f"{section_id}:block:{block['index']:04d}"
                block_record = {
                    "source_id": source_id,
                    "section_id": section_id,
                    "block_id": block_id,
                    "section_index": spine_index,
                    "block_index": block["index"],
                    "kind": block["kind"],
                    "tag": block["tag"],
                    "text": block["text"],
                    "source_file": item["file"],
                }
                if block.get("rare_char_annotations"):
                    block_record["rare_char_annotations"] = [
                        {
                            **annotation,
                            "block_id": block_id,
                            "source_file": item["file"],
                        }
                        for annotation in block["rare_char_annotations"]
                    ]
                blocks.append(block_record)
                document_blocks.append(block_record)
            documents.append(
                {
                    "source_id": source_id,
                    "section_id": section_id,
                    "section_index": spine_index,
                    "item_id": item["id"],
                    "file": item["file"],
                    "title": document_title,
                    "media_type": media_type,
                    "linear": itemref["linear"],
                    "links": parser.links,
                    "assets": parser.assets,
                    "blocks": document_blocks,
                }
            )

        if args.copy_assets:
            for item in manifest.values():
                media_type = item.get("media_type", "")
                if media_type in DOCUMENT_MEDIA_TYPES or media_type.endswith("ncx"):
                    continue
                if not (media_type.startswith(ASSET_MEDIA_PREFIXES) or media_type == "text/css"):
                    continue
                try:
                    data = zf.read(item["file"])
                except KeyError:
                    continue
                target = output_root / "assets" / item["file"]
                ensure_dir(target.parent)
                target.write_bytes(data)
                assets.append(
                    {
                        "id": item["id"],
                        "source_file": item["file"],
                        "snapshot_path": str(target.relative_to(output_root)),
                        "media_type": media_type,
                        "bytes": len(data),
                    }
                )

    source = {
        "source_id": source_id,
        "source_category": args.source_category,
        "format": "epub",
        "title": title,
        "work": args.work or title,
        "edition": args.edition,
        "language": language,
        "creator": creator,
        "input_path": str(epub_path),
        "input_sha256": input_hash,
        "extracted_at": extracted_at,
        "source_url": args.source_url,
        "license": args.license,
        "license_url": args.license_url,
        "license_source_url": args.license_source_url,
        "attribution": args.attribution,
        "usage_boundary": args.usage_boundary,
        "notes": args.notes,
    }
    report = {
        "source_id": source_id,
        "output": str(output_root),
        "documents": len(documents),
        "blocks": len(blocks),
        "assets": len(assets),
        "rare_char_annotations": sum(len(block.get("rare_char_annotations", [])) for block in blocks),
        "manifest_items": len(package["manifest"]),
        "spine_items": len(package["spine"]),
    }

    manifest_records = list(package["manifest"].values())
    spine_records = [
        {
            **itemref,
            "file": package["manifest"].get(itemref["idref"], {}).get("file", ""),
            "media_type": package["manifest"].get(itemref["idref"], {}).get("media_type", ""),
        }
        for itemref in package["spine"]
    ]

    write_json(output_root / "metadata" / "source.json", source)
    write_json(output_root / "metadata" / "metadata.json", package["metadata"])
    write_json(output_root / "metadata" / "manifest.json", manifest_records)
    write_json(output_root / "metadata" / "spine.json", spine_records)
    write_json(output_root / "metadata" / "toc.json", toc)
    write_json(output_root / "metadata" / "assets.json", assets)
    write_json(output_root / "metadata" / "extraction_report.json", report)
    write_jsonl(output_root / "documents" / "documents.jsonl", documents)
    write_jsonl(output_root / "documents" / "blocks.jsonl", blocks)
    write_text(
        output_root / "combined" / "all_sections.txt",
        "\n\n".join(render_document_txt(doc).rstrip() for doc in documents) + "\n",
    )
    write_text(
        output_root / "combined" / "all_sections.md",
        "\n\n".join(render_document_md(doc).rstrip() for doc in documents) + "\n",
    )

    print(
        f"done: source_id={source_id} documents={report['documents']} "
        f"blocks={report['blocks']} assets={report['assets']} -> {output_root}",
        flush=True,
    )
    return 0


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("epub", help="Path to the EPUB file.")
    parser.add_argument(
        "--out",
        default="resources/sources/epub",
        help="Output root. The source_id subdirectory is created under it.",
    )
    parser.add_argument("--source-id", help="Stable source id. Defaults to the EPUB file stem.")
    parser.add_argument(
        "--source-category",
        default="base_material",
        choices=(
            "base_material",
            "extended_base_material",
            "research_material",
            "style_material",
            "evaluation_material",
        ),
    )
    parser.add_argument("--title", help="Override source title.")
    parser.add_argument("--work", help="Override work name.")
    parser.add_argument("--edition", default="", help="Edition or version label.")
    parser.add_argument("--language", help="Override language.")
    parser.add_argument("--source-url", default="", help="Canonical source landing page URL.")
    parser.add_argument("--license", default="", help="Machine-readable source license id.")
    parser.add_argument("--license-url", default="", help="Canonical license URL.")
    parser.add_argument(
        "--license-source-url",
        default="",
        help="URL proving or explaining the source license.",
    )
    parser.add_argument("--attribution", default="", help="Required attribution text.")
    parser.add_argument("--usage-boundary", default="", help="Production usage boundary.")
    parser.add_argument("--notes", default="", help="Human-readable source notes.")
    parser.add_argument("--overwrite", action="store_true", help="Replace an existing output directory.")
    parser.add_argument(
        "--no-assets",
        dest="copy_assets",
        action="store_false",
        help="Do not copy image/CSS/font assets.",
    )
    parser.set_defaults(copy_assets=True)
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    return build_snapshot(args)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
