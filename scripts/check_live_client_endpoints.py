#!/usr/bin/env python3
"""探测本机 League Live Client Data API 可用性。"""

from __future__ import annotations

import argparse
import json
import ssl
import sys
import urllib.error
import urllib.request
from dataclasses import dataclass


DEFAULT_BASE_URL = "https://127.0.0.1:2999"
DEFAULT_TIMEOUT_SECONDS = 2.0
BODY_PREVIEW_CHAR_LIMIT = 300
ENDPOINTS = [
    "/liveclientdata/allgamedata",
    "/liveclientdata/activeplayer",
    "/liveclientdata/activeplayername",
    "/liveclientdata/playerlist",
    "/liveclientdata/gamestats",
    "/liveclientdata/eventdata",
]


@dataclass
class ProbeResult:
    endpoint: str
    ok: bool
    status_code: int | None
    body_length: int | None
    body_preview: str | None
    error: str | None


def summarize_body(body: str) -> str:
    compact = " ".join(body.split())
    if len(compact) <= BODY_PREVIEW_CHAR_LIMIT:
        return compact
    return compact[:BODY_PREVIEW_CHAR_LIMIT] + "..."


def build_ssl_context() -> ssl.SSLContext:
    context = ssl.create_default_context()
    context.check_hostname = False
    context.verify_mode = ssl.CERT_NONE
    return context


def probe_endpoint(base_url: str, endpoint: str, timeout_seconds: float) -> ProbeResult:
    url = f"{base_url.rstrip('/')}{endpoint}"
    request = urllib.request.Request(url, method="GET")
    context = build_ssl_context()
    try:
        with urllib.request.urlopen(request, context=context, timeout=timeout_seconds) as response:
            body = response.read().decode("utf-8", errors="replace")
            return ProbeResult(
                endpoint=endpoint,
                ok=True,
                status_code=response.status,
                body_length=len(body),
                body_preview=summarize_body(body),
                error=None,
            )
    except urllib.error.HTTPError as error:
        body = error.read().decode("utf-8", errors="replace")
        return ProbeResult(
            endpoint=endpoint,
            ok=False,
            status_code=error.code,
            body_length=len(body),
            body_preview=summarize_body(body),
            error=f"HTTP {error.code}: {error.reason}",
        )
    except urllib.error.URLError as error:
        return ProbeResult(
            endpoint=endpoint,
            ok=False,
            status_code=None,
            body_length=None,
            body_preview=None,
            error=f"连接失败: {error.reason}",
        )
    except TimeoutError:
        return ProbeResult(
            endpoint=endpoint,
            ok=False,
            status_code=None,
            body_length=None,
            body_preview=None,
            error="连接超时",
        )
    except Exception as error:  # noqa: BLE001
        return ProbeResult(
            endpoint=endpoint,
            ok=False,
            status_code=None,
            body_length=None,
            body_preview=None,
            error=f"未预期异常: {error}",
        )


def print_human_readable(results: list[ProbeResult]) -> None:
    print("Live Client 接口探测结果")
    print("=" * 32)
    for result in results:
        print(f"接口: {result.endpoint}")
        print(f"状态: {'可用' if result.ok else '不可用'}")
        print(f"HTTP: {result.status_code if result.status_code is not None else '-'}")
        print(f"长度: {result.body_length if result.body_length is not None else '-'}")
        print(f"错误: {result.error or '-'}")
        print(f"摘要: {result.body_preview or '-'}")
        print("-" * 32)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="探测 League Live Client Data API 可用性")
    parser.add_argument(
        "--base-url",
        default=DEFAULT_BASE_URL,
        help=f"基础地址，默认 {DEFAULT_BASE_URL}",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=DEFAULT_TIMEOUT_SECONDS,
        help=f"单个请求超时秒数，默认 {DEFAULT_TIMEOUT_SECONDS}",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="输出 JSON 结果",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    results = [
        probe_endpoint(args.base_url, endpoint, args.timeout)
        for endpoint in ENDPOINTS
    ]
    if args.json:
        print(
            json.dumps(
                [result.__dict__ for result in results],
                ensure_ascii=False,
                indent=2,
            )
        )
    else:
        print_human_readable(results)

    return 0


if __name__ == "__main__":
    sys.exit(main())
