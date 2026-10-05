"""Render a captured queue with explicit frame tokens, then decode every output.
Uses an exclusive, tool-owned AE instance when --reuse is requested. Never attaches
to an AE process that was already open. Pillow is required to validate references.
"""
import argparse
import json
import subprocess
import ctypes
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from PIL import Image
from ae_reference_io import read_ae_tiff


def ae_processes():
    command = "@(Get-Process AfterFX* -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id) | ConvertTo-Json -Compress"
    result = subprocess.check_output(["powershell", "-NoProfile", "-Command", command], text=True).strip()
    value = json.loads(result) if result else []
    return set(value if isinstance(value, list) else [value])


def hide_owned(processes):
    callback_type = ctypes.WINFUNCTYPE(ctypes.c_bool, ctypes.c_void_p, ctypes.c_void_p)
    def callback(handle, _):
        pid = ctypes.c_ulong()
        ctypes.windll.user32.GetWindowThreadProcessId(handle, ctypes.byref(pid))
        if pid.value in processes:
            ctypes.windll.user32.ShowWindow(handle, 0)
        return True
    ctypes.windll.user32.EnumWindows(callback_type(callback), 0)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("aerender", type=Path)
    parser.add_argument("root", type=Path)
    parser.add_argument("--reuse", action="store_true")
    parser.add_argument("--start",type=int,default=1)
    parser.add_argument("--end",type=int,default=0)
    parser.add_argument("--decode-only",action="store_true",help="Validate and decode an already rendered queue")
    parser.add_argument("--jobs",type=int,default=1,choices=[1,2],help="Independent aerender processes; not usable with --reuse")
    parser.add_argument("--skip-existing",action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    cases = json.loads((root / "cases.json").read_text(encoding="utf-8"))
    if args.reuse and args.jobs != 1:
        raise ValueError("--reuse requires --jobs 1")
    if args.reuse and not args.decode_only and ae_processes():
        raise RuntimeError("An AE instance is already open; exclusive reference rendering was not started")
    owned = set()
    outputs = []
    selected = [(i,c) for i,c in enumerate(cases,1) if i>=args.start and (not args.end or i<=args.end)]
    def render(item):
        nonlocal owned
        index, case = item
        if args.reuse and not args.decode_only and ae_processes() != owned:
            raise RuntimeError("AE process ownership changed; stopping before any further project access")
        expected = root / ("ae-cli-" + case["id"] + "-00000.tif")
        arguments = [str(args.aerender), "-project", str(root / "render-references.aep"), "-rqindex", str(index),
                     "-output", str(root / ("ae-cli-" + case["id"] + "-[#####].tif"))]
        if args.reuse:
            arguments.append("-reuse")
        log = root / ("cli-" + case["id"] + ".log")
        startup = subprocess.STARTUPINFO()
        startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = 0
        existing = False
        if args.skip_existing and expected.exists():
            try:
                with read_ae_tiff(expected) as image: existing = image.size == (case["width"],case["height"])
            except Exception: pass
        if not args.decode_only and not existing:
            expected.unlink(missing_ok=True)
            with log.open("wb") as stream:
                result = subprocess.run(arguments, stdout=stream, stderr=subprocess.STDOUT, startupinfo=startup, timeout=180)
        if args.reuse and not args.decode_only:
            current = ae_processes()
            if not owned:
                owned = current
                if len(owned) > 1:
                    raise RuntimeError("Cannot establish exclusive ownership of the render instance")
            hide_owned(owned)
        # Some AE installations return code 1 after writing a valid frame. Retain
        # the exit code/log as evidence; unreadable or missing outputs always fail.
        with read_ae_tiff(expected) as rgba:
            if rgba.size != (case["width"], case["height"]):
                raise RuntimeError("Unexpected AE reference dimensions")
            png = root / ("ae-" + case["id"] + ".png")
            rgba.save(png)
        return dict(id=case["id"], file=png.name, raw_file=expected.name, exit_code=None if args.decode_only or existing else result.returncode, log=log.name, alpha="premultiplied_srgb_black_to_straight")
    with ThreadPoolExecutor(max_workers=args.jobs) as executor:
        futures = [executor.submit(render,item) for item in selected]
        for future in as_completed(futures):
            try:
                outputs.append(future.result())
            except Exception as error:
                for pending in futures: pending.cancel()
                (root / "render-progress.json").write_text(json.dumps(dict(state="failed",completed=len(outputs),cases=len(cases),error=str(error))),encoding="utf-8")
                raise
            outputs.sort(key=lambda x:x["id"])
            (root / "renders.json").write_text(json.dumps(dict(version="18.0.1", outputs=outputs), indent=2), encoding="utf-8")
            (root / "render-progress.json").write_text(json.dumps(dict(state="rendering", completed=len(outputs), selected=len(selected), cases=len(cases))), encoding="utf-8")
            if len(outputs) % 5 == 0 or len(outputs) == len(selected):
                print(f"Validated {len(outputs)}/{len(selected)} selected AE outputs", flush=True)
    (root / "render-progress.json").write_text(json.dumps(dict(state="complete" if len(outputs)==len(cases) else "partial",completed=len(outputs),cases=len(cases))), encoding="utf-8")


if __name__ == "__main__":
    main()
