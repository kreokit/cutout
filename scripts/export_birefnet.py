#!/usr/bin/env python3
"""Exports BiRefNet_lite to ONNX for KreoKit Cutout's "Best" model, reproducibly.

Runs in .github/workflows/export-birefnet.yml (pinned versions in that file):

    python scripts/export_birefnet.py --out dist --photos fixtures/photos [--reference v1.onnx]

Weights: ZhengPeng7/BiRefNet_lite on Hugging Face (MIT), pinned to REVISION; the model code
comes from the same revision.

1. Tries ONE file with a dynamic input size (named `height`/`width` axes), so the app can run
   it at 1024, 768 or 640.
2. Checks every exported file with ONNX Runtime against PyTorch, at each size it claims to
   support, on the fixture photos (the app's preprocessing). A trace that silently baked one
   size in fails here.
3. If the dynamic file fails, exports fixed 768 and 640 files instead (1024 stays the
   models-v1 file).

Writes <out>/*.onnx, <out>/SHA256SUMS, <out>/report.json and <out>/report.md.
"""

import argparse
import gc
import hashlib
import json
import sys
import traceback
from pathlib import Path

import numpy as np
import onnxruntime as ort
import torch
from PIL import Image

REPO = "ZhengPeng7/BiRefNet_lite"
REVISION = "aa62cd87eafb9cc43056d08ef3615a14628b831d"
SIZES = [1024, 768, 640]
OPSET = 17
MEAN = np.array([0.485, 0.456, 0.406], dtype=np.float32)
STD = np.array([0.229, 0.224, 0.225], dtype=np.float32)
# ONNX Runtime must match PyTorch this closely (sigmoid outputs) for a file to pass.
MAX_MEAN_DIFF = 0.005
MIN_IOU = 0.995


def load_model() -> torch.nn.Module:
    from transformers import AutoModelForImageSegmentation

    model = AutoModelForImageSegmentation.from_pretrained(REPO, revision=REVISION, trust_remote_code=True)
    model.eval().float()

    class Final(torch.nn.Module):
        """Only the final prediction (logits), as the app reads output 0."""

        def __init__(self, m):
            super().__init__()
            self.m = m

        def forward(self, x):
            return self.m(x)[-1]

    # The wrapper itself must be in eval mode too: torch.onnx.export restores the top
    # module's mode afterwards, recursively.
    return Final(model).eval()


def register_deform_conv() -> None:
    """The author's route (tutorials/BiRefNet_pth2onnx.ipynb): deform_conv2d_onnx_exporter
    (MIT), with their patch for unknown sizes."""
    import deform_conv2d_onnx_exporter as exporter

    source = Path(exporter.__file__).read_text()
    if "KREOKIT PATCH" not in source:
        source = source.replace(
            "return sym_help._get_tensor_dim_size(tensor, dim)",
            """# KREOKIT PATCH (from the BiRefNet tutorial)
    tensor_dim_size = sym_help._get_tensor_dim_size(tensor, dim)
    if tensor_dim_size == None and (dim == 2 or dim == 3):
        import typing
        from torch import _C
        x_type = typing.cast(_C.TensorType, tensor.type())
        x_strides = x_type.strides()
        tensor_dim_size = x_strides[2] if dim == 3 else x_strides[1] // x_strides[2]
    elif tensor_dim_size == None and (dim == 0):
        import typing
        from torch import _C
        x_type = typing.cast(_C.TensorType, tensor.type())
        x_strides = x_type.strides()
        tensor_dim_size = x_strides[3]
    return tensor_dim_size""",
        )
        Path(exporter.__file__).write_text(source)
        import importlib

        importlib.reload(exporter)
    exporter.register_deform_conv2d_onnx_op()


def export(model: torch.nn.Module, path: Path, size: int, dynamic: bool) -> None:
    dummy = torch.randn(1, 3, size, size)
    torch.onnx.export(
        model,
        dummy,
        str(path),
        opset_version=OPSET,
        input_names=["input_image"],
        output_names=["output_image"],
        dynamic_axes={"input_image": {2: "height", 3: "width"}, "output_image": {2: "height", 3: "width"}}
        if dynamic
        else None,
        dynamo=False,
    )
    model.eval()


def preprocess(photo: Path, size: int) -> np.ndarray:
    """Like the app (cutout.rs): Lanczos to size², /255, ImageNet mean/std, 1×3×H×W."""
    img = Image.open(photo).convert("RGB").resize((size, size), Image.LANCZOS)
    x = (np.asarray(img, dtype=np.float32) / 255.0 - MEAN) / STD
    return x.transpose(2, 0, 1)[None].astype(np.float32)


def sigmoid(x: np.ndarray) -> np.ndarray:
    return 1.0 / (1.0 + np.exp(-x))


def iou(a: np.ndarray, b: np.ndarray) -> float:
    a, b = a >= 0.5, b >= 0.5
    union = np.logical_or(a, b).sum()
    return float(np.logical_and(a, b).sum() / union) if union else 1.0


def session(path: Path) -> ort.InferenceSession:
    """Lean settings (as the app will use), so checks fit in the runner's memory."""
    options = ort.SessionOptions()
    options.enable_cpu_mem_arena = False
    options.enable_mem_pattern = False
    return ort.InferenceSession(str(path), options, providers=["CPUExecutionProvider"])


def check(model, path: Path, sizes: list[int], photos: list[Path]) -> dict:
    """ONNX Runtime vs PyTorch at each size; raises if any size fails."""
    session_ = session(path)
    results = {}
    for size in sizes:
        diffs, ious = [], []
        for photo in photos:
            x = preprocess(photo, size)
            with torch.no_grad():
                want = sigmoid(model(torch.from_numpy(x)).numpy())
            got = sigmoid(session_.run(None, {session_.get_inputs()[0].name: x})[0])
            if got.shape != want.shape:
                raise AssertionError(f"{size}: output {got.shape}, PyTorch {want.shape}")
            diffs.append(float(np.abs(got - want).mean()))
            ious.append(iou(got, want))
        results[size] = {"mean_diff": max(diffs), "min_iou": min(ious)}
        print(f"  {path.name} at {size}: mean diff {max(diffs):.5f}, IoU {min(ious):.4f}", flush=True)
        if max(diffs) > MAX_MEAN_DIFF or min(ious) < MIN_IOU:
            raise AssertionError(f"{size}: ONNX Runtime differs from PyTorch ({results[size]})")
    return results


def against_reference(path: Path, reference: Path, photos: list[Path]) -> float:
    """IoU of a new file at 1024 against the models-v1 file (same weights, same size)."""
    new = session(path)
    old = session(reference)
    worst = 1.0
    for photo in photos:
        x = preprocess(photo, 1024)
        a = sigmoid(new.run(None, {new.get_inputs()[0].name: x})[0])
        b = sigmoid(old.run(None, {old.get_inputs()[0].name: x})[0])
        worst = min(worst, iou(a, b))
    return worst


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--photos", type=Path, required=True)
    parser.add_argument("--reference", type=Path, help="the models-v1 birefnet_lite.onnx")
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    photos = sorted(p for p in args.photos.iterdir() if p.suffix.lower() in {".jpg", ".jpeg", ".png", ".webp"})
    report = {"repo": REPO, "revision": REVISION, "opset": OPSET, "torch": torch.__version__,
              "onnxruntime": ort.__version__, "photos": [p.name for p in photos], "attempts": []}

    model = load_model()
    register_deform_conv()
    files: list[Path] = []

    dynamic = args.out / "birefnet_lite_dynamic.onnx"
    print("Exporting with a dynamic input size…", flush=True)
    try:
        export(model, dynamic, 1024, dynamic=True)
        results = check(model, dynamic, SIZES, photos)
        report["attempts"].append({"file": dynamic.name, "sizes": SIZES, "ok": True, "check": results})
        files.append(dynamic)
    except Exception as e:  # noqa: BLE001 — any failure means: use fixed sizes
        traceback.print_exc()
        report["attempts"].append({"file": dynamic.name, "sizes": SIZES, "ok": False, "error": repr(e)[:2000]})
        dynamic.unlink(missing_ok=True)
    # Outside the except block: its traceback would keep the failed check's session (GBs) alive.
    gc.collect()

    if dynamic not in files:
        print("Dynamic export failed the check; exporting fixed 768 and 640 files.", flush=True)
        for size in (768, 640):
            path = args.out / f"birefnet_lite_{size}.onnx"
            try:
                export(model, path, size, dynamic=False)
                results = check(model, path, [size], photos)
                report["attempts"].append({"file": path.name, "sizes": [size], "ok": True, "check": results})
                files.append(path)
            except Exception as e:  # noqa: BLE001
                traceback.print_exc()
                report["attempts"].append({"file": path.name, "sizes": [size], "ok": False, "error": repr(e)[:2000]})
                path.unlink(missing_ok=True)
            gc.collect()

    if args.reference and dynamic in files:
        report["iou_vs_v1_at_1024"] = against_reference(dynamic, args.reference, photos)
        print(f"Dynamic file at 1024 vs models-v1: IoU {report['iou_vs_v1_at_1024']:.4f}", flush=True)

    report["files"] = {f.name: {"bytes": f.stat().st_size, "sha256": sha256(f)} for f in files}
    (args.out / "SHA256SUMS").write_text("".join(f"{v['sha256']}  {k}\n" for k, v in report["files"].items()))
    (args.out / "report.json").write_text(json.dumps(report, indent=2))
    lines = [f"# BiRefNet_lite export\n\n{REPO} @ {REVISION}, opset {OPSET}, torch {torch.__version__}, "
             f"onnxruntime {ort.__version__}\n\n| File | Sizes | Result |\n|---|---|---|"]
    for a in report["attempts"]:
        result = "ok" if a["ok"] else f"failed: {a['error'][:200]}"
        lines.append(f"| {a['file']} | {', '.join(map(str, a['sizes']))} | {result} |")
    (args.out / "report.md").write_text("\n".join(lines) + "\n")
    print((args.out / "report.md").read_text())
    return 0 if files else 1


if __name__ == "__main__":
    sys.exit(main())
