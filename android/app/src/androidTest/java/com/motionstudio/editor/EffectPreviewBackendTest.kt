package com.motionstudio.editor

import android.graphics.ImageFormat
import android.hardware.HardwareBuffer
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import java.util.concurrent.atomic.AtomicInteger

@RunWith(AndroidJUnit4::class)
class EffectPreviewBackendTest {
    private fun data(raw: String): JSONObject = JSONObject(raw).let {
        assertTrue(it.toString(), it.optBoolean("ok")); it.getJSONObject("data")
    }
    @Test fun repeatedBindingDisconnectsTheOldProducerBeforeCreatingAnotherSurface() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val root=File(context.filesDir,"acceptance/surface-rebind-${UUID.randomUUID()}").apply{mkdirs()}
        val project=data(NativeBridge.projectTemplate(0)).put("width",96).put("height",64).put("layers",JSONArray())
        project.getJSONObject("camera").put("created",false)
        val id=NativeBridge.create(root.absolutePath,project.toString())
        assertTrue(NativeBridge.creationError(),id>0)
        val consumer=HandlerThread("surface-rebind-consumer").apply{start()}
        val reader=ImageReader.newInstance(96,64,ImageFormat.PRIVATE,3,HardwareBuffer.USAGE_GPU_SAMPLED_IMAGE)
        reader.setOnImageAvailableListener({it.acquireLatestImage()?.close()},Handler(consumer.looper))
        try {
            for(frame in 0..2) {
                data(NativeBridge.surface(id,reader.surface,96,64))
                assertTrue(NativeBridge.state(id),NativeBridge.render(id,frame.toDouble()))
                assertTrue(data(NativeBridge.state(id)).isNull("renderError"))
            }
            assertFalse(JSONObject(NativeBridge.surface(id,reader.surface,0,64)).getBoolean("ok"))
            assertTrue("Invalid dimensions must preserve the valid preview",NativeBridge.render(id,3.0))
        } finally {
            data(NativeBridge.surface(id,null,0,0));NativeBridge.destroy(id)
            reader.setOnImageAvailableListener(null,null)
            consumer.quitSafely();consumer.join(3000);assertFalse(consumer.isAlive);reader.close()
        }
    }
    @Test fun fourKLightingAndBlurFitTheSurfaceWithoutChangingFormalPlansOrProject() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val root = File(context.filesDir, "acceptance/effect-preview-${UUID.randomUUID()}").apply { mkdirs() }
        val p = data(NativeBridge.projectTemplate(0)).put("width", 3840).put("height", 2160).put("fps", 30)
        p.getJSONObject("camera").put("created", false)
        val layer = p.getJSONArray("layers").getJSONObject(1)
        layer.put("three_d", false).put("size", JSONArray(listOf(3840, 2160)))
        layer.getJSONObject("transform").getJSONObject("position").put("value", JSONArray(listOf(1920, 1080, 0)))
        p.put("layers", JSONArray().put(layer))
        val id = NativeBridge.create(root.absolutePath, p.toString())
        assertTrue(NativeBridge.creationError(), id > 0)
        val consumer = HandlerThread("effect-preview-consumer").apply { start() }
        val reader = ImageReader.newInstance(640, 360, ImageFormat.PRIVATE, 3, HardwareBuffer.USAGE_GPU_SAMPLED_IMAGE)
        val received=AtomicInteger()
        reader.setOnImageAvailableListener({ it.acquireLatestImage()?.use{received.incrementAndGet()} }, Handler(consumer.looper))
        try {
            data(NativeBridge.surface(id, reader.surface, 640, 360))
            val packages = data(NativeBridge.plugin(id, "{\"op\":\"catalogue\"}")).getJSONArray("packages")
            val pkg = (0 until packages.length()).map { packages.getJSONObject(it) }.first {
                it.getJSONObject("manifest").getString("version") == "1.4.0"
            }
            val m = pkg.getJSONObject("manifest")
            for (name in listOf("rays", "glow", "glow_edges", "glint", "directional_blur")) {
                data(NativeBridge.plugin(id, JSONObject().put("op", "add").put("object", 2)
                    .put("plugin", m.getString("id")).put("version", m.getString("version"))
                    .put("hash", pkg.getString("hash")).put("effect", name).toString()))
            }
            val effects = data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers")
                .getJSONObject(0).getJSONArray("effects")
            data(NativeBridge.command(id, JSONObject().put("op", "effect").put("object", 2)
                .put("action", JSONObject().put("kind", "set").put("effect", effects.getJSONObject(4).getLong("id"))
                    .put("param", "p0002").put("frame", 0).put("value", JSONArray(listOf(100, 0, 0, 0)))).toString()))
            val frozen = data(NativeBridge.state(id)).getJSONObject("project").toString()
            // Formal full-layer scratch limits stay explicit. Preview fitting must
            // not make a frozen export silently accept reduced effect resolution.
            val formalBefore = JSONObject(NativeBridge.renderPlanInfo(id))
            assertFalse(formalBefore.toString(), formalBefore.getBoolean("ok"))
            assertTrue(formalBefore.getString("error").contains("scratch"))
            for (mode in listOf(0, 2, 3)) {
                data(NativeBridge.previewMode(id, mode, 0))
                assertTrue(NativeBridge.state(id), NativeBridge.render(id, 0.0))
                val state = data(NativeBridge.state(id))
                assertEquals(frozen, state.getJSONObject("project").toString())
                val preview = state.getJSONObject("preview")
                val graphics = state.getJSONObject("graphics")
                assertEquals(if (mode == 0) 640 else 427, graphics.getInt("renderWidth"))
                assertEquals(if (mode == 0) 360 else 240, graphics.getInt("renderHeight"))
                assertEquals("projected_2d", preview.getString("effectResolution"))
                assertTrue(preview.getBoolean("surfaceBounded"))
                assertEquals(0, graphics.getInt("previewImageReadbackBytes"))
                assertTrue(state.toString(), state.isNull("renderError"))
                assertEquals(state.toString(), 0, state.getJSONArray("effectErrors").length())
                assertEquals(formalBefore.toString(), JSONObject(NativeBridge.renderPlanInfo(id)).toString())
            }
            data(NativeBridge.previewMode(id, 1, 0))
            val high = data(NativeBridge.previewInfo(id))
            assertEquals(3840, high.getInt("width")); assertEquals(2160, high.getInt("height"))
            assertEquals("full_layer", high.getString("effectResolution")); assertFalse(high.getBoolean("surfaceBounded"))
            data(NativeBridge.previewMode(id, 0, 0))
            val timingActive = data(NativeBridge.previewInfo(id)).getBoolean("gpuTimingActive")
            data(NativeBridge.startProfiling(id, 60))
            assertTrue(NativeBridge.state(id), NativeBridge.render(id, 1.0))
            data(NativeBridge.stopProfiling(id))
            assertEquals(timingActive, data(NativeBridge.previewInfo(id)).getBoolean("gpuTimingActive"))
            val deadline=android.os.SystemClock.elapsedRealtime()+3000
            while(received.get()==0&&android.os.SystemClock.elapsedRealtime()<deadline)Thread.sleep(10)
            assertTrue("Preview must deliver a buffer to its Surface",received.get()>0)
        } finally {
            data(NativeBridge.surface(id, null, 0, 0)); NativeBridge.destroy(id)
            reader.setOnImageAvailableListener(null,null)
            consumer.quitSafely(); consumer.join(3000)
            assertFalse("Preview consumer must stop before its reader is closed",consumer.isAlive)
            reader.close()
        }
    }
}
