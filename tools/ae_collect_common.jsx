/* Run in a tool-owned, separate AE instance: AfterFX.exe -m -r "absolute-native-path\tools\ae_collect_common.jsx".
 * Only this collector's new project is touched. Output stays in artifacts/. */
(function () {
    var root = File($.fileName).parent.parent;
    var out = new Folder(root.fsName + "/artifacts/ae-common/18.0.1");
    out.create();
    function json(v) {
        if (v === null || v === undefined) return "null";
        if (typeof v === "string") return '"' + v.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\r/g, "\\r").replace(/\n/g, "\\n").replace(/\t/g, "\\t") + '"';
        if (typeof v === "number") return isFinite(v) ? String(v) : "null";
        if (typeof v === "boolean") return v ? "true" : "false";
        var a = [], k;
        if (v instanceof Array) { for (k = 0; k < v.length; k++) a.push(json(v[k])); return "[" + a.join(",") + "]"; }
        for (k in v) if (v.hasOwnProperty(k)) a.push(json(k) + ":" + json(v[k]));
        return "{" + a.join(",") + "}";
    }
    function write(name, value) { var f = new File(out.fsName + "/" + name); f.encoding = "UTF-8"; if (!f.open("w")) throw Error("Cannot write " + name); f.write(json(value)); f.close(); }
    var report = {version: app.version, expectedVersion: "18.0.1", effects: [], errors: []};
    try {
        app.beginSuppressDialogs();
        app.newProject();
        app.project.bitsPerChannel = 8;
        app.project.workingSpace = "sRGB IEC61966-2.1";
        app.project.linearizeWorkingSpace = false;
        var comp = app.project.items.addComp("Motion Studio reference", 256, 256, 1, 4, 30);
        comp.motionBlur = false;
        var layer = comp.layers.addSolid([0.5, 0.5, 0.5], "reference", 256, 256, 1, 4);
        var inventory = [], i, e;
        for (i = 0; i < app.effects.length; i++) { e = app.effects[i]; inventory.push({name:e.displayName, matchName:e.matchName, category:e.category, version:e.version}); }
        write("inventory.json", inventory);
        var targets = [
            ["brightness_contrast", "ADBE Brightness & Contrast 2", "ADBE Brightness & Contrast2"],
            ["exposure", "ADBE Exposure2"], ["hue_saturation", "ADBE HUE SATURATION"],
            ["tint", "ADBE Tint"], ["tritone", "ADBE Tritone"], ["color_balance", "ADBE Color Balance 2"],
            ["levels", "ADBE Easy Levels2"], ["curves", "ADBE CurvesCustom"],
            ["gaussian_blur", "ADBE Gaussian Blur 2"], ["fast_box_blur", "ADBE Box Blur2"],
            ["directional_blur", "ADBE Motion Blur"], ["radial_blur", "ADBE Radial Blur"],
            ["sharpen", "ADBE Sharpen"], ["unsharp_mask", "ADBE Unsharp Mask2"],
            ["mirror", "ADBE Mirror"], ["offset", "ADBE Offset"], ["bulge", "ADBE Bulge"],
            ["twirl", "ADBE Twirl"], ["wave_warp", "ADBE Wave Warp"], ["polar_coordinates", "ADBE Polar Coordinates"],
            ["invert", "ADBE Invert"], ["black_white", "ADBE Black&White"],
            ["channel_mixer", "ADBE CHANNEL MIXER"], ["posterize", "ADBE Posterize"],
            ["threshold", "ADBE Threshold2"], ["find_edges", "ADBE Find Edges"],
            ["emboss", "ADBE Emboss"], ["color_emboss", "ADBE Color Emboss"],
            ["mosaic", "ADBE Mosaic"], ["turbulent_displace", "ADBE Turbulent Displace"],
            ["gradient_ramp", "ADBE Ramp"], ["fill", "ADBE Fill"],
            ["linear_wipe", "ADBE Linear Wipe"], ["radial_wipe", "ADBE Radial Wipe"],
            ["venetian_blinds", "ADBE Venetian Blinds"], ["fractal_noise", "ADBE Fractal Noise"]
        ];
        function inspect(p) {
            var r = {name:p.name, matchName:p.matchName, children:[]};
            try { r.valueType = Number(p.propertyValueType); r.animatable = p.canVaryOverTime; r.units = p.unitsText; r.defaultValue = p.value; r.min = p.hasMin ? p.minValue : null; r.max = p.hasMax ? p.maxValue : null; } catch (_) {}
            try { for (var j = 1; j <= p.numProperties; j++) r.children.push(inspect(p.property(j))); } catch (_) {}
            return r;
        }
        for (i = 0; i < targets.length; i++) {
            var t = targets[i], found = null;
            for (var j = 1; j < t.length && !found; j++) if (layer.property("ADBE Effect Parade").canAddProperty(t[j])) found = t[j];
            if (!found) { report.errors.push({id:t[0], error:"matchName not found", candidates:t}); continue; }
            var fx = layer.property("ADBE Effect Parade").addProperty(found);
            var record = inspect(fx); record.id = t[0]; report.effects.push(record); fx.remove();
        }
        var rq = app.project.renderQueue.items.add(comp);
        report.outputTemplates = rq.outputModule(1).templates;
        rq.remove();
        app.project.save(new File(out.fsName + "/reference.aep"));
        write("parameters.json", report);
        app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
        app.endSuppressDialogs(false);
        app.exitCode = 0;
    } catch (err) { report.errors.push({error:String(err), line:err.line}); write("parameters.json", report); app.exitCode = 1; }
    app.exitAfterLaunchAndEval = true;
}());
