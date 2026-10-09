package com.motionstudio.editor

import android.graphics.Bitmap
import android.graphics.Color
import android.graphics.PixelFormat
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

@RunWith(AndroidJUnit4::class)
class ImagePreviewCacheTest {
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private fun data(raw: String) = nativeData(raw)
    private fun values(vararg values: Any) = JSONArray(values.toList())
    private fun track(value: Any) = JSONObject().put("value", value).put("keys", JSONArray())
    private fun image(id: Int, asset: Int, start: Int, end: Int) = JSONObject()
        .put("id", id).put("name", "image $id").put("visible", true).put("locked", false)
        .put("size", values(64, 64)).put("content", JSONObject().put("kind", "image").put("asset", asset))
        .put("timeline", JSONObject().put("in_frame", start).put("out_frame", end).put("offset_frame", 0))
        .put("transform", JSONObject().put("position", track(values(32, 32, 0)))
            .put("rotation", track(values(0, 0, 0))).put("scale", track(values(100, 100, 100)))
            .put("opacity", track(1)).put("anchor", values(.5, .5)))
    private fun png(root: File, id: Int): JSONObject {
        val path = "assets/$id.png"
        val file = File(root, path).apply { parentFile!!.mkdirs() }
        val bitmap = Bitmap.createBitmap(32, 16, Bitmap.Config.ARGB_8888)
        bitmap.eraseColor(if (id == 100) Color.RED else Color.BLUE)
        file.outputStream().use { assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) }
        bitmap.recycle()
        return JSONObject().put("id", id).put("path", path).put("width", 32).put("height", 16)
    }
    private fun create(root: File, assets: JSONArray, layers: JSONArray): Long {
        val p = data(NativeBridge.projectTemplate(0)).put("width", 64).put("height", 64)
            .put("fps", 60).put("frames", 120).put("assets", assets).put("layers", layers)
        p.getJSONObject("camera").put("created", false)
        return NativeBridge.create(root.absolutePath, p.toString()).also { assertTrue(it > 0) }
    }
    private fun preview(id: Long, action: () -> Unit) {
        val consumer = HandlerThread("image-preview-consumer").apply { start() }
        val reader = ImageReader.newInstance(64, 64, PixelFormat.RGBA_8888, 3)
        reader.setOnImageAvailableListener({ it.acquireLatestImage()?.close() }, Handler(consumer.looper))
        try {
            data(NativeBridge.surface(id, reader.surface, 64, 64))
            data(NativeBridge.previewMode(id, 2, 0))
            action()
        } finally {
            data(NativeBridge.surface(id, null, 0, 0))
            reader.setOnImageAvailableListener(null, null)
            consumer.quitSafely()
            consumer.join(3000)
            assertFalse(consumer.isAlive)
            reader.close()
        }
    }
    private fun await(message: String, check: () -> Boolean) {
        val end = System.nanoTime() + 20_000_000_000L
        while (!check()) { assertTrue(message, System.nanoTime() < end); Thread.sleep(5) }
    }

    @Test fun previewPreparesUpcomingImageAndReverseSeekDoesNotUploadAgain() {
        val root = File(context.filesDir, "image-preview-cache/${UUID.randomUUID()}").apply { mkdirs() }
        val id = create(root, values(png(root, 100), png(root, 101)),
            values(image(1, 100, 0, 6), image(2, 101, 6, 120)))
        try {
            preview(id) {
                await("initial image") { NativeBridge.render(id, 0.0) }
                await("upcoming image") {
                    assertTrue(NativeBridge.state(id), NativeBridge.render(id, 0.0))
                    data(NativeBridge.previewInfo(id)).getLong("imageUploadBytes") == 2L * 32 * 16 * 4
                }
                val before = data(NativeBridge.previewInfo(id))
                assertEquals(1L, before.getLong("imagePrefetches"))
                assertEquals(32L * 16 * 4, before.getLong("imageIdleBytes"))
                assertTrue("prefetched clip was not ready", NativeBridge.render(id, 7.0))
                assertTrue("reverse seek reloaded the image", NativeBridge.render(id, 0.0))
                val after = data(NativeBridge.previewInfo(id))
                assertEquals(before.getLong("imageUploadBytes"), after.getLong("imageUploadBytes"))
                assertEquals(2L, after.getLong("imageMemoryCacheHits"))
                assertEquals(2L, after.getLong("imageDecodes"))
                data(NativeBridge.previewMode(id, 1, 0))
                assertTrue(NativeBridge.render(id, 0.0))
                assertEquals(0L, data(NativeBridge.previewInfo(id)).getLong("imageIdleBytes"))
                File(root, "preview-cache-report.json").writeText(after.toString())
            }
        } finally { NativeBridge.destroy(id) }
    }

    @Test fun coldImageAndVideoRequestsStartOnTheSameRenderAttempt() {
        val root = File(context.filesDir, "image-preview-cache/${UUID.randomUUID()}").apply { mkdirs() }
        val id = create(root, values(png(root, 100)), values(image(1, 100, 0, 120)))
        fun media(op: String, vararg fields: Pair<String, Any>) = data(MediaBridge.request(id, context,
            JSONObject().put("op", op).apply { fields.forEach { put(it.first, it.second) } }.toString()))
        try {
            media("import_media", "kind" to "video", "request_id" to "video",
                "uri" to "content://com.motionstudio.editor.test.audio-fixtures/silent-24fps.mp4", "with_audio" to false)
            await("video import") { media("media_status", "request_id" to "video").getString("state") != "running" }
            assertEquals("ready", media("media_status", "request_id" to "video").getString("state"))
            media("finish_media_import", "request_id" to "video")
            preview(id) {
                // Starting the first asynchronous image always returns false;
                // nevertheless the video stream must already have been started.
                assertFalse(NativeBridge.render(id, 0.0))
                val pending = data(NativeBridge.previewInfo(id))
                assertEquals(0L, pending.getLong("imageDecodes"))
                assertEquals(1, pending.getJSONObject("video").getInt("streams"))
                await("mixed preview") { NativeBridge.render(id, 0.0) }
                val ready = data(NativeBridge.previewInfo(id))
                assertEquals(1L, ready.getLong("imageDecodes"))
                assertEquals(1L, ready.getJSONObject("video").getLong("gpuConversions"))
                assertTrue(NativeBridge.render(id, 0.0))
                assertEquals(ready.getLong("imageUploadBytes"), data(NativeBridge.previewInfo(id)).getLong("imageUploadBytes"))
                File(root, "parallel-input-report.json").writeText(JSONObject().put("pending", pending).put("ready", ready).toString())
            }
        } finally { NativeBridge.destroy(id) }
    }
}
