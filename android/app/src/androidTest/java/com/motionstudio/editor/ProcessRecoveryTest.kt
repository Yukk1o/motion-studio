package com.motionstudio.editor

import android.content.Intent
import android.graphics.BitmapFactory
import android.os.Bundle
import android.os.Process
import android.util.Base64
import androidx.compose.ui.test.junit4.createEmptyComposeRule
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
import java.security.MessageDigest
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

/** Invoked in two different application processes by validate_stability.py. */
@RunWith(AndroidJUnit4::class)
class ProcessRecoveryTest {
    @get:Rule val compose=createEmptyComposeRule()
    private fun root():File {
        val args=InstrumentationRegistry.getArguments();val token=args.getString("recoveryToken")?:error("Use the process recovery host script")
        require(token.matches(Regex("[A-Za-z0-9_-]+")))
        return File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"acceptance/"+token).apply{mkdirs()}
    }
    private fun hash(file:File)=MessageDigest.getInstance("SHA-256").digest(file.readBytes()).joinToString(""){"%02x".format(it.toInt() and 255)}
    private fun launch(root:File):Pair<ActivityScenario<AcceptanceActivity>,EditorViewModel> {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        val scenario=ActivityScenario.launch<AcceptanceActivity>(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        lateinit var vm:EditorViewModel
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null};return scenario to vm
    }
    private fun awaitFrame(scenario:ActivityScenario<AcceptanceActivity>,vm:EditorViewModel,frame:Double) {
        scenario.onActivity{vm.seek(frame)}
        compose.waitUntil(20000){scenario.onActivity{vm.refreshDiagnostics()};val s=vm.state.sample
            s!=null&&!s.isNull("lastPresentedFrame")&&s.getDouble("lastPresentedFrame")==frame&&s.getLong("lastPresentedRevision")==s.getLong("revision")}
    }
    private fun capture(scenario:ActivityScenario<AcceptanceActivity>,vm:EditorViewModel,target:File) {
        var captured=false;scenario.onActivity{vm.output(true){it.copyTo(target,true);captured=true}}
        compose.waitUntil(20000){captured};assertNull(vm.state.error)
    }
    @Test fun prepareAndWaitForExternalProcessTermination() {
        val root=root();PerformanceFixture.create(root)
        val (scenario,vm)=launch(root)
        try {
            scenario.onActivity{vm.addNull()}
            compose.waitUntil(15000){vm.state.project!!.getJSONArray("layers").length()==21}
            val parent=vm.selected
            val curve=JSONObject().put("space","velocity").put("shape",JSONObject().put("kind","cubic")
                .put("control1",JSONArray(listOf(.2,1.5))).put("control2",JSONArray(listOf(.8,2.0))).put("start",0).put("end",0))
            val commands=JSONArray()
                .put(JSONObject().put("op","parent").put("object",0).put("parent",parent).put("frame",72))
                .put(JSONObject().put("op","curve").put("object",2).put("property","position").put("frame",0)
                    .put("easing",JSONObject().put("ease","linear").put("curve",curve)))
            scenario.onActivity{vm.editBatch(commands)}
            compose.waitUntil(15000){vm.state.project!!.getJSONObject("camera").optJSONObject("parent")?.optLong("object")==parent&&vm.state.saved}
            awaitFrame(scenario,vm,72.0);capture(scenario,vm,File(root,"before-termination.png"))
            val expected=JSONObject().put("pid",Process.myPid()).put("project",vm.state.project).put("savedJsonSha256",hash(File(root,"project.json")))
                .put("assets",JSONArray(File(root,"assets").listFiles()!!.sortedBy{it.name}.map{JSONObject().put("name",it.name).put("sha256",hash(it))}))
            File(root,"expected-recovery.json").writeText(expected.toString(2))
            InstrumentationRegistry.getInstrumentation().sendStatus(0,Bundle().apply{putString("recoveryReady",Base64.encodeToString(JSONObject().put("pid",Process.myPid()).put("token",root.name).toString().toByteArray(),Base64.NO_WRAP))})
            assertFalse("Host did not terminate the confirmed test process",CountDownLatch(1).await(60,TimeUnit.SECONDS))
            fail("Process termination did not occur")
        }finally{scenario.close()}
    }
    @Test fun reopenAfterProcessTerminationPreservesProjectAssetsAndPixels() {
        val root=root();val expected=JSONObject(File(root,"expected-recovery.json").readText())
        assertNotEquals("Expected a new process",expected.getInt("pid"),Process.myPid())
        val (scenario,vm)=launch(root)
        try {
            assertEquals(expected.getJSONObject("project").toString(),vm.state.project!!.toString())
            assertEquals(expected.getString("savedJsonSha256"),hash(File(root,"project.json")))
            val assets=expected.getJSONArray("assets")
            for(i in 0 until assets.length()){val a=assets.getJSONObject(i);assertEquals(a.getString("sha256"),hash(File(root,"assets/"+a.getString("name"))))}
            awaitFrame(scenario,vm,72.0);capture(scenario,vm,File(root,"after-termination.png"))
            val before=BitmapFactory.decodeFile(File(root,"before-termination.png").absolutePath)
            val after=BitmapFactory.decodeFile(File(root,"after-termination.png").absolutePath)
            try{assertTrue("Reopened composition changed pixels",before.sameAs(after))}finally{before.recycle();after.recycle()}
            File(root,"process-recovery-report.json").writeText(JSONObject().put("beforePid",expected.getInt("pid")).put("afterPid",Process.myPid())
                .put("projectAndAssetsPreserved",true).put("frame72PixelsIdentical",true).put("durationFrames",vm.state.project!!.getInt("frames")).toString(2))
        }finally{scenario.close()}
    }
}
