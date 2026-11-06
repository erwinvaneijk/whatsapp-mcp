"""Parity harness: run every case in cases.json against an MCP server and
compare the results with the recorded golden files. See README.md.

    uv run --locked python run.py                  # compare (exit 1 on a difference)
    uv run --locked python run.py --update         # record new golden files
    uv run --locked python run.py --profile no_fts # one profile only
"""

import argparse
import asyncio
import difflib
import fnmatch
import json
import os
import re
import socket
import sqlite3
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
FIXTURE = HERE / "fixture"
TOKEN = "parity-test-token-0123456789abcdef"  # 32+ characters, like a real one
DEFAULT_TZ = "Europe/Amsterdam"
RUST_SUFFIX = ".rust"  # golden/<profile>/<case>.rust.json

# How to start each implementation. The command gets PARITY_STORE_DIR (the
# folder with messages.db and whatsapp.db) and PARITY_BRIDGE_URL (the bridge
# API base, ending in /api) and must serve MCP over stdio.
SERVERS = {
    "python": [
        "uv",
        "run",
        "--locked",
        "--quiet",
        "--directory",
        str(REPO / "whatsapp-mcp-server"),
        "python",
        str(HERE / "adapters" / "python_server.py"),
    ],
    "rust": [sys.executable, str(HERE / "adapters" / "rust_server.py")],
}

# Default fake-bridge answers, per API path, when a case does not set one.
DEFAULT_BRIDGE_RESPONSES = {
    "/api/send": {"status": 200, "json": {"success": True, "message": "Message sent"}},
    "/api/react": {"status": 200, "json": {"success": True, "message": "Reaction sent"}},
    "/api/download": {
        "status": 500,
        "json": {"success": False, "message": "Failed to download media: no media"},
    },
}


class Workspace:
    """A temporary folder with the test databases, media folder and config file."""

    def __init__(self, root: Path, profile: dict):
        self.root = root
        self.store = root / "store"
        self.media = root / "media"
        self.store.mkdir()
        self.media.mkdir()
        self._messages_db(fts=profile.get("fts", True))
        self._run_sql(self.store / "whatsapp.db", (FIXTURE / "whatsmeow.sql").read_text("utf-8"))
        (self.media / "photo.jpg").write_bytes(b"\xff\xd8\xff\xe0 parity test image")
        (self.media / "voice.ogg").write_bytes(b"OggS parity test audio")
        (root / "outside.txt").write_text("outside the media folder\n")
        self.config = root / "config.yaml"
        self.config.write_text(profile.get("config", ""))
        self.config.chmod(0o600)

    def _messages_db(self, fts: bool):
        schema = (FIXTURE / "schema.sql").read_text("utf-8")
        statements = [s for s in schema.split("\n\n") if not s.startswith("--")]
        if not fts:
            # As left by a bridge built without FTS5: no index, no triggers.
            statements = [s for s in statements if "messages_fts" not in s]
        script = "\n".join(statements) + "\n" + (FIXTURE / "messages.sql").read_text("utf-8")
        self._run_sql(self.store / "messages.db", script)

    @staticmethod
    def _run_sql(path: Path, script: str):
        conn = sqlite3.connect(path)
        try:
            conn.executescript(script)
            conn.commit()
        finally:
            conn.close()

    def placeholders(self) -> list[tuple[str, str]]:
        """Real path -> placeholder, longest first, also for symlinked temp folders."""
        pairs = []
        for path, name in (
            (self.store, "<STORE>"),
            (self.media, "<MEDIA>"),
            (self.root, "<WORKSPACE>"),
        ):
            pairs.append((str(path), name))
            pairs.append((os.path.realpath(path), name))
        return sorted(set(pairs), key=lambda p: -len(p[0]))

    def fill(self, value):
        """Replace placeholders in case arguments and bridge answers with real paths."""
        return _map_strings(
            value,
            lambda s: (
                s.replace("<STORE>", str(self.store))
                .replace("<MEDIA>", str(self.media))
                .replace("<WORKSPACE>", str(self.root))
            ),
        )

    def blank(self, value):
        """Replace real paths in results with placeholders."""

        def replace(s):
            for path, name in self.placeholders():
                s = s.replace(path, name)
            return s

        return _map_strings(value, replace)


def _map_strings(value, func):
    if isinstance(value, str):
        return func(value)
    if isinstance(value, list):
        return [_map_strings(v, func) for v in value]
    if isinstance(value, dict):
        return {k: _map_strings(v, func) for k, v in value.items()}
    return value


class FakeBridge:
    """Records the requests the MCP server sends and answers as told."""

    def __init__(self, workspace: Workspace):
        self.workspace = workspace
        self.requests: list[dict] = []
        self.answer: dict | None = None
        bridge = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
                token = self.headers.get("X-Bridge-Token")
                record = {
                    "method": "POST",
                    "path": self.path,
                    "token": "missing" if token is None else ("ok" if token == TOKEN else "wrong"),
                    "content_type": (self.headers.get("Content-Type") or "").split(";")[0],
                }
                try:
                    record["json"] = json.loads(body)
                except ValueError:
                    record["body"] = body.decode("utf-8", "replace")
                bridge.requests.append(record)

                answer = bridge.workspace.fill(
                    bridge.answer
                    or DEFAULT_BRIDGE_RESPONSES.get(
                        self.path, {"status": 404, "body": "not found\n"}
                    )
                )
                if "json" in answer:
                    # Like Go's json.Encoder: compact, newline at the end.
                    data = (json.dumps(answer["json"], separators=(",", ":")) + "\n").encode()
                    content_type = "application/json"
                else:
                    data = answer.get("body", "").encode()
                    content_type = "text/plain; charset=utf-8"
                self.send_response(answer["status"])
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def log_message(self, *args):
                pass

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}/api"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


def closed_port_url() -> str:
    """A bridge URL where nothing listens."""
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    return f"http://127.0.0.1:{port}/api"


def dump(model) -> dict:
    data = model.model_dump(mode="json", by_alias=True, exclude_none=True)
    data.pop("_meta", None)
    data.pop("meta", None)
    return data


def normalize(value, rules: list[dict]):
    for rule in rules:
        value = _map_strings(value, lambda s, r=rule: re.sub(r["pattern"], r["replace"], s))
    return value


async def run_profile(server_cmd: list[str], name: str, profile: dict) -> dict[str, dict]:
    """Start one server for the profile, run its cases; returns file name -> content."""
    with tempfile.TemporaryDirectory(prefix=f"parity-{name}-") as tmp:
        ws = Workspace(Path(tmp), profile)
        bridge = FakeBridge(ws) if profile.get("bridge") != "down" else None
        bridge_url = bridge.url if bridge else closed_port_url()

        env = {
            k: v
            for k, v in os.environ.items()
            if not k.startswith("WHATSAPP_") and k != "VIRTUAL_ENV"
        }
        env.update(
            {
                "TZ": profile.get("tz", DEFAULT_TZ),
                "WHATSAPP_MCP_CONFIG": str(ws.config),
                "PARITY_STORE_DIR": str(ws.store),
                "PARITY_BRIDGE_URL": bridge_url,
                "PYTHONDONTWRITEBYTECODE": "1",
            }
        )
        if profile.get("token", True):
            env["WHATSAPP_BRIDGE_TOKEN"] = TOKEN
        if profile.get("media_dir", True):
            env["WHATSAPP_MEDIA_DIR"] = str(ws.media)
        env.update(profile.get("env", {}))

        rules = profile.get("normalize", [])
        files: dict[str, dict] = {}
        params = StdioServerParameters(command=server_cmd[0], args=server_cmd[1:], env=env)
        with open(ws.root / "server.log", "w") as errlog:
            try:
                async with stdio_client(params, errlog=errlog) as (r, w), ClientSession(r, w) as s:
                    init = await s.initialize()
                    tools = sorted(
                        (dump(t) for t in (await s.list_tools()).tools), key=lambda t: t["name"]
                    )
                    files["_server"] = {
                        "name": init.server_info.name,
                        "instructions": init.instructions,
                        "tools": tools,
                    }
                    for case_name, case in profile.get("cases", {}).items():
                        if bridge:
                            bridge.requests.clear()
                            bridge.answer = case.get("bridge")
                        result = await asyncio.wait_for(
                            s.call_tool(case["tool"], ws.fill(case["args"])), 60
                        )
                        record = {
                            "tool": case["tool"],
                            "args": case["args"],
                            "result": dump(result),
                            "bridge_requests": bridge.requests[:] if bridge else [],
                        }
                        files[case_name] = normalize(ws.blank(record), rules)
            finally:
                if bridge:
                    bridge.close()
            errlog.flush()
        if not files:
            log = (ws.root / "server.log").read_text(errors="replace")
            raise RuntimeError(f"profile {name}: server did not start\n{log}")
        return files


def to_text(data) -> str:
    return json.dumps(data, indent=2, ensure_ascii=False, sort_keys=True) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--server", default="python", choices=sorted(SERVERS), help="implementation to test"
    )
    parser.add_argument(
        "--server-cmd", help="command that starts the server, instead of --server (split on spaces)"
    )
    parser.add_argument("--profile", action="append", help="run only this profile (repeatable)")
    parser.add_argument(
        "--update", action="store_true", help="write the results as the new golden files"
    )
    parser.add_argument("--golden", type=Path, default=HERE / "golden", help="golden files folder")
    parser.add_argument(
        "--only",
        action="append",
        metavar="PATTERN",
        help="compare (or record) only the files whose name matches, such as _server or "
        "'list_*' (repeatable); the other cases still run",
    )
    args = parser.parse_args()

    cases = json.loads((HERE / "cases.json").read_text("utf-8"))
    profiles = cases["profiles"]
    selected = args.profile or list(profiles)
    unknown = set(selected) - set(profiles)
    if unknown:
        parser.error(f"unknown profile: {', '.join(sorted(unknown))}")
    server_cmd = args.server_cmd.split() if args.server_cmd else SERVERS[args.server]

    differences = 0
    total = 0

    def wanted(stem: str) -> bool:
        return not args.only or any(fnmatch.fnmatchcase(stem, p) for p in args.only)

    # A case marked "rust_differs" has its own golden file for the Rust
    # server, <case>.rust.json: there it behaves better on purpose (see
    # docs/rust-1.0/interface.md, section 5). All other files are shared.
    # --server-cmd only changes how the server starts, not which golden files
    # apply: name the implementation with --server.
    rust = args.server == "rust"

    def golden_name(profile: dict, stem: str) -> str:
        differs = profile.get("cases", {}).get(stem, {}).get("rust_differs")
        return f"{stem}{RUST_SUFFIX}" if rust and differs else stem

    def shared(path: Path) -> bool:
        return not path.stem.endswith(RUST_SUFFIX)

    for name in selected:
        files = asyncio.run(run_profile(server_cmd, name, profiles[name]))
        files = {stem: data for stem, data in files.items() if wanted(stem)}
        folder = args.golden / name
        if args.update:
            folder.mkdir(parents=True, exist_ok=True)
            if rust:
                # Only the Rust-specific files; the shared ones come from Python.
                written = 0
                for stem, data in files.items():
                    if golden_name(profiles[name], stem) != stem:
                        path = folder / f"{golden_name(profiles[name], stem)}.json"
                        path.write_text(to_text(data), "utf-8")
                        written += 1
                print(f"{name}: recorded {written} Rust-specific files")
                continue
            for old in folder.glob("*.json"):
                if shared(old) and old.stem not in files and wanted(old.stem):
                    old.unlink()
            for stem, data in files.items():
                (folder / f"{stem}.json").write_text(to_text(data), "utf-8")
            print(f"{name}: recorded {len(files)} files")
            continue
        for stem, data in files.items():
            total += 1
            path = folder / f"{golden_name(profiles[name], stem)}.json"
            want = path.read_text("utf-8") if path.exists() else ""
            got = to_text(data)
            if want != got:
                differences += 1
                print(f"DIFF {name}/{stem}" + ("" if want else " (no golden file)"))
                sys.stdout.writelines(
                    difflib.unified_diff(
                        want.splitlines(True),
                        got.splitlines(True),
                        f"golden/{name}/{golden_name(profiles[name], stem)}.json",
                        "now",
                    )
                )
        missing = {p.stem for p in folder.glob("*.json") if shared(p) and wanted(p.stem)} - set(
            files
        )
        for stem in sorted(missing):
            differences += 1
            print(f"DIFF {name}/{stem}: golden file has no case any more")
    if not args.update:
        print(f"{total - differences} of {total} match" if differences else f"all {total} match")
    return 1 if differences else 0


if __name__ == "__main__":
    sys.exit(main())
