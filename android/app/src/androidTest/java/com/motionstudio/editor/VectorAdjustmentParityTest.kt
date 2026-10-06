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

class VectorAdjustmentParityTest {
    private fun data(raw: String) = nativeData(raw)
    private fun root() = File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,
        "vector-parity/${UUID.randomUUID()}").apply { mkdirs() }
    private fun base() = data(NativeBridge.projectTemplate(0)).put("width", 192).put("height", 192)
        .put("frames", 24).put("layers", JSONArray()).put("background", JSONArray(listOf(0, 0, 0, 0)))
        .also { it.getJSONObject("camera").put("created", false) }
    private fun effect(native: Long, objectId: Int, name: String) {
        val packages = data(NativeBridge.plugin(native, "{\"op\":\"catalogue\"}")).getJSONArray("packages")
        val pkg = (0 until packages.length()).map { packages.getJSONObject(it) }.first {
            it.getJSONObject("manifest").getString("id") == "com.motionstudio.effects.ae2021" && it.getJSONObject("manifest").getString("version") == "1.4.0"
        }
        data(NativeBridge.plugin(native, JSONObject().put("op", "add").put("object", objectId).put("effect", name)
            .put("plugin", pkg.getJSONObject("manifest").getString("id")).put("version", "1.4.0").put("hash", pkg.getString("hash")).toString()))
    }
    private fun shape(native: Long, id: Int, name: String, size: Int, x: Int, y: Int) = data(NativeBridge.command(native,
        JSONObject().put("op", "add_shape").put("id", id).put("name", name).put("shape", name)
            .put("size", JSONArray(listOf(size, size))).put("position", JSONArray(listOf(x, y, 0))).toString()))

    @Test fun twoAdjustmentsRotatedMaskAnimatedVectorsAndAlphaMatchUnencodedExporter() {
        val root = root(); val native = NativeBridge.create(root.absolutePath, base().toString())
        assertTrue(NativeBridge.creationError(), native > 0)
        val reports = JSONArray()
        try {
            shape(native, 1, "ring", 110, 68, 70); shape(native, 2, "heart", 80, 126, 120)
            for (id in listOf(3, 4)) data(NativeBridge.command(native, JSONObject().put("op", "add_adjustment").put("id", id).put("name", "adjust $id").toString()))
            shape(native, 5, "rectangle", 14, 160, 30)
            effect(native, 1, "tint"); effect(native, 3, "tint"); effect(native, 4, "gaussian_blur")
            val project = data(NativeBridge.state(native)).getJSONObject("project"); val layers = project.getJSONArray("layers")
            layers.getJSONObject(0).getJSONObject("content").getJSONObject("vector").getJSONObject("fill").put("value", JSONArray(listOf(.9, .2, .1, .5)))
            layers.getJSONObject(1).getJSONObject("content").getJSONObject("vector").getJSONObject("fill").put("value", JSONArray(listOf(.1, .8, .3, .7)))
            layers.getJSONObject(1).put("three_d", true).getJSONObject("transform").getJSONObject("rotation").put("value", JSONArray(listOf(12, 25, 0)))
            val mask = layers.getJSONObject(2); mask.put("size", JSONArray(listOf(85, 70)))
            mask.getJSONObject("transform").getJSONObject("rotation").put("value", JSONArray(listOf(0, 0, 25)))
            mask.getJSONObject("transform").getJSONObject("opacity").put("value", .5)
            layers.getJSONObject(3).getJSONArray("effects").getJSONObject(0).getJSONObject("params").getJSONObject("p0001")
                .getJSONObject("track").put("value", JSONArray(listOf(4, 0, 0, 0)))
            data(NativeBridge.replace(native, project.toString()))
            for ((frame, ratio) in listOf(0 to .25, 23 to .75)) data(NativeBridge.command(native, JSONObject().put("op", "vector").put("object", 1)
                .put("action", JSONObject().put("action", "set_parameter").put("parameter", "inner_ratio").put("frame", frame).put("value", ratio).put("animated", true)).toString()))
            data class Sample(val frame: Int, val label: String, val png: android.graphics.Bitmap, val plan: ByteBuffer)
            val samples = ArrayList<Sample>()
            fun sample(frame: Int, label: String) {
                data(NativeBridge.seek(native, frame.toDouble()))
                val path = data(NativeBridge.capture(native)).getString("path")
                File(path).copyTo(File(root, "$label.png"), overwrite = true)
                val png = BitmapFactory.decodeFile(path, BitmapFactory.Options().apply { inPremultiplied = false; inScaled = false })
                val info = data(NativeBridge.renderPlanInfo(native)); val plan = ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.LITTLE_ENDIAN)
                assertTrue(NativeBridge.sampleRenderPlanInto(native, frame, plan) > 0)
                samples.add(Sample(frame, label, png, plan))
            }
            try {
                for (frame in listOf(0, 7, 23, 7)) {
                    sample(frame, "frame-$frame-${samples.size}")
                }
                assertFalse("Geometry animation had no visible effect", samples[0].png.sameAs(samples[2].png))
                assertTrue("Random native seek changed the same frame", samples[1].png.sameAs(samples[3].png))
                val complete = data(NativeBridge.state(native)).getJSONObject("project").toString()
                for (mode in listOf("vectors_only", "vector_tint", "one_adjustment", "blur_adjustment", "all")) {
                    val p = JSONObject(complete); val ls = p.getJSONArray("layers")
                    ls.getJSONObject(0).getJSONArray("effects").getJSONObject(0).put("enabled", mode != "vectors_only")
                    ls.getJSONObject(2).put("visible", mode in listOf("one_adjustment", "all"))
                    ls.getJSONObject(3).put("visible", mode in listOf("blur_adjustment", "all"))
                    data(NativeBridge.replace(native, p.toString()))
                    sample(0, mode)
                }
                val gpu = EglMovieRenderer(null, 192, 192, data(NativeBridge.state(native)).getJSONObject("project"), native, data(NativeBridge.renderPlanInfo(native)))
                try {
                    for (sample in samples) {
                        gpu.draw(sample.plan); val pixels = ByteBuffer.allocateDirect(192 * 192 * 4); gpu.readPixelsInto(pixels)
                        val raw = ByteArray(pixels.capacity()); pixels.duplicate().apply { clear() }.get(raw)
                        File(root, "${sample.label}.rgba").writeBytes(raw)
                        reports.put(glesParity(sample.png, pixels).put("frame", sample.frame).put("label", sample.label))
                        File(root, "unencoded-report.json").writeText(reports.toString(2))
                    }
                    assertEquals("The synthetic capability probe must run once", 4L, gpu.graphicsCapabilityReadbackBytes)
                } finally { gpu.close() }
            } finally { samples.forEach { it.png.recycle() } }
        } finally { NativeBridge.destroy(native) }
    }

    @Test fun cancellationRemovesPartialMovieAndNextVectorExportStillWorks() {
        val root = root(); val native = NativeBridge.create(root.absolutePath, base().put("frames", 4).toString())
        assertTrue(NativeBridge.creationError(), native > 0)
        try {
            shape(native, 1, "star", 110, 96, 96)
            val project = data(NativeBridge.state(native)).getJSONObject("project").toString()
            val exporter = VideoExporter(root, project).apply { cancelled.set(true) }
            assertTrue(runCatching { exporter.run { _, _ -> } }.isFailure)
            assertTrue(File(root, "exports").listFiles().orEmpty().none { it.extension == "mp4" })
            assertTrue(VideoExporter(root, project).run { _, _ -> }.length() > 64)
            assertTrue(File(data(NativeBridge.capture(native)).getString("path")).isFile)
        } finally { NativeBridge.destroy(native) }
    }
}
