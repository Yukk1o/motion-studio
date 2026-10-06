"""Compare decoded AE and motion-studio frames; publish evidence without changing acceptance."""
import argparse
import json
import shutil
import hashlib
from pathlib import Path
from PIL import Image, ImageChops, ImageFilter, ImageStat

ROOT = Path(__file__).resolve().parents[1]
REFERENCE = ROOT / "crates/aem-effects/reference"
ALGORITHMS = {
    "brightness_contrast": "亮度混合黑/白，对比度绕0.5缩放；旧版使用加法亮度。",
    "exposure": "曝光以2的幂缩放，叠加偏移与gamma；支持RGB分量与旁路线性解码。",
    "hue_saturation": "RGB/HSL转换，主色相、饱和度、明度和着色。独立颜色范围不支持。",
    "tint": "0.299/0.587/0.114亮度在黑白映射颜色间插值，按映射量混合。",
    "tritone": "按亮度分段插值阴影、中间调与高光，再混合原图。",
    "color_balance": "按亮度产生阴影/中间调/高光权重，RGB加性偏移；保留亮度为HSL近似。",
    "levels": "按输入黑白点归一化、gamma映射和输出黑白点重映射；支持RGB和Alpha分量。",
    "curves": "宿主对主曲线和RGBA五通道生成256项LUT；关键帧在LUT空间插值。",
    "gaussian_blur": "分离横/纵两pass高斯采样，工作空间直通RGB按Alpha加权。有限采样核。",
    "fast_box_blur": "按迭代次数交替横/纵盒式采样，最多6pass；0迭代为恒等。",
    "directional_blur": "沿指定角度用对称有限采样核平均，长度单位为图层像素。",
    "radial_blur": "绕中心旋转或径向缩放取样，质量档控制有限采样数。",
    "sharpen": "中心与四邻域构建锐化核，RGB截断，保留中心Alpha。",
    "unsharp_mask": "采样局部模糊后按差值阈值和数量增强；核与AE不同。",
    "mirror": "按反射中心和角度反射一侧的图层像素坐标。",
    "offset": "按偏移后的中心平移采样，重复边缘并与原图混合。",
    "bulge": "椭圆半径内按高度对归一化距离进行径向重映射；抗锯齿控制不支持。",
    "twirl": "半径内按中心距离衰减角度，旋转图层像素坐标。",
    "wave_warp": "波形、方向、宽高、相位、时间速度产生位移；钉住和抗锯齿选项不支持。",
    "polar_coordinates": "矩形/极坐标映射按插值量混合，中心使用图层几何中心。",
}


def metrics(a, b):
    difference = ImageChops.difference(a, b)
    alpha = ImageChops.lighter(a.getchannel("A"), b.getchannel("A"))
    visible = alpha.point(lambda x: 255 if x else 0)
    low = alpha.filter(ImageFilter.MinFilter(33))
    high = alpha.filter(ImageFilter.MaxFilter(33))
    transitions = ImageChops.difference(low, high).point(lambda x: 255 if x else 0)
    w, h = a.size
    border = Image.new("L", (w, h))
    border.putdata([255 if x < 16 or y < 16 or x >= w-16 or y >= h-16 else 0 for y in range(h) for x in range(w)])
    edge = ImageChops.lighter(border, transitions)
    masks = {"whole": Image.new("L", a.size, 255), "interior": ImageChops.invert(edge), "edge": edge}
    result = {}
    for name, mask in masks.items():
        rgb_mask = ImageChops.darker(mask, visible)
        rgb_count = rgb_mask.histogram()[255]
        alpha_count = mask.histogram()[255]
        rgb = sum(ImageStat.Stat(difference.convert("RGB"), rgb_mask).mean)/3 if rgb_count else 0
        av = ImageStat.Stat(difference.getchannel("A"), mask).mean[0] if alpha_count else 0
        result[name] = dict(rgb_mae_255=rgb, alpha_mae_255=av, rgb_pixels=rgb_count, alpha_pixels=alpha_count,
                            threshold_pass=bool(alpha_count and rgb <= 3 and av <= 3))
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--publish", action="store_true")
    parser.add_argument("--package",type=Path,default=ROOT/"artifacts/ae-library.msfx",help="Built-in package emitted by effect_tool, for provenance")
    args = parser.parse_args()
    root = args.root.resolve()
    cases = json.loads((root / "cases.json").read_text(encoding="utf-8"))
    manifest = json.loads((ROOT / "crates/aem-effects/library/manifest.json").read_text(encoding="utf-8"))
    parameters = json.loads((REFERENCE / "ae2021-parameters.json").read_text(encoding="utf-8"))
    inventory = json.loads((root / "inventory.json").read_text(encoding="utf-8")) if (root / "inventory.json").exists() else []
    package_hash = hashlib.sha256(args.package.read_bytes()).hexdigest() if args.package.exists() else None
    records = []
    for case in cases:
        record = dict(case=case, status="missing_reference", metrics=None)
        ae = root / ("ae-" + case["id"] + ".png")
        motion = root / ("motion-" + case["id"] + ".png")
        record["input"] = "fixtures/"+case["input"]
        record["input_sha256"] = hashlib.sha256((root/case["input"]).read_bytes()).hexdigest()
        if ae.exists() and motion.exists():
            try:
                with Image.open(ae) as ai, Image.open(motion) as mi:
                    a, b = ai.convert("RGBA"), mi.convert("RGBA")
                    if a.size != (case["width"], case["height"]) or b.size != a.size:
                        raise ValueError("Reference dimensions mismatch")
                    record["metrics"] = metrics(a, b)
                    record["ae_output"] = "outputs/ae/"+ae.name
                    record["application_output"] = "outputs/motion/"+motion.name
                    record["ae_output_sha256"] = hashlib.sha256(ae.read_bytes()).hexdigest()
                    record["application_output_sha256"] = hashlib.sha256(motion.read_bytes()).hexdigest()
                    record["status"] = "threshold_pass" if all(x["threshold_pass"] for x in record["metrics"].values()) else "approximate_difference"
            except Exception as error:
                record.update(status="invalid_reference", error=str(error))
        records.append(record)
        if args.publish and record["metrics"]:
            for source, folder in [(ae, "ae"), (motion, "motion")]:
                destination = REFERENCE / "outputs" / folder / source.name
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, destination)
    report = dict(baseline=dict(application=parameters["version"], bpc=8, working_space="sRGB IEC61966-2.1", linearize=False,
                               square_pixels=True, motion_blur=False, rgb_alpha_mae_threshold_255=3),
                  roi="edge: 16-pixel image border or Alpha transition within 16 pixels; RGB ignores pixels transparent in both images",
                  accepted_effects=0, acceptance_note="Static cases do not cover boundary values, parameter animation, custom AE Curves or physical-device performance. No automatic promotion.",
                  cases=len(cases), decoded_pairs=sum(x["metrics"] is not None for x in records), results=records)
    (root / "comparison.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    if args.publish:
        reports = REFERENCE / "reports"; reports.mkdir(exist_ok=True)
        (reports / "ae-comparison.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
        record_dir = REFERENCE / "records"; record_dir.mkdir(exist_ok=True)
        matrix = ["# AE 2021 效果兼容矩阵", "", "基准来自本机 AE 18.0.1x1。已验收数量为 **0**；阈值通过仅表示当前静态样本通过，不能代替完整验收。参数原始采集与逐项报告在 `crates/aem-effects/reference`。", "", "| 效果 | matchName | 状态 | 已对照 / 用例 | 超阈值用例 |", "|---|---|---|---|---|"]
        for effect in manifest["effects"]:
            selected = [x for x in records if x["case"]["effect"] == effect["id"]]
            native = next(x for x in parameters["effects"] if x["id"] == effect["id"])
            version = next((x["version"] for x in inventory if x["matchName"] == effect["reference_match_name"]), "not_captured")
            evidence = dict(effect=effect["id"], display_name=effect["name"], english_name=effect["english_name"], compatibility="approximate",
                            ae_match_name=effect["reference_match_name"], ae_application=parameters["version"], ae_effect_version=version,
                            package=dict(id=manifest["id"], version=manifest["version"], sha256=package_hash), params=effect["params"], native_parameter_snapshot=native,
                            algorithm=ALGORITHMS[effect["id"]], shader_passes=effect["passes"], working_space=effect["working_space"], alpha_mode=effect["alpha_mode"],
                            edge_mode=effect["edge_mode"], edge_param=effect.get("edge_param"), output_padding=effect["padding"], output_range="RGBA8: [0,1]",
                            known_differences=effect["known_differences"], cases=selected,
                            acceptance_gaps=["参数边界与动画的AE参考", "叠加/摄影机AE参考", "物理Android设备耗时/峰值内存"] + (["AE枚举菜单名称采集"] if any(p['kind']=='enum' for p in effect['params']) else []) + (["AE私有曲线数据的自定义曲线参考"] if effect["id"] == "curves" else []))
            (record_dir / (effect["id"] + ".json")).write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding="utf-8")
            count = sum(x["metrics"] is not None for x in selected)
            fail = sum(x["status"] == "approximate_difference" for x in selected)
            matrix.append(f"| {effect['name']} / {effect['english_name']} | `{effect['reference_match_name']}` | 近似实现 | {count}/{len(selected)} | {fail} |")
        local_matrix = ROOT / "docs/effects/compatibility.md"
        local_matrix.parent.mkdir(parents=True, exist_ok=True)
        local_matrix.write_text("\n".join(matrix)+"\n", encoding="utf-8")
    print(json.dumps(dict(cases=len(cases), decoded_pairs=report["decoded_pairs"], accepted_effects=0)))


if __name__ == "__main__":
    main()
