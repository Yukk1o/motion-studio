#!/usr/bin/env python3
"""Exercise real desktop windows, both UI languages and the shared MCP session."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading


def verify_mcp(binary: Path, output: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="motion-mcp-") as folder:
        root = Path(folder) / "default"
        report = output / "mcp-ui-smoke.json"
        with (output / "mcp-ui.stderr.log").open("w", encoding="utf-8") as errors:
            options = {"creationflags": subprocess.CREATE_NO_WINDOW} if os.name == "nt" else {}
            process = subprocess.Popen(
                [str(binary), "--mcp-ui", "--smoke", str(report), "--project", str(root), "--locale", "en"],
                stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors,
                text=True, encoding="utf-8", **options,
            )
            received: queue.Queue[str | None] = queue.Queue()

            def read() -> None:
                assert process.stdout
                for line in process.stdout:
                    received.put(line)
                received.put(None)

            threading.Thread(target=read, daemon=True).start()
            sequence = 0

            def send(method: str, params: dict | None = None, notification: bool = False) -> dict:
                nonlocal sequence
                sequence += 1
                message: dict = {"jsonrpc": "2.0", "method": method}
                if not notification:
                    message["id"] = sequence
                if params is not None:
                    message["params"] = params
                assert process.stdin
                process.stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
                process.stdin.flush()
                if notification:
                    return {}
                line = received.get(timeout=20)
                if line is None:
                    raise RuntimeError("desktop stopped before its MCP reply")
                response = json.loads(line)
                if response.get("id") != sequence or "error" in response:
                    raise RuntimeError(f"invalid MCP response: {response}")
                return response["result"]

            try:
                send("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "motion-ci", "version": "1"}})
                send("notifications/initialized", notification=True)
                tools = send("tools/list")["tools"]
                if {tool["name"] for tool in tools} != {"motion_state", "motion_edit", "motion_seek", "motion_history", "motion_save"}:
                    raise RuntimeError("MCP catalogue is incomplete")
                state = send("tools/call", {"name": "motion_state", "arguments": {}})["structuredContent"]
                result = send("tools/call", {"name": "motion_edit", "arguments": {
                    "expectedRevision": state["revision"],
                    "commands": [{"op": "add_shape", "id": 2, "name": "MCP 矩形", "shape": "rectangle", "size": [120, 120], "position": [960, 540, 0]}],
                }})
                if result.get("isError"):
                    raise RuntimeError(f"shared edit failed: {result}")
                result = send("tools/call", {"name": "motion_seek", "arguments": {"frame": 21.5}})
                if result.get("isError") or result["structuredContent"]["frame"] != 21.5:
                    raise RuntimeError("shared fractional seek failed")
                result = send("tools/call", {"name": "motion_save", "arguments": {}})
                if result.get("isError"):
                    raise RuntimeError("shared save failed")
                assert process.stdin
                process.stdin.close()
                if process.wait(timeout=40) != 0:
                    raise RuntimeError("shared-window acceptance failed")
                saved = json.loads((root / "project.json").read_text(encoding="utf-8"))
                if not any(layer["id"] == 2 and layer["name"] == "MCP 矩形" for layer in saved["layers"]):
                    raise RuntimeError("MCP changes were not saved by the shared project")
                snapshot = json.loads(report.read_text(encoding="utf-8"))
                if snapshot["frame"] != 21.5 or not snapshot["desktop"]["sharedGpuDevice"]:
                    raise RuntimeError("GUI and MCP did not keep the same project clock/device")
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=10)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=Path("artifacts/desktop"))
    args = parser.parse_args()
    binary = args.binary.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    for locale in ("en", "zh"):
        report = output / f"smoke-{locale}.json"
        result = subprocess.run([str(binary), "--smoke", str(report), "--locale", locale], capture_output=True, text=True, encoding="utf-8", timeout=40)
        (output / f"smoke-{locale}.stderr.log").write_text(result.stderr, encoding="utf-8")
        if result.returncode:
            raise RuntimeError(f"{locale} desktop acceptance failed: {result.stderr}")
        snapshot = json.loads(report.read_text(encoding="utf-8"))
        if snapshot["locale"] != locale or snapshot["floatingFrames"].get("3", 0) < 2:
            raise RuntimeError(f"{locale} did not validate the floating composition")
    verify_mcp(binary, output)
    print("Desktop languages, native floating windows and shared MCP passed.")


if __name__ == "__main__":
    main()
