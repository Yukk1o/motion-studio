package com.motionstudio.editor

import android.content.Intent
import android.graphics.BitmapFactory
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

class VectorPathOperationsBackendTest {
    private fun data(raw:String)=nativeData(raw)
    private fun constant(value:Double)=JSONObject().put("value",value).put("keys",JSONArray())
    @Test fun trimDashAnimationMatchesGlesAndSurvivesFrozenProjectAndNestedPlayback() {
        val root=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"acceptance/path-ops-${UUID.randomUUID()}").apply{mkdirs()}
        val p=data(NativeBridge.projectTemplate(0)).put("width",64).put("height",64).put("frames",12).put("layers",JSONArray())
            .put("background",JSONArray(listOf(0,0,0,0)))
        p.getJSONObject("camera").put("created",false)
        val id=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),id>0)
        fun command(op:JSONObject)=data(NativeBridge.command(id,op.toString()))
        fun vector(action:JSONObject)=command(JSONObject().put("op","vector").put("object",1).put("action",action))
        fun compare(frame:Int):android.graphics.Bitmap {
            data(NativeBridge.seek(id,frame.toDouble()))
            val reference=BitmapFactory.decodeFile(data(NativeBridge.capture(id)).getString("path"),
                BitmapFactory.Options().apply{inPremultiplied=false;inScaled=false})
            val project=data(NativeBridge.state(id)).getJSONObject("project")
            val info=data(NativeBridge.renderPlanInfo(id))
            val gpu=EglMovieRenderer(null,64,64,project,id,info)
            try {
                if(project.optJSONArray("compositions")?.length()?.let{it>0}==true) {
                    var bundle=ByteBuffer.allocateDirect(info.getInt("composition_bundle_buffer_hint")).order(ByteOrder.nativeOrder())
                    var bytes=CompositionBridge.sampleFrameBundleInto(id,"comp-main",frame.toDouble(),bundle)
                    if(bytes < -1){bundle=ByteBuffer.allocateDirect(-bytes).order(ByteOrder.nativeOrder());bytes=CompositionBridge.sampleFrameBundleInto(id,"comp-main",frame.toDouble(),bundle)}
                    assertTrue(bytes>=32);gpu.prepareBundle(bundle);gpu.drawBundle(bundle)
                }else {
                    val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
                    assertTrue(NativeBridge.sampleRenderPlanInto(id,frame,plan)>0);gpu.draw(plan)
                }
                val pixels=ByteBuffer.allocateDirect(64*64*4);gpu.readPixelsInto(pixels)
                glesParity(reference,pixels)
            }finally{gpu.close()}
            return reference
        }
        try {
            command(JSONObject().put("op","add_shape").put("id",1).put("name","line").put("shape","line")
                .put("size",JSONArray(listOf(40,40))).put("position",JSONArray(listOf(32,32,0))))
            vector(JSONObject().put("action","set_trim").put("trim",JSONObject().put("start",constant(0.0))
                .put("end",constant(50.0)).put("offset",constant(0.0)).put("mode","simultaneously")))
            vector(JSONObject().put("action","set_dashes").put("dashes",JSONObject().put("pattern",JSONArray().put(constant(5.0)).put(constant(5.0)))
                .put("offset",constant(0.0))))
            val caps=data(NativeBridge.state(id)).getJSONObject("capabilities").getJSONObject("vector_drawing")
            assertTrue(caps.getInt("protocol")>=2);assertEquals(11,data(NativeBridge.state(id)).getJSONObject("project").getInt("version"))
            val frozen=data(NativeBridge.state(id)).getJSONObject("project").toString()
            val still=compare(0)
            assertTrue(android.graphics.Color.alpha(still.getPixel(14,32))>200)
            assertEquals(0,android.graphics.Color.alpha(still.getPixel(44,32)))
            for((frame,value)in listOf(0 to 50.0,10 to 100.0))vector(JSONObject().put("action","set_modifier_parameter")
                .put("parameter","trim_end").put("frame",frame).put("value",value).put("animated",true))
            val frames=listOf(0,10,5,0).map(::compare)
            try {assertTrue(frames[0].sameAs(frames[3]));assertFalse(frames[0].sameAs(frames[1]))}
            finally{frames.forEach{it.recycle()}}
            command(JSONObject().put("op","composition").put("action",JSONObject().put("kind","precompose").put("objects",JSONArray(listOf(1)))
                .put("name","nested lines").put("range","composition")))
            compare(5).recycle()
            data(NativeBridge.replace(id,frozen));val restored=compare(0)
            assertTrue(still.sameAs(restored));still.recycle();restored.recycle()
        }finally{NativeBridge.destroy(id)}
    }
}

class VectorPathOperationsUiTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(context.filesDir,"acceptance/path-ops-ui-${UUID.randomUUID()}").apply{mkdirs()}
        val p=nativeData(NativeBridge.projectTemplate(0)).put("width",512).put("height",512).put("frames",120).put("layers",JSONArray())
        p.getJSONObject("camera").put("created",false);File(root,"project.json").writeText(p.toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null}
        scenario.onActivity{vm.addVectorShape("line","线条")}
        settled{vm.vectorData()!=null&&vm.state.saved}
        compose.onNodeWithTag("footer-vector").performScrollTo().performClick()
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun settled(predicate:()->Boolean){compose.waitUntil(15000){vm.state.error!=null||predicate()};assertNull(vm.state.error);assertTrue(predicate())}
    @Test fun trimUsesSharedTimelineEasingAndGestureUndoWithoutReplacingThePath() {
        val source=vm.vectorData()!!.getJSONObject("source").toString()
        compose.onNodeWithTag("vector-tab-operations").performClick()
        compose.onNodeWithTag("vector-trim-toggle").performClick();settled{vm.vectorData()?.optJSONObject("trim")!=null}
        compose.onNodeWithTag("timeline").assertIsDisplayed();compose.onNodeWithTag("preview-gesture").assertIsDisplayed()
        scenario.onActivity{vm.trimClip(vm.selected,0,80);vm.moveClip(vm.selected,20)}
        settled{vm.timelineLayer(vm.selected)?.optInt("offset_frame")==20}
        scenario.onActivity{vm.seek(30.0);vm.selectVectorTrack("vector:trim_end")}
        settled{vm.state.sample?.optDouble("frame")==30.0}
        compose.onNodeWithTag("vector-key").performClick();settled{vm.keys().size==1}
        scenario.onActivity{vm.seek(60.0)};settled{vm.state.sample?.optDouble("frame")==60.0}
        scenario.onActivity{vm.setValue(50.0)};settled{vm.keys().size==2}
        assertEquals(listOf(10,40),vm.vectorTrackRaw(vm.selected,vm.property)!!.getJSONArray("keys").objects().map{it.getInt("frame")})
        assertEquals(listOf(30,60),vm.keys().map{it.getInt("frame")})
        scenario.onActivity{vm.seek(45.0)};settled{(vm.vectorValue(vm.selected,vm.property) as? Number)?.toDouble()==75.0}
        scenario.onActivity{vm.ease("in_out")};settled{vm.keys().first().optString("ease")=="in_out"}
        compose.onNodeWithTag("vector-curve").performClick();compose.onNodeWithText("返回属性").performClick()
        val before=vm.vectorTrackRaw(vm.selected,vm.property).toString()
        compose.onNodeWithTag("vector-wheel-trim_end-0").performScrollTo().performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.SetProgress){it(65f)}
        settled{(vm.vectorValue(vm.selected,vm.property) as? Number)?.toDouble()==65.0}
        scenario.onActivity{vm.undo()};settled{vm.vectorTrackRaw(vm.selected,vm.property).toString()==before}
        assertEquals(source,vm.vectorData()!!.getJSONObject("source").toString())
        compose.waitForIdle();val shot=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"trim-panel.png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()
    }
    @Test fun dashPairsAndAnimatedOffsetUseExistingStyleControls() {
        compose.onNodeWithTag("vector-tab-style").performClick()
        compose.onNodeWithTag("vector-dashes-toggle").performScrollTo().performClick()
        settled{vm.vectorData()?.getJSONObject("stroke")?.optJSONObject("dashes")!=null}
        compose.onNodeWithTag("vector-dash-add").performScrollTo().performClick()
        settled{vm.vectorData()!!.getJSONObject("stroke").getJSONObject("dashes").getJSONArray("pattern").length()==4}
        compose.onNodeWithTag("vector-dash-remove").performScrollTo().performClick()
        settled{vm.vectorData()!!.getJSONObject("stroke").getJSONObject("dashes").getJSONArray("pattern").length()==2}
        scenario.onActivity{vm.selectVectorTrack("vector:dash_offset");vm.addKey()};settled{vm.keys().size==1}
        scenario.onActivity{vm.seek(20.0)};settled{vm.state.sample?.optDouble("frame")==20.0}
        scenario.onActivity{vm.setValue(10.0)};settled{vm.keys().size==2}
        scenario.onActivity{vm.seek(10.0)};settled{(vm.vectorValue(vm.selected,vm.property) as? Number)?.toDouble()==5.0}
        compose.onNodeWithTag("vector-dashes-toggle").performScrollTo().performClick()
        settled{vm.vectorData()!!.getJSONObject("stroke").optJSONObject("dashes")==null}
        scenario.onActivity{vm.undo()};settled{vm.vectorTrackRaw(vm.selected,"vector:dash_offset")?.getJSONArray("keys")?.length()==2}
        scenario.onActivity{vm.save()};settled{vm.state.saved}
        val disk=JSONObject(File(root,"project.json").readText())
        assertEquals(11,disk.getInt("version"));assertEquals(2,disk.getJSONArray("layers").getJSONObject(0).getJSONObject("content")
            .getJSONObject("vector").getJSONObject("stroke").getJSONObject("dashes").getJSONObject("offset").getJSONArray("keys").length())
    }
}
