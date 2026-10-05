package com.motionstudio.editor

import android.content.Intent
import android.os.Bundle
import android.os.Debug
import android.os.SystemClock
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.io.ByteArrayOutputStream
import java.util.UUID
import java.util.zip.GZIPOutputStream
import android.util.Base64

@RunWith(AndroidJUnit4::class)
class ResourceLifecycleTest {
    @get:Rule val compose=createEmptyComposeRule()
    private fun data(raw:String):JSONObject {val r=JSONObject(raw);assertTrue(r.optString("error"),r.optBoolean("ok"));return r.getJSONObject("data")}
    private fun info(root:File)=data(NativeBridge.resourceInfo(root.absolutePath))
    private fun memory():JSONObject {
        val mem=Debug.MemoryInfo();Debug.getMemoryInfo(mem)
        val threads=File("/proc/self/task").listFiles()?.mapNotNull{runCatching{File(it,"comm").readText().trim()}.getOrNull()}?:emptyList()
        return JSONObject().put("pssKiB",mem.totalPss).put("javaUsedBytes",Runtime.getRuntime().totalMemory()-Runtime.getRuntime().freeMemory())
            .put("nativeHeapBytes",Debug.getNativeHeapAllocatedSize()).put("openDescriptors",File("/proc/self/fd").list()?.size?:-1)
            .put("renderWorkerThreads",threads.count{it.startsWith("motion-render")})
    }
    private fun median(values:List<Int>)=values.sorted().let{it[it.size/2]}
    private fun emit(report:JSONObject) {
        val buffer=ByteArrayOutputStream();GZIPOutputStream(buffer).use{it.write(report.toString().toByteArray(Charsets.UTF_8))}
        Base64.encodeToString(buffer.toByteArray(),Base64.NO_WRAP).chunked(32000).forEachIndexed{i,chunk->
            InstrumentationRegistry.getInstrumentation().sendStatus(0,Bundle().apply{putInt("resourceIndex",i);putString("resourceChunk",chunk)})}
    }
    @Test fun repeatedRealEditorOpenCloseReleasesScopedSessionsTexturesAndWorkers() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        val args=InstrumentationRegistry.getArguments()
        val seconds=(args.getString("durationSeconds")?:"30").toInt().coerceIn(15,600)
        val root=File(app.filesDir,"acceptance/resources-"+UUID.randomUUID()).apply{mkdirs()}
        PerformanceFixture.create(root)
        val original=File(root,"project.json").readBytes()
        val baseline=memory();val samples=JSONArray();val started=SystemClock.elapsedRealtime()
        var cycle=0
        while(SystemClock.elapsedRealtime()-started<seconds*1000L||cycle<6) {
            val scenario=ActivityScenario.launch<AcceptanceActivity>(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
            lateinit var vm:EditorViewModel
            try {
                scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
                compose.waitUntil(20000){vm.state.project!=null}
                scenario.onActivity{vm.choosePreviewMode(1);vm.seek(0.0)}
                compose.waitUntil(20000){scenario.onActivity{vm.refreshDiagnostics()};val s=vm.state.sample
                    s!=null&&!s.isNull("lastPresentedFrame")&&s.getDouble("lastPresentedFrame")==0.0&&s.getLong("lastPresentedRevision")==s.getLong("revision")}
                val opened=info(root);assertEquals(1,opened.getInt("sessions"));assertEquals(1,opened.getInt("graphics"))
                assertTrue(opened.getLong("assetTextureBytes")>0)
                scenario.onActivity{vm.togglePlay()};Thread.sleep(1000)
                scenario.onActivity{vm.pause()}
                compose.waitUntil(10000){!vm.playing&&vm.state.sample?.optDouble("frame")==vm.frame}
                assertNull("Cycle $cycle preview failed",vm.state.error)
                if(cycle%3==0){scenario.moveToState(Lifecycle.State.CREATED);scenario.moveToState(Lifecycle.State.RESUMED)
                    compose.waitUntil(15000){scenario.onActivity{vm.refreshDiagnostics()};val s=vm.state.sample
                        s!=null&&!s.isNull("graphics")&&!s.isNull("lastPresentedFrame")&&s.getDouble("lastPresentedFrame")==vm.frame}}
                samples.put(memory().put("cycle",cycle).put("phase","open").put("elapsedMs",SystemClock.elapsedRealtime()-started).put("owned",info(root)))
                assertArrayEquals(original,File(root,"project.json").readBytes())
            }finally{scenario.close()}
            compose.waitUntil(15000){vm.isClosed&&info(root).getInt("sessions")==0}
            var after=memory();val settle=SystemClock.elapsedRealtime()+10000
            while(after.getInt("renderWorkerThreads")>baseline.getInt("renderWorkerThreads")&&SystemClock.elapsedRealtime()<settle){Thread.sleep(50);after=memory()}
            val closed=info(root)
            assertEquals(0,closed.getInt("sessions"));assertEquals(0,closed.getInt("graphics"));assertEquals(0,closed.getLong("assetTextureBytes"));assertEquals(0,closed.getLong("renderTargetBytes"))
            assertTrue("Render workers were retained",after.getInt("renderWorkerThreads")<=baseline.getInt("renderWorkerThreads"))
            samples.put(after.put("cycle",cycle).put("phase","closed").put("elapsedMs",SystemClock.elapsedRealtime()-started).put("owned",closed))
            cycle++
            File(root,"resource-lifecycle.json").writeText(JSONObject().put("samples",samples).put("cycles",cycle).put("baseline",baseline).toString(2))
            Thread.sleep(1000)
        }
        val closed=(0 until samples.length()).map{samples.getJSONObject(it)}.filter{it.getString("phase")=="closed"}
        val window=(closed.size/3).coerceAtMost(10).coerceAtLeast(1)
        val warm=closed.drop(2).take(window);val late=closed.takeLast(window)
        val growth=median(late.map{it.getInt("pssKiB")})-median(warm.map{it.getInt("pssKiB")})
        val fdGrowth=median(late.map{it.getInt("openDescriptors")})-median(warm.map{it.getInt("openDescriptors")})
        val report=JSONObject().put("cycles",cycle).put("elapsedMs",SystemClock.elapsedRealtime()-started).put("samples",samples).put("baseline",baseline)
            .put("warmToLatePssMedianGrowthKiB",growth).put("warmToLateDescriptorGrowth",fdGrowth)
            .put("debuggable",app.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE!=0)
            .put("scope","Application-owned resources and this device/process; PSS excludes some driver allocations; bounded growth does not prove absence of every leak")
        File(root,"resource-lifecycle.json").writeText(report.toString(2));emit(report)
        assertTrue("PSS grew beyond the 32 MiB measurement guard: $growth KiB",growth<=32*1024)
        assertTrue("Descriptor growth beyond guard: $fdGrowth",fdGrowth<=16)
        if(args.getString("requireNonDebuggable")=="true")assertFalse(report.getBoolean("debuggable"))
    }
}
