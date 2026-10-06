package com.motionstudio.editor

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import android.media.MediaMetadataRetriever
import android.opengl.EGL14
import android.opengl.GLES30
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.pow

@RunWith(AndroidJUnit4::class)
class SpatialEffectsTest {
    private fun data(raw: String): JSONObject = JSONObject(raw).let {
        assertTrue(it.optString("error"), it.optBoolean("ok")); it.getJSONObject("data")
    }
    private fun root() = File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,
        "spatial-effects-test/${UUID.randomUUID()}/project").apply { mkdirs() }
    private fun fixture(root: File, opaque: Boolean): JSONObject {
        val p = data(NativeBridge.projectTemplate(0)).put("width", 192).put("height", 192).put("frames", 12)
            .put("background", JSONArray(listOf(0, 0, 0, if (opaque) 1 else 0)))
        p.getJSONObject("camera").put("created", false)
        val layer = p.getJSONArray("layers").getJSONObject(1).put("three_d", false)
            .put("size", JSONArray(listOf(32, 20))).put("content", JSONObject().put("kind", "image").put("asset", 1))
        layer.getJSONObject("transform").getJSONObject("position").put("value", JSONArray(listOf(96, 96, 0)))
        p.put("layers", JSONArray().put(layer)).put("assets", JSONArray().put(JSONObject().put("id", 1)
            .put("path", "assets/input.png").put("width", 32).put("height", 20)))
        val image = Bitmap.createBitmap(32, 20, Bitmap.Config.ARGB_8888).apply { eraseColor(Color.WHITE) }
        File(root, "assets/input.png").apply { parentFile!!.mkdirs() }.outputStream().use {
            image.compress(Bitmap.CompressFormat.PNG, 100, it)
        }
        image.recycle(); return p
    }
    private fun pkg(native: Long, version: String): JSONObject {
        val packages = data(NativeBridge.plugin(native, "{\"op\":\"catalogue\"}")).getJSONArray("packages")
        return (0 until packages.length()).map { packages.getJSONObject(it) }.first {
            it.getJSONObject("manifest").getString("id") == "com.motionstudio.effects.ae2021" &&
                it.getJSONObject("manifest").getString("version") == version
        }
    }
    private fun request(native: Long, op: String, effect: String, version: String): JSONObject {
        val p = pkg(native, version)
        return JSONObject().put("op", op).put("object", 2).put("plugin", p.getJSONObject("manifest").getString("id"))
            .put("version", version).put("hash", p.getString("hash")).put("effect", effect)
    }
    private fun command(native: Long, action: JSONObject): JSONObject = data(NativeBridge.command(native,
        JSONObject().put("op", "effect").put("object", 2).put("action", action).toString()))
    private fun set(native: Long, param: String, values: List<Number>, frame: Int = 0) = command(native,
        JSONObject().put("kind", "set").put("effect", 1).put("param", param).put("frame", frame)
            .put("value", JSONArray(values + List(4 - values.size) { 0 })))
    private fun layer(snapshot: JSONObject) = snapshot.getJSONObject("project").getJSONArray("layers").getJSONObject(0)

    @Test fun explicitUpgradeRetainsTracksSeedDisabledStateAndChainOrderAndIsOneUndo() {
        val root = root(); val native = NativeBridge.create(root.absolutePath, fixture(root, false).toString())
        assertTrue(NativeBridge.creationError(), native > 0)
        try {
            data(NativeBridge.plugin(native, request(native, "add", "shake", "1.3.0").toString()))
            data(NativeBridge.plugin(native, request(native, "add", "tint", "1.3.0").toString()))
            set(native, "translation", listOf(30, 15))
            command(native, JSONObject().put("kind", "animate").put("effect", 1).put("param", "translation").put("frame", 0).put("enabled", true))
            set(native, "translation", listOf(40, 20), 11)
            command(native, JSONObject().put("kind", "seed").put("effect", 1).put("seed", 789))
            command(native, JSONObject().put("kind", "enable").put("effect", 1).put("enabled", false))
            val before = layer(data(NativeBridge.state(native)))
            val upgrade = request(native, "upgrade", "shake", "1.4.0").put("instance", 1).put("preserve_parameters", true)
            val after = layer(data(NativeBridge.plugin(native, upgrade.toString())))
            assertEquals(before.getJSONObject("transform").toString(), after.getJSONObject("transform").toString())
            val old = before.getJSONArray("effects").getJSONObject(0); val updated = after.getJSONArray("effects").getJSONObject(0)
            assertEquals("1.4.0", updated.getString("version")); assertEquals(1L, updated.getLong("id"))
            for (key in listOf("params", "seed", "enabled", "scene")) assertEquals("preserved $key", old.opt(key)?.toString(), updated.opt(key)?.toString())
            assertEquals(before.getJSONArray("effects").getJSONObject(1).toString(), after.getJSONArray("effects").getJSONObject(1).toString())
            assertEquals(before.toString(), layer(data(NativeBridge.history(native, 0))).toString())
            assertEquals(after.toString(), layer(data(NativeBridge.history(native, 1))).toString())
            val rejected = JSONObject(NativeBridge.plugin(native, request(native, "upgrade", "tint", "1.4.0")
                .put("instance", 1).put("preserve_parameters", true).toString()))
            assertFalse(rejected.toString(), rejected.getBoolean("ok"))
            assertEquals(after.toString(), layer(data(NativeBridge.state(native))).toString())
            val saved = data(NativeBridge.state(native)).getJSONObject("project")
            val reopened = NativeBridge.create(root.absolutePath, saved.toString())
            assertTrue(NativeBridge.creationError(), reopened > 0)
            try { assertEquals(after.toString(), layer(data(NativeBridge.state(reopened))).toString()) }
            finally { NativeBridge.destroy(reopened) }
        } finally { NativeBridge.destroy(native) }
    }

    private fun configure(native: Long, name: String) {
        data(NativeBridge.plugin(native, request(native, "add", name, "1.4.0").toString()))
        when (name) {
            "shake" -> { set(native, "translation", listOf(32, 16)); set(native, "rotation", listOf(0)); set(native, "zoom", listOf(0)) }
            "transform_blur" -> {
                set(native, "shift", listOf(38, 24)); set(native, "translation_blur", listOf(0, 0))
                command(native, JSONObject().put("kind", "animate").put("effect", 1).put("param", "shift").put("frame", 0).put("enabled", true))
                set(native, "shift", listOf(-35, -20), 11)
            }
            "polar_coordinates" -> set(native, "p0001", listOf(1))
        }
    }

    @Test fun expandedPlanesMatchUnencodedGlesAndAnimatedMp4OutsideOriginalRectangle() {
        val reports = JSONArray()
        for (name in listOf("shake", "transform_blur", "polar_coordinates")) {
            val root = root(); val native = NativeBridge.create(root.absolutePath, fixture(root, false).toString())
            assertTrue(NativeBridge.creationError(), native > 0)
            try {
                configure(native, name); data(NativeBridge.seek(native, 7.0))
                val reference = BitmapFactory.decodeFile(data(NativeBridge.capture(native)).getString("path"), BitmapFactory.Options().apply { inPremultiplied = false; inScaled = false })
                var outside = 0
                for (y in 0 until 192) for (x in 0 until 192) if ((x !in 80..111 || y !in 86..105) && Color.alpha(reference.getPixel(x, y)) > 200) outside++
                assertTrue("$name remained inside original layer", outside > 5)
                val unencoded = compareUnencoded(native, reference)
                reference.recycle()
                // Composite over opaque black for a meaningful RGB comparison to H.264.
                val frozen = data(NativeBridge.state(native)).getJSONObject("project").put("background", JSONArray(listOf(0, 0, 0, 1)))
                data(NativeBridge.replace(native, frozen.toString()))
                val file = VideoExporter(root, frozen.toString()).run { _, _ -> }
                val retriever = MediaMetadataRetriever()
                try {
                    retriever.setDataSource(file.absolutePath)
                    assertEquals("12", retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_FRAME_COUNT))
                    for (frame in listOf(0, 7, 11)) {
                        data(NativeBridge.seek(native, frame.toDouble()))
                        val expected = BitmapFactory.decodeFile(data(NativeBridge.capture(native)).getString("path"))
                        val decoded = retriever.getFrameAtIndex(frame)!!
                        var sum = 0L; var foreground = 0L; var active = 0
                        for (y in 0 until 192) for (x in 0 until 192) {
                            val a = expected.getPixel(x, y); val b = decoded.getPixel(x, y)
                            val d = abs(Color.red(a) - Color.red(b)) + abs(Color.green(a) - Color.green(b)) + abs(Color.blue(a) - Color.blue(b))
                            sum += d
                            if (Color.red(a) > 50) { foreground += d; active += 3 }
                        }
                        val report = JSONObject().put("effect", name).put("frame", frame).put("frames", 12)
                            .put("outsidePixels", outside).put("rgbMae", sum.toDouble() / (192 * 192 * 3))
                            .put("foregroundRgbMae", foreground.toDouble() / active.coerceAtLeast(1)).put("unencoded", unencoded)
                        reports.put(report); File(root, "spatial-report.json").writeText(reports.toString(2))
                        assertTrue(report.toString(), report.getDouble("rgbMae") < 6 && report.getDouble("foregroundRgbMae") < 8)
                        expected.recycle(); decoded.recycle()
                    }
                } finally { retriever.release() }
            } finally { NativeBridge.destroy(native) }
        }
        assertEquals(9, reports.length())
    }

    private fun compareUnencoded(native: Long, reference: Bitmap): JSONObject {
        val info = data(NativeBridge.renderPlanInfo(native))
        val plan = ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
        assertTrue(NativeBridge.sampleRenderPlanInto(native, 7, plan) > 0)
        val display = EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY)
        var context = EGL14.EGL_NO_CONTEXT; var surface = EGL14.EGL_NO_SURFACE
        val textures = ArrayList<Int>(); var gl: GlEffects? = null
        try {
            val version = IntArray(2); assertTrue(EGL14.eglInitialize(display, version, 0, version, 1))
            val configs = arrayOfNulls<android.opengl.EGLConfig>(1); val count = IntArray(1)
            assertTrue(EGL14.eglChooseConfig(display, intArrayOf(EGL14.EGL_RED_SIZE, 8, EGL14.EGL_GREEN_SIZE, 8, EGL14.EGL_BLUE_SIZE, 8, EGL14.EGL_ALPHA_SIZE, 8, EGL14.EGL_RENDERABLE_TYPE, 0x0040, EGL14.EGL_SURFACE_TYPE, EGL14.EGL_PBUFFER_BIT, EGL14.EGL_NONE), 0, configs, 0, 1, count, 0))
            context = EGL14.eglCreateContext(display, configs[0], EGL14.EGL_NO_CONTEXT, intArrayOf(EGL14.EGL_CONTEXT_CLIENT_VERSION, 3, EGL14.EGL_NONE), 0)
            surface = EGL14.eglCreatePbufferSurface(display, configs[0], intArrayOf(EGL14.EGL_WIDTH, 1, EGL14.EGL_HEIGHT, 1, EGL14.EGL_NONE), 0)
            assertTrue(EGL14.eglMakeCurrent(display, surface, surface, context))
            fun texture(w: Int, h: Int, bytes: ByteArray) {
                val id = IntArray(1); GLES30.glGenTextures(1, id, 0); textures.add(id[0]); GLES30.glBindTexture(GLES30.GL_TEXTURE_2D, id[0])
                GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_MIN_FILTER, GLES30.GL_LINEAR); GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_MAG_FILTER, GLES30.GL_LINEAR)
                GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_WRAP_S, GLES30.GL_CLAMP_TO_EDGE); GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D, GLES30.GL_TEXTURE_WRAP_T, GLES30.GL_CLAMP_TO_EDGE)
                GLES30.glTexImage2D(GLES30.GL_TEXTURE_2D, 0, GLES30.GL_SRGB8_ALPHA8, w, h, 0, GLES30.GL_RGBA, GLES30.GL_UNSIGNED_BYTE, ByteBuffer.allocateDirect(bytes.size).put(bytes).apply { flip() })
            }
            texture(1, 1, byteArrayOf(-1, -1, -1, -1)); texture(32, 20, NativeBridge.assetPixels(native, 1)!!)
            gl = GlEffects(info, native, textures); gl.prepare(plan); gl.passes(plan, 0, plan.getInt(12))
            val pass = plan.getInt(20) + (plan.getInt(12) - 1) * 40
            val w = plan.getInt(pass + 16); val h = plan.getInt(pass + 20); val uniform = plan.getInt(pass + 24)
            val rx = plan.getFloat(uniform + 16); val ry = plan.getFloat(uniform + 20)
            val rw = plan.getFloat(uniform + 24); val rh = plan.getFloat(uniform + 28)
            val raw = ByteBuffer.allocateDirect(w * h * 4); GLES30.glReadPixels(0, 0, w, h, GLES30.GL_RGBA, GLES30.GL_UNSIGNED_BYTE, raw)
            assertEquals(GLES30.GL_NO_ERROR, GLES30.glGetError())
            fun channel(x: Int, y: Int, c: Int) = (raw.get((y.coerceIn(0, h - 1) * w + x.coerceIn(0, w - 1)) * 4 + c).toInt() and 255) / 255.0
            fun linear(v: Double) = if (v <= .04045) v / 12.92 else ((v + .055) / 1.055).pow(2.4)
            fun encode(v: Double) = if (v <= .0031308) v * 12.92 else 1.055 * v.pow(1 / 2.4) - .055
            var rgb = 0.0; var alpha = 0.0
            for (y in 0 until 192) for (x in 0 until 192) {
                val px = x + .5 - 80; val py = y + .5 - 86
                val c = DoubleArray(4)
                if (px >= rx && px < rx + rw && py >= ry && py < ry + rh) {
                    val sx = (px - rx) * w / rw - .5; val sy = (py - ry) * h / rh - .5
                    val ix = floor(sx).toInt(); val iy = floor(sy).toInt(); val fx = sx - ix; val fy = sy - iy
                    for (k in 0..3) {
                        fun v(xx: Int, yy: Int) = if (k == 3) channel(xx, yy, k) else linear(channel(xx, yy, k))
                        c[k] = (v(ix, iy) * (1 - fx) + v(ix + 1, iy) * fx) * (1 - fy) + (v(ix, iy + 1) * (1 - fx) + v(ix + 1, iy + 1) * fx) * fy
                    }
                }
                val expected = reference.getPixel(x, y); val channels = intArrayOf(Color.red(expected), Color.green(expected), Color.blue(expected))
                for (k in 0..2) rgb += abs((if (c[3] == 0.0) 0.0 else encode((c[k] / c[3]).coerceIn(0.0, 1.0)) * 255) - channels[k])
                alpha += abs(c[3] * 255 - Color.alpha(expected))
            }
            val report = JSONObject().put("rgbMae", rgb / (192 * 192 * 3)).put("alphaMae", alpha / (192 * 192)).put("region", JSONArray(listOf(rx, ry, rw, rh)))
                .put("renderer", GLES30.glGetString(GLES30.GL_RENDERER))
            assertTrue(report.toString(), report.getDouble("rgbMae") <= 3 && report.getDouble("alphaMae") <= 3)
            return report
        } finally {
            gl?.close(); GLES30.glDeleteTextures(textures.size, textures.toIntArray(), 0)
            EGL14.eglMakeCurrent(display, EGL14.EGL_NO_SURFACE, EGL14.EGL_NO_SURFACE, EGL14.EGL_NO_CONTEXT)
            EGL14.eglDestroySurface(display, surface); EGL14.eglDestroyContext(display, context); EGL14.eglTerminate(display)
        }
    }
}
