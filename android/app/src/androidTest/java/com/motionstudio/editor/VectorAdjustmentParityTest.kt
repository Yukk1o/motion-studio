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
            for ((frame, angle) in listOf(0 to 0, 23 to 100)) data(NativeBridge.command(native, JSONObject().put("op", "vector").put("object", 1)
                .put("action", JSONObject().put("action", "set_parameter").put("parameter", "angle").put("frame", frame).put("value", angle).put("animated", true)).toString()))
            for (frame in listOf(0, 7, 23, 7)) {
                data(NativeBridge.seek(native, frame.toDouble()))
                val png = BitmapFactory.decodeFile(data(NativeBridge.capture(native)).getString("path"), BitmapFactory.Options().apply { inPremultiplied = false; inScaled = false })
                val info = data(NativeBridge.renderPlanInfo(native)); val plan = ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.LITTLE_ENDIAN)
                assertTrue(NativeBridge.sampleRenderPlanInto(native, frame, plan) > 0)
                val gpu = EglMovieRenderer(null, 192, 192, data(NativeBridge.state(native)).getJSONObject("project"), native, info)
                try {
                    gpu.draw(plan); val pixels = ByteBuffer.allocateDirect(192 * 192 * 4); gpu.readPixelsInto(pixels)
                    reports.put(glesParity(png, pixels).put("frame", frame))
                    File(root, "unencoded-report.json").writeText(reports.toString(2))
                } finally { gpu.close(); png.recycle() }
            }
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
