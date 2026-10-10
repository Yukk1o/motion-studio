package com.motionstudio.editor

import android.graphics.Bitmap
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import java.util.zip.ZipFile

@RunWith(AndroidJUnit4::class)
class PackageBackendApiTest {
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private fun data(raw: String): JSONObject {
        val envelope = JSONObject(raw)
        assertTrue(envelope.optString("error"), envelope.optBoolean("ok"))
        return envelope.getJSONObject("data")
    }
    private fun req(id: Long, op: String, key: String) = data(MediaBridge.request(id, context,
        JSONObject().put("op", op).put("request_id", key).toString()))
    private fun await(id: Long, key: String): JSONObject {
        val deadline = System.nanoTime() + 45_000_000_000L
        while (true) {
            val task = req(id, "media_status", key)
            if (task.getString("state") != "running") return task
            assertTrue("Package timeout: $task", System.nanoTime() < deadline)
            Thread.sleep(5)
        }
    }
    private fun root() = File(context.filesDir, "acceptance/package-${UUID.randomUUID()}").apply { mkdirs() }
    private fun create(root: File): Long {
        val p = data(NativeBridge.projectTemplate(0)).put("layers", JSONArray())
        return NativeBridge.create(root.absolutePath, p.toString()).also { assertTrue(it > 0) }
    }

    private fun assertJsonEquals(expected: Any, actual: Any, path: String = "$") {
        when {
            expected is JSONObject && actual is JSONObject -> {
                val keys = expected.keys().asSequence().toSet()
                assertEquals(path, keys, actual.keys().asSequence().toSet())
                keys.forEach { assertJsonEquals(expected.get(it), actual.get(it), "$path.$it") }
            }
            expected is JSONArray && actual is JSONArray -> {
                assertEquals(path, expected.length(), actual.length())
                for (i in 0 until expected.length()) assertJsonEquals(expected.get(i), actual.get(i), "$path[$i]")
            }
            expected is Number && actual is Number -> {
                // Project fields round-trip through Rust f32; JSON Value and typed
                // serialization can spell the same f32 differently. Integers stay exact.
                if (expected is Double || expected is Float || actual is Double || actual is Float)
                    assertEquals(path, expected.toFloat().toBits(), actual.toFloat().toBits())
                else assertEquals(path, expected.toLong(), actual.toLong())
            }
            else -> assertEquals(path, expected, actual)
        }
    }

    @Test fun limitsAreAvailableWithoutASessionAndMatchCapabilityQuery() {
        val limits = data(MediaBridge.packageLimits())
        assertEquals(1, limits.getInt("schema_version"))
        assertEquals(2L * 1024 * 1024 * 1024, limits.getLong("max_source_bytes"))
        assertEquals(4L * 1024 * 1024 * 1024 + 4 * 1024 * 1024, limits.getLong("max_archive_bytes"))
        assertEquals(65536, limits.getInt("transfer_buffer_bytes"))
        assertTrue(limits.getBoolean("frozen_export"))
        val root = root(); val id = create(root)
        try {
            val caps = data(MediaBridge.request(id, context, "{\"op\":\"media_capabilities\"}"))
            assertEquals(limits.toString(), caps.getJSONObject("project_package").toString())
            assertEquals(limits.getLong("max_source_bytes"), caps.getLong("max_source_bytes"))
        } finally { NativeBridge.destroy(id) }
    }

    @Test fun sharedVideoSourceAndFrozenProjectSurviveEditsAndProjectSwitch() {
        val root = root(); val id = create(root)
        try {
            data(MediaBridge.request(id, context, JSONObject().put("op", "import_media")
                .put("request_id", "video").put("kind", "video").put("uri",
                    "content://com.motionstudio.editor.test.audio-fixtures/sound-24fps.mp4").toString()))
            assertEquals("ready", await(id, "video").getString("state"))
            req(id, "finish_media_import", "video")
            val before = data(NativeBridge.state(id)).getJSONObject("project")
            val initial = req(id, "export_project", "pack")
            assertEquals("running", initial.getString("state"))
            assertFalse(initial.has("path"))
            assertEquals(1, initial.getInt("source_count"))
            // Existing owner-thread operations remain usable while the payload worker runs.
            data(NativeBridge.command(id, "{\"op\":\"flags\",\"object\":1,\"visible\":false,\"locked\":false}"))
            val empty = data(NativeBridge.projectTemplate(0)).put("layers", JSONArray())
            data(NativeBridge.newProject(id, empty.toString()))
            val done = await(id, "pack")
            assertEquals(done.toString(), "succeeded", done.getString("state"))
            assertEquals(initial.getLong("frozen_revision"), done.getLong("frozen_revision"))
            assertEquals(done.getLong("total_bytes"), done.getLong("bytes_processed"))
            val output = File(done.getString("path"))
            ZipFile(output).use { zip ->
                assertEquals(2, zip.size())
                val path = before.getJSONArray("video_assets").getJSONObject(0).getString("path")
                assertEquals(path, before.getJSONArray("audio_assets").getJSONObject(0).getString("path"))
                assertEquals(java.util.zip.ZipEntry.STORED, zip.getEntry(path).method)
                val frozen = JSONObject(zip.getInputStream(zip.getEntry("project.json")).bufferedReader().use { it.readText() })
                assertJsonEquals(before, frozen)
            }
            assertEquals("succeeded", req(id, "cancel_media_task", "pack").getString("state"))
            req(id, "release_media_task", "pack")
            data(NativeBridge.importProject(id, output.absolutePath))
            assertJsonEquals(before, data(NativeBridge.state(id)).getJSONObject("project"))
        } finally { NativeBridge.destroy(id) }
    }

    @Test fun cancellationAndTaskReleaseWaitForTemporaryFileCleanup() {
        val root = root(); val id = create(root)
        try {
            val source = File(root, "assets/image.png").apply { parentFile!!.mkdirs() }
            val bitmap = Bitmap.createBitmap(32, 32, Bitmap.Config.ARGB_8888)
            source.outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
            bitmap.recycle()
            // Valid PNG with trailing padding: exercise streaming without a giant bitmap.
            java.io.FileOutputStream(source, true).use { out ->
                val chunk = ByteArray(64 * 1024)
                repeat(512) { out.write(chunk) }
            }
            data(NativeBridge.command(id, JSONObject().put("op", "register_asset").put("asset",
                JSONObject().put("id", 7).put("path", "assets/image.png").put("width", 32).put("height", 32)).toString()))
            req(id, "export_project", "cancel")
            val cancelled = req(id, "cancel_media_task", "cancel")
            val done = await(id, "cancel")
            assertTrue(done.toString(), done.getString("state") in listOf("cancelled", "succeeded"))
            if (cancelled.getString("state") != "succeeded") {
                assertEquals("cancelled", done.getString("state")); assertFalse(done.has("path"))
                assertEquals(0, File(root, "exports").listFiles()?.count { it.extension == "msproj" } ?: 0)
            }
            assertFalse(File(root, "exports").listFiles()?.any { it.name.startsWith(".ms-package-") } ?: false)
            req(id, "release_media_task", "cancel")
        } finally { NativeBridge.destroy(id) }
    }

    /** Opt-in local data; no large or private video is packaged into the test APK. */
    @Test fun optionalLargeVideoImportsThroughTheRealAndroidDecoder() {
        val path = InstrumentationRegistry.getArguments().getString("large_video_path")
        org.junit.Assume.assumeTrue("Pass -e large_video_path with a readable local MP4/AAC", path != null)
        val source = File(path!!)
        val bytes = source.length()
        assertTrue(bytes > 512L * 1024 * 1024)
        assertTrue(bytes <= data(MediaBridge.packageLimits()).getLong("max_source_bytes"))
        val root = root(); var id = create(root); var ownedSource: File? = null
        try {
            val started = System.nanoTime()
            data(MediaBridge.request(id, context, JSONObject().put("op", "import_media")
                .put("request_id", "large").put("kind", "video").put("uri", "file://${source.absolutePath}").toString()))
            val requestMs = (System.nanoTime() - started) / 1_000_000
            var task: JSONObject
            val deadline = System.nanoTime() + 600_000_000_000L
            do {
                task = req(id, "media_status", "large")
                assertTrue("Large import timeout: $task", System.nanoTime() < deadline)
                if (task.getString("state") == "running") Thread.sleep(20)
            } while (task.getString("state") == "running")
            assertEquals(task.toString(), "ready", task.getString("state"))
            val imported = req(id, "finish_media_import", "large")
            assertEquals(imported.toString(), "succeeded", imported.getJSONObject("task").getString("state"))
            val project = imported.getJSONObject("state").getJSONObject("project")
            val video = project.getJSONArray("video_assets").getJSONObject(0)
            val audio = project.getJSONArray("audio_assets").getJSONObject(0)
            assertEquals(bytes, video.getLong("bytes"))
            assertEquals(video.getString("path"), audio.getString("path"))
            assertEquals(1, File(root, "assets").listFiles()!!.size)
            ownedSource = File(root, video.getString("path"))
            assertEquals(bytes, ownedSource!!.length())
            val report = JSONObject().put("bytes", bytes).put("request_ms", requestMs)
                .put("import_and_commit_ms", (System.nanoTime() - started) / 1_000_000)
                .put("video", video).put("audio", audio).put("shared_source", true)
                .put("native_heap_bytes_after_commit", android.os.Debug.getNativeHeapAllocatedSize())
                .put("process_pss_kib_after_commit", android.os.Debug.getPss())
            data(NativeBridge.save(id)); NativeBridge.destroy(id); id = 0
            id = NativeBridge.create(root.absolutePath, ""); assertTrue(id > 0)
            assertEquals(bytes, data(NativeBridge.state(id)).getJSONObject("project")
                .getJSONArray("video_assets").getJSONObject(0).getLong("bytes"))
            report.put("reopened", true)
            File(root, "large-import-report.json").writeText(report.toString(2))
            android.util.Log.i("MotionLargeMedia", report.toString())
        } finally {
            NativeBridge.destroy(id)
            // Exact source created by this test's import; preserve report/cache for inspection.
            ownedSource?.delete()
        }
    }
}
