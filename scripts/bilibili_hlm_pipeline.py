#!/usr/bin/env python3
"""Download Bilibili videos and extract audio/text for a space list."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any


DEFAULT_SPACE_URL = "https://space.bilibili.com/558777092/lists"
DEFAULT_MID = 558777092
DEFAULT_QUERY = "红楼梦"
DEFAULT_ASR_PROMPT = (
    "《红楼梦》文本细读。用简体中文。专名按原著写：宝黛钗、钗黛、脂批、回目、原著、读者、诸君。"
)
ASR_PROMPT_GLOSSARY_CHAR_LIMIT = 60
ASR_HOTWORDS_CHAR_LIMIT = 80
USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"
)


@dataclass(frozen=True)
class Archive:
    index: int
    aid: int
    bvid: str
    title: str
    duration: int
    pubdate: int
    url: str


def http_json(url: str, params: dict[str, Any] | None = None, retries: int = 3) -> Any:
    if params:
        url = f"{url}?{urllib.parse.urlencode(params)}"

    headers = {
        "User-Agent": USER_AGENT,
        "Referer": DEFAULT_SPACE_URL,
        "Accept": "application/json,text/plain,*/*",
    }
    request = urllib.request.Request(url, headers=headers)

    last_error: Exception | None = None
    for attempt in range(retries):
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                payload = response.read().decode("utf-8")
            data = json.loads(payload)
            if data.get("code") not in (0, None):
                raise RuntimeError(f"Bilibili API error {data.get('code')}: {data.get('message')}")
            return data
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError, RuntimeError) as exc:
            last_error = exc
            if attempt + 1 < retries:
                time.sleep(1.5 * (attempt + 1))
    raise RuntimeError(f"failed to fetch {url}: {last_error}")


def http_text(url: str) -> str:
    if url.startswith("//"):
        url = "https:" + url
    headers = {"User-Agent": USER_AGENT, "Referer": DEFAULT_SPACE_URL}
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=30) as response:
        return response.read().decode("utf-8")


def extract_mid(url: str | None) -> int:
    if not url:
        return DEFAULT_MID
    match = re.search(r"space\.bilibili\.com/(\d+)", url)
    return int(match.group(1)) if match else DEFAULT_MID


def ensure_tools(require_video: bool) -> None:
    missing = []
    if shutil.which("yt-dlp") is None:
        missing.append("yt-dlp")
    if require_video and shutil.which("ffmpeg") is None:
        missing.append("ffmpeg")
    if missing:
        raise SystemExit(f"missing required tool(s): {', '.join(missing)}")


def list_collections(mid: int) -> dict[str, Any]:
    data = http_json(
        "https://api.bilibili.com/x/polymer/web-space/seasons_series_list",
        {"mid": mid, "page_num": 1, "page_size": 20},
    )
    return data["data"]["items_lists"]


def find_collection(
    lists: dict[str, Any], query: str, season_id: int | None, series_id: int | None
) -> tuple[str, dict[str, Any]]:
    seasons = lists.get("seasons_list") or []
    series = lists.get("series_list") or []

    if season_id is not None:
        for item in seasons:
            if item.get("meta", {}).get("season_id") == season_id:
                return "season", item
        raise SystemExit(f"season_id {season_id} was not found")

    if series_id is not None:
        for item in series:
            if item.get("meta", {}).get("series_id") == series_id:
                return "series", item
        raise SystemExit(f"series_id {series_id} was not found")

    needle = query.casefold()
    for kind, items in (("season", seasons), ("series", series)):
        for item in items:
            meta = item.get("meta", {})
            haystack = " ".join(
                str(meta.get(key, "")) for key in ("title", "name", "description")
            ).casefold()
            if needle in haystack:
                return kind, item

    if seasons:
        return "season", seasons[0]
    if series:
        return "series", series[0]
    raise SystemExit("no seasons or series found for this space")


def fetch_archives(mid: int, kind: str, collection_id: int) -> list[Archive]:
    archives: list[dict[str, Any]] = []
    page_num = 1
    page_size = 30

    while True:
        if kind == "season":
            endpoint = "https://api.bilibili.com/x/polymer/web-space/seasons_archives_list"
            params = {
                "mid": mid,
                "season_id": collection_id,
                "sort_reverse": "false",
                "page_num": page_num,
                "page_size": page_size,
            }
        else:
            endpoint = "https://api.bilibili.com/x/series/archives"
            params = {
                "mid": mid,
                "series_id": collection_id,
                "only_normal": "true",
                "sort": "desc",
                "pn": page_num,
                "ps": page_size,
            }

        data = http_json(endpoint, params)
        block = data.get("data") or {}
        page = block.get("page") or {}
        batch = block.get("archives") or block.get("aids") or []
        archives.extend(batch)

        total = int(page.get("total") or len(archives))
        if len(archives) >= total or not batch:
            break
        page_num += 1

    result: list[Archive] = []
    for idx, item in enumerate(archives, start=1):
        bvid = item["bvid"]
        result.append(
            Archive(
                index=idx,
                aid=int(item["aid"]),
                bvid=bvid,
                title=item["title"],
                duration=int(item.get("duration") or 0),
                pubdate=int(item.get("pubdate") or 0),
                url=f"https://www.bilibili.com/video/{bvid}",
            )
        )
    return result


def safe_stem(archive: Archive) -> str:
    raw = f"{archive.index:03d}_{archive.bvid}_{archive.title}"
    stem = re.sub(r'[\\/:*?"<>|\r\n\t]+', "_", raw)
    stem = re.sub(r"\s+", " ", stem).strip(" .")
    return stem[:150]


def run(cmd: list[str]) -> None:
    print("+ " + " ".join(cmd), flush=True)
    subprocess.run(cmd, check=True)


def complete_file(path: Path) -> bool:
    return path.exists() and path.stat().st_size > 0


def load_glossary_terms(path: Path | None) -> list[str]:
    if not path:
        return []
    terms: list[str] = []
    seen: set[str] = set()
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if "#" in line:
            line = line.split("#", 1)[0].strip()
        for term in re.split(r"[，,、\t]+", line):
            term = term.strip()
            if term and term not in seen:
                seen.add(term)
                terms.append(term)
    return terms


def build_asr_prompt(base_prompt: str, archive: Archive, glossary_terms: list[str]) -> str:
    parts = [base_prompt.strip(), f"当前视频标题：{archive.title}。"]
    if glossary_terms:
        glossary = "、".join(glossary_terms)
        if len(glossary) > ASR_PROMPT_GLOSSARY_CHAR_LIMIT:
            glossary = glossary[:ASR_PROMPT_GLOSSARY_CHAR_LIMIT].rsplit("、", 1)[0]
        parts.append(f"可能出现的专名和固定表达：{glossary}")
    return "\n".join(part for part in parts if part).strip()


def glossary_hotwords(glossary_terms: list[str]) -> str | None:
    if not glossary_terms:
        return None
    hotwords = " ".join(glossary_terms)
    if len(hotwords) > ASR_HOTWORDS_CHAR_LIMIT:
        hotwords = hotwords[:ASR_HOTWORDS_CHAR_LIMIT].rsplit(" ", 1)[0]
    return hotwords


def download_video(archive: Archive, out_dir: Path, cookies: Path | None) -> Path:
    out_dir.mkdir(parents=True, exist_ok=True)
    stem = safe_stem(archive)
    existing = sorted(out_dir.glob(f"{stem}.mp4"))
    if existing and complete_file(existing[0]):
        print(f"Using existing video: {existing[0]}", flush=True)
        return existing[0]

    output_template = str(out_dir / f"{stem}.%(ext)s")
    cmd = [
        "yt-dlp",
        "--no-playlist",
        "--no-progress",
        "--retries",
        "10",
        "--fragment-retries",
        "10",
        "--retry-sleep",
        "5",
        "-f",
        "bestvideo[vcodec^=avc]+bestaudio/bestvideo+bestaudio/best",
        "--merge-output-format",
        "mp4",
        "-o",
        output_template,
        archive.url,
    ]
    if cookies:
        cmd[1:1] = ["--cookies", str(cookies)]
    run(cmd)

    matches = sorted(out_dir.glob(f"{stem}.*"), key=lambda path: path.stat().st_mtime, reverse=True)
    matches = [path for path in matches if not path.name.endswith((".part", ".ytdl"))]
    if not matches:
        raise RuntimeError(f"download finished but no video file matched {stem}")
    return matches[0]


def extract_audio(video_path: Path, audio_dir: Path, stem: str) -> tuple[Path, Path]:
    audio_dir.mkdir(parents=True, exist_ok=True)
    m4a_path = audio_dir / f"{stem}.m4a"
    wav_path = audio_dir / f"{stem}.16k.wav"

    if complete_file(m4a_path):
        print(f"Using existing audio: {m4a_path}", flush=True)
    else:
        run(
            [
                "ffmpeg",
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                str(video_path),
                "-vn",
                "-c:a",
                "aac",
                "-b:a",
                "128k",
                str(m4a_path),
            ]
        )

    if complete_file(wav_path):
        print(f"Using existing ASR wav: {wav_path}", flush=True)
    else:
        run(
            [
                "ffmpeg",
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-i",
                str(video_path),
                "-vn",
                "-ac",
                "1",
                "-ar",
                "16000",
                str(wav_path),
            ]
        )
    return m4a_path, wav_path


def srt_time(seconds: float) -> str:
    millis = int(round(seconds * 1000))
    hours, millis = divmod(millis, 3_600_000)
    minutes, millis = divmod(millis, 60_000)
    secs, millis = divmod(millis, 1000)
    return f"{hours:02d}:{minutes:02d}:{secs:02d},{millis:03d}"


def write_subtitle_files(items: list[dict[str, Any]], txt_path: Path, srt_path: Path) -> None:
    txt_path.write_text("\n".join(item["content"].strip() for item in items if item.get("content")) + "\n", encoding="utf-8")
    lines: list[str] = []
    for idx, item in enumerate(items, start=1):
        content = str(item.get("content", "")).strip()
        if not content:
            continue
        lines.extend(
            [
                str(idx),
                f"{srt_time(float(item['from']))} --> {srt_time(float(item['to']))}",
                content,
                "",
            ]
        )
    srt_path.write_text("\n".join(lines), encoding="utf-8")


def fetch_bilibili_subtitles(archive: Archive, text_dir: Path, stem: str) -> bool:
    view = http_json("https://api.bilibili.com/x/web-interface/view", {"bvid": archive.bvid})
    data = view["data"]
    aid = data["aid"]
    cid = data["cid"]
    player = http_json("https://api.bilibili.com/x/player/v2", {"aid": aid, "cid": cid})
    subtitles = ((player.get("data") or {}).get("subtitle") or {}).get("subtitles") or []
    if not subtitles:
        return False

    text_dir.mkdir(parents=True, exist_ok=True)
    subtitle = subtitles[0]
    body = json.loads(http_text(subtitle["subtitle_url"]))
    items = body.get("body") or []
    if not items:
        return False

    write_subtitle_files(items, text_dir / f"{stem}.txt", text_dir / f"{stem}.srt")
    (text_dir / f"{stem}.subtitle.json").write_text(
        json.dumps(body, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    return True


def transcribe_with_faster_whisper(
    audio_path: Path,
    text_dir: Path,
    stem: str,
    model: str,
    prompt: str,
    hotwords: str | None,
) -> bool:
    try:
        from faster_whisper import WhisperModel  # type: ignore
    except ImportError:
        return False

    text_dir.mkdir(parents=True, exist_ok=True)
    model_obj = WhisperModel(model, device="cpu", compute_type="int8")
    segments, info = model_obj.transcribe(
        str(audio_path),
        language="zh",
        initial_prompt=prompt,
        hotwords=hotwords,
        vad_filter=True,
        beam_size=5,
    )

    subtitle_items: list[dict[str, Any]] = []
    plain_lines: list[str] = []
    for segment in segments:
        text = segment.text.strip()
        if not text:
            continue
        subtitle_items.append({"from": segment.start, "to": segment.end, "content": text})
        plain_lines.append(text)

    (text_dir / f"{stem}.txt").write_text("\n".join(plain_lines) + "\n", encoding="utf-8")
    write_subtitle_files(subtitle_items, text_dir / f"{stem}.txt", text_dir / f"{stem}.srt")
    (text_dir / f"{stem}.transcript.json").write_text(
        json.dumps(
            {
                "engine": "faster-whisper",
                "model": model,
                "language": info.language,
                "language_probability": info.language_probability,
                "initial_prompt": prompt,
                "hotwords": hotwords,
                "segments": subtitle_items,
            },
            ensure_ascii=False,
            indent=2,
        ),
        encoding="utf-8",
    )
    return True


def transcribe_with_whisper_cli(
    audio_path: Path, text_dir: Path, stem: str, model: str, prompt: str
) -> bool:
    if shutil.which("whisper") is None:
        return False
    text_dir.mkdir(parents=True, exist_ok=True)
    run(
        [
            "whisper",
            str(audio_path),
            "--language",
            "Chinese",
            "--model",
            model,
            "--output_dir",
            str(text_dir),
            "--output_format",
            "all",
            "--initial_prompt",
            prompt,
        ]
    )
    base = text_dir / audio_path.stem
    renamed = False
    for suffix in (".txt", ".srt", ".json", ".vtt", ".tsv"):
        src = base.with_suffix(suffix)
        if src.exists():
            src.rename(text_dir / f"{stem}{suffix}")
            renamed = True
    return renamed


def transcribe(audio_path: Path, text_dir: Path, stem: str, model: str, prompt: str) -> bool:
    if all(
        complete_file(text_dir / f"{stem}{suffix}")
        for suffix in (".txt", ".srt", ".transcript.json")
    ):
        print(f"Using existing transcript: {text_dir / f'{stem}.txt'}", flush=True)
        return True

    if transcribe_with_faster_whisper(audio_path, text_dir, stem, model, prompt, None):
        return True
    return transcribe_with_whisper_cli(audio_path, text_dir, stem, model, prompt)


def transcribe_with_context(
    audio_path: Path,
    text_dir: Path,
    stem: str,
    model: str,
    prompt: str,
    hotwords: str | None,
    force: bool,
) -> bool:
    if not force and all(
        complete_file(text_dir / f"{stem}{suffix}")
        for suffix in (".txt", ".srt", ".transcript.json")
    ):
        print(f"Using existing transcript: {text_dir / f'{stem}.txt'}", flush=True)
        return True

    if transcribe_with_faster_whisper(audio_path, text_dir, stem, model, prompt, hotwords):
        return True
    return transcribe_with_whisper_cli(audio_path, text_dir, stem, model, prompt)


def save_metadata(
    output_dir: Path,
    lists: dict[str, Any],
    collection_kind: str,
    collection_meta: dict[str, Any],
    archives: list[Archive],
) -> None:
    metadata_dir = output_dir / "metadata"
    metadata_dir.mkdir(parents=True, exist_ok=True)
    (metadata_dir / "lists.json").write_text(
        json.dumps(lists, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    manifest = {
        "collection_kind": collection_kind,
        "collection": collection_meta,
        "archives": [archive.__dict__ for archive in archives],
    }
    (metadata_dir / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    with (metadata_dir / "manifest.jsonl").open("w", encoding="utf-8") as handle:
        for archive in archives:
            handle.write(json.dumps(archive.__dict__, ensure_ascii=False) + "\n")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("url", nargs="?", default=DEFAULT_SPACE_URL, help="Bilibili space /lists URL")
    parser.add_argument("--mid", type=int, help="Bilibili space mid; overrides URL parsing")
    parser.add_argument("--query", default=DEFAULT_QUERY, help="collection title/name search keyword")
    parser.add_argument("--season-id", type=int, help="download a specific season id")
    parser.add_argument("--series-id", type=int, help="download a specific series id")
    parser.add_argument("--limit", type=int, default=3, help="number of videos to process")
    parser.add_argument("--offset", type=int, default=0, help="skip this many videos before processing")
    parser.add_argument("--output-dir", type=Path, default=Path("resources/styles/buhongjushi"))
    parser.add_argument("--cookies", type=Path, help="yt-dlp cookies.txt for login-only quality/content")
    parser.add_argument("--dry-run", action="store_true", help="only write metadata and print selected videos")
    parser.add_argument("--skip-video", action="store_true", help="do not download video/extract audio")
    parser.add_argument("--skip-transcript", action="store_true", help="do not fetch subtitles or run ASR")
    parser.add_argument("--prefer-asr", action="store_true", help="run ASR instead of fetching Bilibili subtitles")
    parser.add_argument("--force-transcript", action="store_true", help="overwrite existing transcript files")
    parser.add_argument("--asr-model", default="base", help="Whisper/faster-whisper model name")
    parser.add_argument("--asr-prompt", default=DEFAULT_ASR_PROMPT, help="initial prompt for ASR")
    parser.add_argument("--asr-glossary", type=Path, help="newline/comma separated ASR hotword glossary")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    mid = args.mid or extract_mid(args.url)
    ensure_tools(require_video=not args.skip_video and not args.dry_run)

    lists = list_collections(mid)
    kind, collection = find_collection(lists, args.query, args.season_id, args.series_id)
    meta = collection["meta"]
    collection_id = int(meta["season_id"] if kind == "season" else meta["series_id"])
    archives = fetch_archives(mid, kind, collection_id)
    save_metadata(args.output_dir, lists, kind, meta, archives)

    selected = archives[args.offset : args.offset + args.limit]
    glossary_terms = load_glossary_terms(args.asr_glossary)
    hotwords = glossary_hotwords(glossary_terms)
    print(f"Selected {len(selected)} video(s) from {kind}: {meta.get('title') or meta.get('name')}")
    for archive in selected:
        print(f"{archive.index:03d} {archive.bvid} {archive.title}")

    if args.dry_run:
        return 0

    status_path = args.output_dir / "metadata" / "run_status.json"
    status: list[dict[str, Any]] = []
    for archive in selected:
        stem = safe_stem(archive)
        video_path: Path | None = None
        audio_path: Path | None = None
        wav_path: Path | None = None
        transcript = "skipped"

        if not args.skip_video:
            video_path = download_video(archive, args.output_dir / "videos", args.cookies)
            audio_path, wav_path = extract_audio(video_path, args.output_dir / "audio", stem)

        if not args.skip_transcript:
            asr_prompt = build_asr_prompt(args.asr_prompt, archive, glossary_terms)
            transcript_dir = args.output_dir / "transcripts"
            if not args.prefer_asr and fetch_bilibili_subtitles(archive, transcript_dir, stem):
                transcript = "bilibili-subtitle"
            elif wav_path and transcribe_with_context(
                wav_path,
                transcript_dir,
                stem,
                args.asr_model,
                asr_prompt,
                hotwords,
                args.force_transcript,
            ):
                transcript = "asr"
            else:
                transcript = "missing-asr"
                print(
                    "No official subtitle found and no ASR engine is installed. "
                    "Install faster-whisper or openai-whisper, then rerun.",
                    file=sys.stderr,
                )

        status.append(
            {
                "index": archive.index,
                "bvid": archive.bvid,
                "title": archive.title,
                "video": str(video_path) if video_path else None,
                "audio": str(audio_path) if audio_path else None,
                "wav": str(wav_path) if wav_path else None,
                "transcript": transcript,
            }
        )
        status_path.write_text(json.dumps(status, ensure_ascii=False, indent=2), encoding="utf-8")

    status_path.write_text(json.dumps(status, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Wrote run status: {status_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
