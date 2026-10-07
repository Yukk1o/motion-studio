package com.motionstudio.editor

import android.content.Intent
import android.graphics.BitmapFactory
import android.os.Bundle
import android.os.Debug
import android.os.PowerManager
import android.util.Base64
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import java.io.ByteArrayOutputStream
import java.util.zip.GZIPOutputStream
import kotlin.math.abs

@RunWith(AndroidJUnit4::class)
class PreviewPerformanceTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(app.filesDir,"acceptance/performance-"+UUID.randomUUID()).apply{mkdirs()}
        PerformanceFixture.create(root)
        scenario=ActivityScenario.launch(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null};scenario.onActivity{vm.choosePreviewMode(1)};awaitPresented()
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun awaitPresented(mode:Int?=null){compose.waitUntil(15000){scenario.onActivity{vm.refreshDiagnostics()};val d=vm.state.sample
        val tierMatches=mode==null||(d?.optJSONObject("preview")?.optString("mode")==listOf("", "high","balanced","economy")[mode]&&
            d.optJSONObject("graphics")?.optInt("renderWidth")==d.optJSONObject("preview")?.optInt("width"))
        d!=null&&tierMatches&&!d.isNull("lastPresentedFrame")&&d.getDouble("lastPresentedFrame")==vm.frame&&d.getLong("lastPresentedRevision")==d.getLong("revision")&&
            d.getLong("lastPresentedViewRevision")==d.getLong("viewRevision")}}
    @Test fun previewTiersKeepTheCameraProjectAndFullResolutionPng() {
        scenario.onActivity{vm.seek(72.0);vm.save()};awaitPresented()
        val project=vm.state.project!!.toString();val file=File(root,"project.json").readBytes();var reference:ByteArray?=null
        for(mode in 1..3) {
            scenario.onActivity{vm.choosePreviewMode(mode)}
            compose.waitUntil(10000){vm.previewInfo?.optString("mode")==listOf("", "high","balanced","economy")[mode]};awaitPresented(mode)
            val g=vm.state.sample!!.getJSONObject("graphics")
            if(mode==1){assertEquals(1080,g.getInt("renderWidth"));assertEquals(1920,g.getInt("renderHeight"))}
            else {
                assertTrue(g.getInt("renderWidth")<=720&&g.getInt("renderWidth")<=g.getInt("width"))
                assertTrue(g.getInt("renderHeight")<=1280&&g.getInt("renderHeight")<=g.getInt("height"))
                assertTrue(abs(g.getInt("renderWidth")/g.getInt("renderHeight").toDouble()-1080.0/1920.0)<0.005)
            }
            assertEquals(0,g.getInt("previewImageReadbackBytes"));assertEquals(project,vm.state.project!!.toString());assertArrayEquals(file,File(root,"project.json").readBytes())
            var png:File?=null;scenario.onActivity{vm.output(true){png=it}};compose.waitUntil(15000){png!=null}
            val bitmap=BitmapFactory.decodeFile(png!!.absolutePath);assertEquals(1080,bitmap.width);assertEquals(1920,bitmap.height);bitmap.recycle()
            val bytes=png!!.readBytes();if(reference==null)reference=bytes else assertArrayEquals(reference,bytes)
        }
    }
    @Test fun fixedTwentyLayerWorkloadRecordsCpuGpuAndMemoryWithoutFrameImageReadbacks() {
        val args=InstrumentationRegistry.getArguments();val seconds=(args.getString("durationSeconds")?:"5").toInt().coerceIn(1,600)
        scenario.onActivity{vm.togglePlay()};Thread.sleep(2000)
        var started=false;scenario.onActivity{vm.startProfiling(40000){started=true}};compose.waitUntil(15000){started}
        val samples=org.json.JSONArray();val began=android.os.SystemClock.elapsedRealtime()
        while(android.os.SystemClock.elapsedRealtime()-began<seconds*1000L) {
            val memory=Debug.MemoryInfo();Debug.getMemoryInfo(memory)
            val pm=InstrumentationRegistry.getInstrumentation().targetContext.getSystemService(PowerManager::class.java)
            samples.put(JSONObject().put("elapsedMs",android.os.SystemClock.elapsedRealtime()-began).put("pssKiB",memory.totalPss).put("thermalStatus",pm.currentThermalStatus))
            Thread.sleep(1000)
        }
        var report:File?=null;scenario.onActivity{vm.pause();vm.stopProfiling{report=it}};compose.waitUntil(20000){report!=null}
        val json=JSONObject(report!!.readText());assertTrue(json.getInt("frameCount")>seconds*5);assertEquals(0,json.getInt("overflowFrames"))
        val metadata=json.getJSONObject("metadata");assertEquals(20,metadata.getInt("layerCount"));assertEquals(1080,metadata.getJSONObject("graphics").getInt("renderWidth"))
        assertEquals(0,metadata.getJSONObject("graphics").getInt("previewImageReadbackBytes"))
        var refreshRate=0f;scenario.onActivity{refreshRate=it.windowManager.defaultDisplay.refreshRate}
        val debuggable=InstrumentationRegistry.getInstrumentation().targetContext.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE!=0
        if(args.getString("requireNonDebuggable")=="true")assertFalse("Performance target must be non-debuggable",debuggable)
        json.put("android",JSONObject().put("debuggable",debuggable).put("requestedDurationSeconds",seconds).put("memoryAndThermal",samples).put("displayRefreshHz",refreshRate))
        json.put("workload",JSONObject(File(root,"workload.json").readText()))
        report!!.writeText(json.toString())
        val buffer=ByteArrayOutputStream();GZIPOutputStream(buffer).use{it.write(json.toString().toByteArray(Charsets.UTF_8))}
        val encoded=Base64.encodeToString(buffer.toByteArray(),Base64.NO_WRAP)
        encoded.chunked(32000).forEachIndexed{i,chunk->InstrumentationRegistry.getInstrumentation().sendStatus(0,Bundle().apply{putInt("perfIndex",i);putString("perfChunk",chunk)})}
    }
}
