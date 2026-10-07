/* Read-only project collector, not an importer or renderer.
 * Run in AE capable of opening the project. For automation use a separate
 * AfterFX.exe -m -r wrapper calling motionStudioExportProject(config).
 * Reads pre-expression values; does not evaluate imported expression strings,
 * save the project, relink footage, change properties or add render-queue items.
 */
function motionStudioExportProject(config) {
    // Output is private metadata: never place it inside the source directory.
    var sourceFile = config.project ? new File(config.project) : app.project && app.project.file;
    if (sourceFile) {
        var sourceDir = sourceFile.parent.fsName.replace(/\\/g, "/").toLowerCase();
        var outputDir = new Folder(config.output).fsName.replace(/\\/g, "/").toLowerCase();
        if (outputDir === sourceDir || outputDir.indexOf(sourceDir + "/") === 0) throw Error("Output must be outside the source directory");
    }
    function json(value) {
        if (value === null || value === undefined) return "null";
        if (typeof value === "string") return '"' + value.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/[\x00-\x1f]/g, function (c) {
            var hex = c.charCodeAt(0).toString(16); return "\\u" + ("0000" + hex).slice(-4);
        }) + '"';
        if (typeof value === "number") return isFinite(value) ? String(value) : "null";
        if (typeof value === "boolean") return value ? "true" : "false";
        var parts = [], i;
        if (value instanceof Array) {
            for (i = 0; i < value.length; i++) parts.push(json(value[i]));
            return "[" + parts.join(",") + "]";
        }
        for (i in value) if (value.hasOwnProperty(i)) parts.push(json(i) + ":" + json(value[i]));
        return "{" + parts.join(",") + "}";
    }
    function write(path, value) {
        var file = new File(path); file.encoding = "UTF-8";
        if (!file.open("w")) throw Error("Cannot write export: " + path);
        try { file.write(json(value)); } finally { file.close(); }
    }
    var report = {schema:"motion-studio-ae-project-1", aeVersion:app.version,
        state:"collecting", rootId:null, compositions:[], footage:[], effects:[],
        reachableCompositionIds:[], warnings:[], errors:[],
        evaluation:"pre_expression_only", restorationReady:false};
    var opened = false, suppressed = false, propertyCount = 0;
    var enumAttributes = {justification:true, blendingMode:true, trackMatteType:true, autoOrient:true,
        quality:true, samplingQuality:true, alphaMode:true, fieldSeparationType:true};
    function read(object, key, record, outputKey) {
        try { var value = object[key]; if (value !== undefined) record[outputKey || key] = enumAttributes[key] ? String(value) : plain(value); }
        catch (_) {} // Attributes absent in this AE version remain absent.
    }
    function attrs(object, keys, record) {
        for (var i = 0; i < keys.length; i++) read(object, keys[i], record);
    }
    function plain(value) {
        var i, result;
        if (value === null || value === undefined) return null;
        if (typeof value === "number" || typeof value === "string" || typeof value === "boolean") return value;
        if (value instanceof Array) {
            result = []; for (i = 0; i < value.length; i++) result.push(plain(value[i])); return result;
        }
        if (value instanceof Shape) {
            result = {type:"shape"}; attrs(value, ["vertices", "inTangents", "outTangents", "closed",
                "featherSegLocs", "featherRelSegLocs", "featherRadii", "featherInterps", "featherTensions",
                "featherTypes", "featherRelCornerAngles"], result); return result;
        }
        if (value instanceof TextDocument) {
            result = {type:"text_document"}; attrs(value, ["text", "font", "fontSize", "fontStyle",
                "applyFill", "fillColor", "applyStroke", "strokeColor", "strokeWidth", "strokeOverFill",
                "justification", "tracking", "leading", "autoLeading", "baselineShift", "fauxBold",
                "fauxItalic", "allCaps", "smallCaps", "horizontalScale", "verticalScale", "boxText",
                "boxTextSize", "boxTextPos", "pointText", "baselineLocs"], result); return result;
        }
        if (value instanceof MarkerValue) {
            result = {type:"marker"}; attrs(value, ["comment", "chapter", "url", "frameTarget", "cuePointName",
                "eventCuePoint", "duration", "label", "protectedRegion"], result);
            try { result.parameters = value.getParameters(); } catch (_) {} return result;
        }
        // Never coerce arbitrary host/plugin objects or execute their methods.
        return {type:"opaque_host_value", readable:false};
    }
    function ease(items) {
        var result = []; for (var i = 0; i < items.length; i++) result.push({speed:items[i].speed, influence:items[i].influence});
        return result;
    }
    function inspect(property, path, depth) {
        propertyCount++;
        if (propertyCount > 250000 || depth > 64) throw Error("Property export resource limit exceeded");
        var result = {name:property.name, matchName:property.matchName,
            index:property.propertyIndex, path:path, propertyType:String(property.propertyType)};
        attrs(property, ["enabled", "active", "canSetEnabled", "elided", "isEffect", "isMask"], result);
        if (property.propertyType !== PropertyType.PROPERTY) {
            result.children = [];
            for (var j = 1; j <= property.numProperties; j++) result.children.push(inspect(property.property(j), path.concat([j]), depth + 1));
            return result;
        }
        attrs(property, ["canVaryOverTime", "canSetExpression", "isSpatial", "isSeparationFollower",
            "isSeparationLeader", "dimensionsSeparated", "separationDimension", "unitsText", "hasMin", "hasMax"], result);
        result.valueType = String(property.propertyValueType);
        if (result.hasMin) read(property, "minValue", result);
        if (result.hasMax) read(property, "maxValue", result);
        if (property.canSetExpression) attrs(property, ["expression", "expressionEnabled", "expressionError"], result);
        if (property.propertyValueType === PropertyValueType.NO_VALUE) return result;
        if (property.propertyValueType === PropertyValueType.CUSTOM_VALUE) {
            result.value = {type:"custom_value", readable:false};
            report.warnings.push({code:"custom_property", path:path, matchName:property.matchName}); return result;
        }
        try { result.value = plain(property.valueAtTime(0, true)); }
        catch (err) { result.readError = String(err); report.warnings.push({code:"property_unreadable", path:path, error:String(err)}); }
        result.keys = [];
        for (var k = 1; k <= property.numKeys; k++) {
            var key = {index:k, time:property.keyTime(k), value:plain(property.keyValue(k))};
            try { key.inInterpolation = String(property.keyInInterpolationType(k)); key.outInterpolation = String(property.keyOutInterpolationType(k)); } catch (_) {}
            try { key.inEase = ease(property.keyInTemporalEase(k)); key.outEase = ease(property.keyOutTemporalEase(k)); } catch (_) {}
            try { key.temporalAutoBezier = property.keyTemporalAutoBezier(k); key.temporalContinuous = property.keyTemporalContinuous(k); } catch (_) {}
            if (property.isSpatial) {
                try { key.inSpatialTangent = plain(property.keyInSpatialTangent(k)); key.outSpatialTangent = plain(property.keyOutSpatialTangent(k));
                    key.spatialAutoBezier = property.keySpatialAutoBezier(k); key.spatialContinuous = property.keySpatialContinuous(k);
                    key.roving = property.keyRoving(k); } catch (_) {}
            }
            result.keys.push(key);
        }
        return result;
    }
    function layerRecord(layer, comp) {
        var record = {id:String(comp.id) + ":" + layer.index, idKind:"composition_index",
            index:layer.index, name:layer.name, properties:[]};
        // AE 2021 has no persistent Layer.id; newer versions may provide it.
        try { if (layer.id !== undefined) { record.id = String(layer.id); record.idKind = "ae_layer_id"; } } catch (_) {}
        attrs(layer, ["enabled", "solo", "shy", "locked", "inPoint", "outPoint", "startTime", "stretch",
            "threeDLayer", "threeDPerChar", "adjustmentLayer", "nullLayer", "guideLayer", "motionBlur",
            "collapseTransformation", "preserveTransparency", "audioEnabled", "hasAudio", "hasVideo",
            "timeRemapEnabled", "width", "height", "blendingMode", "trackMatteType", "isTrackMatte",
            "hasTrackMatte", "autoOrient", "quality", "samplingQuality"], record);
        try { record.parentIndex = layer.parent ? layer.parent.index : null; } catch (_) {}
        try { record.sourceId = layer.source ? String(layer.source.id) : null; } catch (_) {}
        try { record.trackMatteLayerIndex = layer.trackMatteLayer ? layer.trackMatteLayer.index : null; } catch (_) {}
        record.class = layer instanceof CameraLayer ? "camera" : layer instanceof LightLayer ? "light" :
            layer instanceof TextLayer ? "text" : layer instanceof ShapeLayer ? "shape" : "av";
        for (var i = 1; i <= layer.numProperties; i++) record.properties.push(inspect(layer.property(i), [String(comp.id), layer.index, i], 0));
        return record;
    }
    try {
        var output = new Folder(config.output);
        if (!output.exists && !output.create()) throw Error("Cannot create output directory");
        if (config.project) {
            if (app.project && (app.project.numItems > 0 || app.project.file)) throw Error("Use an empty, dedicated AE instance; refusing to replace an open project");
            app.beginSuppressDialogs(); suppressed = true;
            if (!app.open(new File(config.project))) throw Error("AE could not open project");
            opened = true;
        }
        if (!app.project) throw Error("No project is open");
        report.projectFile = app.project.file ? app.project.file.fsName : null;
        report.settings = {};
        attrs(app.project, ["bitsPerChannel", "workingSpace", "linearizeWorkingSpace", "linearBlending", "expressionEngine"], report.settings);
        var compMap = {}, roots = [], i;
        for (i = 0; i < app.effects.length; i++) {
            var effect = app.effects[i]; report.effects.push({name:effect.displayName, matchName:effect.matchName, category:effect.category, version:effect.version});
        }
        for (i = 1; i <= app.project.numItems; i++) {
            var item = app.project.item(i), record = {id:String(item.id), name:item.name};
            if (item instanceof CompItem) {
                attrs(item, ["width", "height", "pixelAspect", "frameRate", "frameDuration", "duration",
                    "displayStartTime", "displayStartFrame", "workAreaStart", "workAreaDuration", "bgColor",
                    "motionBlur", "shutterAngle", "shutterPhase", "frameBlending", "preserveNestedFrameRate",
                    "preserveNestedResolution", "renderer"], record);
                record.layers = []; record.references = [];
                for (var l = 1; l <= item.numLayers; l++) {
                    var layer = item.layer(l); record.layers.push(layerRecord(layer, item));
                    try { if (layer.source instanceof CompItem) record.references.push({layerIndex:l, compositionId:String(layer.source.id), enabled:layer.enabled}); } catch (_) {}
                }
                report.compositions.push(record); compMap[record.id] = record;
                if ((config.rootId && String(config.rootId) === record.id) ||
                    (!config.rootId && config.rootName && config.rootName === record.name)) roots.push(record.id);
            } else if (item instanceof FootageItem) {
                attrs(item, ["width", "height", "pixelAspect", "frameRate", "duration", "hasAudio", "hasVideo", "footageMissing"], record);
                try { record.path = item.file ? item.file.fsName : null; } catch (_) {}
                record.interpretation = {};
                attrs(item.mainSource, ["alphaMode", "premulColor", "invertAlpha", "isStill", "conformFrameRate",
                    "nativeFrameRate", "displayFrameRate", "fieldSeparationType", "loop", "color", "missingFootagePath"], record.interpretation);
                report.footage.push(record);
            }
        }
        if (roots.length === 1) {
            report.rootId = roots[0]; var visited = {};
            function visit(id) {
                if (visited[id]) return; visited[id] = true; report.reachableCompositionIds.push(id);
                var refs = compMap[id].references;
                for (var r = 0; r < refs.length; r++) visit(refs[r].compositionId);
            }
            visit(report.rootId);
            report.reachability = "all_source_references_including_disabled_layers";
        } else report.warnings.push({code:"root_not_unique", matches:roots.length});
        report.propertyCount = propertyCount;
        report.state = "collected";
        report.warnings.push({code:"not_restoration_ready", reason:"Custom values, missing dependencies and visual fidelity still need validation; no expressions or renders were evaluated."});
    } catch (err) {
        report.state = "failed"; report.errors.push({message:String(err), line:err.line});
    } finally {
        if (opened) try { app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); } catch (_) {}
        if (suppressed) try { app.endSuppressDialogs(false); } catch (_) {}
        write(config.output + "/ae-project-export.json", report);
    }
    return report;
}
