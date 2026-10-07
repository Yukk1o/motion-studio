package com.motionstudio.editor

import android.graphics.BitmapFactory
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

class CompositionParityTest {
    @Test fun nestedAnimatedVectorsAndAdjustmentLayersMatchUnencodedExport() {
        val root=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"composition-vector-parity/${UUID.randomUUID()}").apply{mkdirs()}
        val project=data(NativeBridge.projectTemplate(0)).put("width",192).put("height",192).put("frames",24).put("layers",JSONArray()).put("background",array(0,0,0,0))
        project.getJSONObject("camera").put("created",false)
        val native=NativeBridge.create(root.absolutePath,project.toString());assertTrue(NativeBridge.creationError(),native>0)
        val reports=JSONArray()
        try {
            for((id,x,y) in listOf(Triple(1,65,62),Triple(2,115,134)))data(CompositionBridge.command(native,"comp-main",JSONObject().put("op","add_shape").put("id",id).put("name","ring").put("shape","ring").put("size",array(96,96)).put("position",array(x,y,0))))
            data(CompositionBridge.command(native,"comp-main",JSONObject().put("op","add_adjustment").put("id",3).put("name","adjustment")))
            val packages=data(CompositionBridge.plugin(native,"comp-main",JSONObject().put("op","catalogue"))).getJSONArray("packages")
            val pkg=packages.objects().first{it.getJSONObject("manifest").getString("id")=="com.motionstudio.effects.ae2021"&&it.getJSONObject("manifest").getString("version")=="1.4.0"}
            data(CompositionBridge.plugin(native,"comp-main",JSONObject().put("op","add").put("object",3).put("effect","tint").put("plugin",pkg.getJSONObject("manifest").getString("id")).put("version","1.4.0").put("hash",pkg.getString("hash"))))
            for((frame,ratio) in listOf(0 to .2,23 to .7))data(CompositionBridge.command(native,"comp-main",JSONObject().put("op","vector").put("object",1).put("action",JSONObject().put("action","set_parameter").put("parameter","inner_ratio").put("frame",frame).put("value",ratio).put("animated",true))))
            val child=result(action(native,"comp-main","precompose","objects" to array(1,2,3),"name" to "vectors")).getString("composition")
            val reference=req(native,"comp-main","state").getJSONObject("project").getJSONArray("layers").getJSONObject(0).getLong("id")
            action(native,"comp-main","precompose","objects" to array(reference),"name" to "outer")
            val frozen=req(native,"comp-main","state").getJSONObject("project")
            val samples=ArrayList<Triple<Int,android.graphics.Bitmap,ByteBuffer>>()
            try {
                for(frame in listOf(0,11,23,11)) {
                    req(native,"comp-main","seek","frame" to frame)
                    val path=data(CompositionBridge.capture(native,"comp-main")).getString("path");File(path).copyTo(File(root,"native-$frame.png"),overwrite=true)
                    val png=BitmapFactory.decodeFile(path,BitmapFactory.Options().apply{inPremultiplied=false;inScaled=false})
                    val tiny=ByteBuffer.allocateDirect(32).order(ByteOrder.LITTLE_ENDIAN)
                    val required=-CompositionBridge.sampleFrameBundleInto(native,"comp-main",frame.toDouble(),tiny);assertTrue(required>32)
                    val bundle=ByteBuffer.allocateDirect(required).order(ByteOrder.LITTLE_ENDIAN)
                    assertEquals(required,CompositionBridge.sampleFrameBundleInto(native,"comp-main",frame.toDouble(),bundle))
                    samples.add(Triple(frame,png,bundle))
                }
                assertFalse("Nested vector animation had no visible effect",samples[0].second.sameAs(samples[2].second))
                assertTrue("Random seek changed the same nested frame",samples[1].second.sameAs(samples[3].second))
                val gpu=EglMovieRenderer(null,192,192,frozen,native,data(NativeBridge.renderPlanInfo(native)))
                try {for((frame,png,bundle) in samples) {
                    gpu.prepareBundle(bundle);gpu.drawBundle(bundle)
                    val pixels=ByteBuffer.allocateDirect(192*192*4);gpu.readPixelsInto(pixels)
                    val raw=ByteArray(pixels.capacity());pixels.duplicate().apply{clear()}.get(raw);File(root,"gles-$frame.rgba").writeBytes(raw)
                    reports.put(glesParity(png,pixels).put("frame",frame).put("child",child))
                    File(root,"unencoded-report.json").writeText(reports.toString(2))
                }}finally{gpu.close()}
            }finally{samples.forEach{it.second.recycle()}}
        }finally{NativeBridge.destroy(native)}
    }
    private fun data(raw: String) = nativeData(raw)
    private fun array(vararg values: Any) = JSONArray(values.toList())
    private fun track(value: Any) = JSONObject().put("value", value).put("keys", JSONArray())
    private fun layer(id: Int, x: Int, y: Int, color: JSONArray) = JSONObject().put("id", id).put("name", "layer $id")
        .put("visible", true).put("locked", false).put("three_d", false).put("size", array(70, 45))
        .put("content", JSONObject().put("kind", "solid").put("color", color))
        .put("transform", JSONObject().put("position", track(array(x, y, 0))).put("rotation", track(array(0, 0, 0)))
            .put("scale", track(array(100, 100, 100))).put("opacity", track(1)).put("anchor", array(.5, .5)))
    private fun req(native: Long, composition: String, op: String, vararg fields: Pair<String, Any>) = data(CompositionBridge.request(native,
        JSONObject().put("version", 1).put("composition", composition).put("op", op).apply { fields.forEach { put(it.first, it.second) } }.toString()))
    private fun action(native: Long, composition: String, kind: String, vararg fields: Pair<String, Any>) = req(native, composition, "action",
        "action" to JSONObject().put("kind", kind).apply { fields.forEach { put(it.first, it.second) } })
    private fun result(snapshot: JSONObject) = snapshot.getJSONArray("edit_results").getJSONObject(0).getJSONObject("result")

    @Test fun mixedFpsRepeated3dReferencesAndSpatialEffectsMatchUnencodedExporter() {
        val root = File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir, "composition-parity/${UUID.randomUUID()}").apply { mkdirs() }
        val project = data(NativeBridge.projectTemplate(0)).put("width", 192).put("height", 192).put("fps", 59).put("frames", 118)
            .put("background", array(0, 0, 0, 0)).put("layers", JSONArray().put(layer(1, 65, 65, array(.9, .2, .1, .6))).put(layer(2, 128, 128, array(.1, .3, .9, .7))))
        project.getJSONObject("camera").put("created", false)
        val native = NativeBridge.create(root.absolutePath, project.toString()); assertTrue(NativeBridge.creationError(), native > 0)
        val reports = JSONArray()
        try {
            val packages = data(CompositionBridge.plugin(native, "comp-main", JSONObject().put("op", "catalogue"))).getJSONArray("packages")
            val pkg = (0 until packages.length()).map { packages.getJSONObject(it) }.first {
                it.getJSONObject("manifest").getString("id") == "com.motionstudio.effects.ae2021" && it.getJSONObject("manifest").getString("version") == "1.4.0"
            }
            data(CompositionBridge.plugin(native, "comp-main", JSONObject().put("op", "add").put("object", 1).put("effect", "shake")
                .put("plugin", pkg.getJSONObject("manifest").getString("id")).put("version", "1.4.0").put("hash", pkg.getString("hash"))))
            for ((parameter, values) in listOf("translation" to array(12, 6, 0, 0), "rotation" to array(0, 0, 0, 0), "zoom" to array(0, 0, 0, 0)))
                data(CompositionBridge.command(native, "comp-main", JSONObject().put("op", "effect").put("object", 1)
                    .put("action", JSONObject().put("kind", "set").put("effect", 1).put("param", parameter).put("frame", 0).put("value", values))))
            val inside = result(action(native, "comp-main", "precompose", "objects" to array(1, 2), "name" to "inside")).getString("composition")
            val first = req(native, "comp-main", "state").getJSONObject("project").getJSONArray("layers").getJSONObject(0).getLong("id")
            val outside = result(action(native, "comp-main", "precompose", "objects" to array(first), "name" to "outside")).getString("composition")
            req(native, inside, "open", "path" to array("comp-main", outside, inside))
            val settings = JSONObject().put("name", "144 fps inner").put("width", 192).put("height", 192).put("fps", 144).put("frames", 288)
                .put("timing", "preserve_seconds").put("shorten", "reject")
            val preview = req(native, inside, "settings_preview", "settings" to settings); assertTrue(preview.toString(), preview.getBoolean("valid"))
            req(native, inside, "settings_apply", "settings" to settings, "expected_revision" to preview.getLong("expected_revision"))
            req(native, "comp-main", "open", "path" to array("comp-main"))
            val reference = req(native, "comp-main", "state").getJSONObject("project").getJSONArray("layers").getJSONObject(0).getLong("id")
            data(CompositionBridge.command(native, "comp-main", JSONObject().put("op", "duplicate").put("object", reference)))
            val duplicate = req(native, "comp-main", "state").getJSONObject("project").getJSONArray("layers").getJSONObject(1).getLong("id")
            action(native, "comp-main", "set_clip", "object" to duplicate, "source_start_frame" to 10, "volume" to 1, "muted" to false)
            data(CompositionBridge.command(native, "comp-main", JSONObject().put("op", "set_layer_3d").put("object", duplicate).put("enabled", true)))
            val saved = JSONObject(CompositionBridge.freezeProject(native, "comp-main"))
            saved.getJSONArray("layers").getJSONObject(1).getJSONObject("transform").getJSONObject("rotation").put("value", array(10, 30, 5))
            data(NativeBridge.replace(native, saved.toString()))
            data class Sample(val frame: Int, val png: android.graphics.Bitmap, val bundle: ByteBuffer)
            val samples = ArrayList<Sample>()
            try {
                for (frame in listOf(0, 17, 58, 115, 17)) {
                    req(native, "comp-main", "seek", "frame" to frame)
                    val png = BitmapFactory.decodeFile(data(CompositionBridge.capture(native, "comp-main")).getString("path"), BitmapFactory.Options().apply { inPremultiplied = false; inScaled = false })
                    val tiny = ByteBuffer.allocateDirect(32).order(ByteOrder.LITTLE_ENDIAN)
                    val needed = CompositionBridge.sampleFrameBundleInto(native, "comp-main", frame.toDouble(), tiny); assertTrue(needed < -32)
                    val bundle = ByteBuffer.allocateDirect(-needed).order(ByteOrder.LITTLE_ENDIAN)
                    assertEquals(-needed, CompositionBridge.sampleFrameBundleInto(native, "comp-main", frame.toDouble(), bundle))
                    samples.add(Sample(frame, png, bundle))
                }
                assertFalse("Timed references had no visible effect", samples[0].png.sameAs(samples[3].png))
                assertTrue("Random native seek changed the same frame", samples[1].png.sameAs(samples[4].png))
                val gpu = EglMovieRenderer(null, 192, 192, saved, native, data(NativeBridge.renderPlanInfo(native)))
                try {
                    for (sample in samples) {
                        gpu.prepareBundle(sample.bundle); gpu.drawBundle(sample.bundle)
                        val pixels = ByteBuffer.allocateDirect(192 * 192 * 4); gpu.readPixelsInto(pixels)
                        reports.put(glesParity(sample.png, pixels).put("frame", sample.frame).put("nodes", sample.bundle.getInt(8)))
                        File(root, "unencoded-report.json").writeText(reports.toString(2))
                    }
                } finally { gpu.close() }
            } finally { samples.forEach { it.png.recycle() } }
        } finally { NativeBridge.destroy(native) }
    }
}
