package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.view.PixelCopy
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.Rule
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.roundToInt

@RunWith(AndroidJUnit4::class)
class OrthographicPreviewTest {
    @get:Rule val compose = createEmptyComposeRule()
    private fun data(raw: String): JSONObject {
        val response = JSONObject(raw)
        assertTrue(response.optString("error"), response.getBoolean("ok"))
        return response.getJSONObject("data")
    }
    private fun vector(vararg values: Number) = JSONArray(values.toList())
    private fun track(value: Any) = JSONObject().put("value", value).put("keys", JSONArray())

    private fun check(view: Int, x: Int, y: Int) {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val root = File(context.filesDir, "acceptance/orthographic-surface-" + UUID.randomUUID()).apply { mkdirs() }
        val project = data(NativeBridge.projectTemplate(0)).put("version", 3)
            .put("width", 256).put("height", 256).put("background", vector(0, 0, 0, 1))
            .put("assets", JSONArray())
        val layers = JSONArray()
        for (id in 1..2) {
            val offset = if (id == 1) 400 else 500
            val angle = if (id == 1) 10 else 20
            val size = if (view == 2) vector(128, 2048) else vector(2048, 128)
            val position = if (view == 2) vector(128, 128 - offset, 0) else vector(128 + offset, 128, 0)
            val rotation = if (view == 2) vector(angle, 0, 0) else vector(0, angle, 0)
            layers.put(JSONObject().put("id", id).put("name", "large-plane-$id").put("size", size)
                .put("three_d", true).put("visible", true).put("locked", false)
                .put("content", JSONObject().put("kind", "solid").put("color", if (id == 1) vector(1, 0, 0, 1) else vector(0, 0, 1, 1)))
                .put("transform", JSONObject().put("position", track(position)).put("rotation", track(rotation))
                    .put("scale", track(vector(100, 100, 100))).put("opacity", track(1)).put("anchor", vector(.5, .5))))
        }
        project.put("layers", layers)
        val session = NativeBridge.create(root.absolutePath, project.toString())
        assertTrue(NativeBridge.creationError(), session > 0)
        val hits: JSONArray
        try {
            data(NativeBridge.view(session, view))
            hits = data(GeometryBridge.hitCandidates(session, x.toDouble(), y.toDouble())).getJSONArray("candidates")
            assertEquals(2, hits.length())
            assertEquals(2, hits.getJSONObject(0).getInt("id"))
            data(NativeBridge.save(session))
        } finally { NativeBridge.destroy(session) }

        val scenario = ActivityScenario.launch<AcceptanceActivity>(Intent(context, AcceptanceActivity::class.java)
            .putExtra("projectDirectory", root.absolutePath))
        val report = JSONArray()
        try {
            lateinit var vm: EditorViewModel
            scenario.onActivity { vm = ViewModelProvider(it)[EditorViewModel::class.java] }
            compose.waitUntil(20000) { vm.state.project != null }
            scenario.onActivity { vm.view(view) }
            for (opacity in listOf(1.0, .5)) {
                scenario.onActivity { vm.setPropertyValue(2, "opacity", 0, opacity, false) }
                compose.waitUntil(15000) {
                    scenario.onActivity { vm.refreshDiagnostics() }
                    assertNull(vm.state.error)
                    val state = vm.state.sample
                    val applied = vm.layer(2)?.getJSONObject("transform")?.getJSONObject("opacity")?.getDouble("value") == opacity
                    state != null && applied && state.optString("observationView") == (if (view == 2) "top" else "side") &&
                        !state.isNull("graphics") && !state.isNull("lastPresentedFrame") &&
                        state.getDouble("lastPresentedFrame") == vm.frame &&
                        state.getLong("lastPresentedRevision") == state.getLong("revision") &&
                        state.getLong("lastPresentedViewRevision") == state.getLong("viewRevision")
                }
                compose.waitForIdle()
                val bitmap = copySurface(scenario)
                try {
                    val scale = minOf(bitmap.width / 256.0, bitmap.height / 256.0)
                    val px = (bitmap.width / 2.0 + (x - 128) * scale).roundToInt()
                    val py = (bitmap.height / 2.0 + (y - 128) * scale).roundToInt()
                    val pixel = bitmap.getPixel(px, py)
                    val actual = listOf(Color.red(pixel), Color.green(pixel), Color.blue(pixel), Color.alpha(pixel))
                    val expected = if (opacity == 1.0) listOf(0, 0, 255, 255) else listOf(188, 0, 188, 255)
                    report.put(JSONObject().put("view", view).put("opacity", opacity).put("pixel", JSONArray(actual))
                        .put("expected", JSONArray(expected)).put("surfacePixel", vector(px, py)))
                    File(root, "preview-report.json").writeText(JSONObject().put("samples", report).put("pick", hits).toString(2))
                    File(root, "preview-$opacity.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
                    for (i in 0..3) assertTrue("View $view, opacity $opacity: $actual != $expected", kotlin.math.abs(actual[i] - expected[i]) <= 3)
                } finally { bitmap.recycle() }
            }
        } finally { scenario.close() }
    }

    private fun copySurface(scenario: ActivityScenario<AcceptanceActivity>): Bitmap {
        fun find(view: View): SurfaceView? {
            if (view is SurfaceView) return view
            if (view is ViewGroup) for (i in 0 until view.childCount) find(view.getChildAt(i))?.let { return it }
            return null
        }
        var surface: SurfaceView? = null
        scenario.onActivity { surface = find(it.window.decorView) }
        val view = surface ?: error("Native preview surface is missing")
        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
        val done = CountDownLatch(1)
        var result = -1
        scenario.onActivity { PixelCopy.request(view, bitmap, { result = it; done.countDown() }, Handler(Looper.getMainLooper())) }
        assertTrue(done.await(10, TimeUnit.SECONDS))
        assertEquals(PixelCopy.SUCCESS, result)
        return bitmap
    }

    @Test fun topPreviewUsesParallelOcclusionForOpaqueAndTranslucentPlanes() = check(2, 128, 218)
    @Test fun sidePreviewUsesParallelOcclusionForOpaqueAndTranslucentPlanes() = check(3, 218, 128)
}
