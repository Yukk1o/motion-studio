package com.motionstudio.editor

import android.app.Application
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.util.UUID

class ErrorReportsTest {
    @Test fun reportsRedactLocationsAndSecretsAndSurviveRestartWithinRetentionLimit() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as Application
        val folder=File(app.cacheDir,"reports-test-${UUID.randomUUID()}")
        val reports=ErrorReports(app,folder)
        repeat(60){reports.operation("edit:set_scalar")}
        val file=reports.record("import failed content://secret/clip.mp4 /storage/emulated/0/private.mp4 token=private-token",IllegalStateException("file:///private/clip.mp4"))!!
        val text=file.readText();val json=JSONObject(text)
        assertFalse(text.contains("private-token"));assertFalse(text.contains("content://secret"));assertFalse(text.contains("private.mp4"));assertFalse(text.contains("file:///private"))
        assertFalse(json.getBoolean("includes_project_or_media"));assertEquals(40,json.getJSONArray("recent_operations").length());assertTrue(json.has("stack"))
        repeat(12){reports.record("error $it")}
        assertEquals(8,folder.listFiles()!!.count{it.extension=="json"})
        assertEquals(reports.latest()!!.name,ErrorReports(app,folder).export().name)
    }
    @Test fun duplicateFailuresAreCoalescedAndOnlyOperationNamesAreCaptured() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as Application
        val folder=File(app.cacheDir,"reports-test-${UUID.randomUUID()}")
        val reports=ErrorReports(app,folder)
        val first=reports.record("repeat")!!;repeat(10){reports.record("repeat")}
        assertEquals(1,folder.listFiles()!!.count{it.extension=="json"});assertEquals(first,reports.latest())
        assertEquals("password=[已隐藏] api-key=[已隐藏]",redactDiagnostic("password=secret api-key:secret"))
    }
}
