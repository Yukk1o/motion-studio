package com.motionstudio.editor

import android.graphics.BitmapFactory
import android.content.Intent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.Before
import org.junit.After
import org.junit.Rule
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

class VectorGroupsBackendTest {
    private fun data(raw:String)=nativeData(raw)
    private fun track(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
    @Test fun groupedAlphaRepeatersAndViewportCropMatchUnencodedGles() {
        val root=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"acceptance/groups-${UUID.randomUUID()}").apply{mkdirs()}
        val p=data(NativeBridge.projectTemplate(0)).put("width",64).put("height",64).put("frames",30).put("layers",JSONArray()).put("background",JSONArray(listOf(0,0,0,0)))
        p.getJSONObject("camera").put("created",false)
        val id=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),id>0)
        fun command(c:JSONObject)=data(NativeBridge.command(id,c.toString()))
        fun vector(action:JSONObject)=command(JSONObject().put("op","vector").put("object",1).put("action",action))
        fun current()=data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONObject("content").getJSONObject("vector")
        fun compare(frame:Int=0):ByteBuffer {
            data(NativeBridge.seek(id,frame.toDouble()))
            val ref=BitmapFactory.decodeFile(data(NativeBridge.capture(id)).getString("path"),BitmapFactory.Options().apply{inPremultiplied=false;inScaled=false})
            val project=data(NativeBridge.state(id)).getJSONObject("project");val info=data(NativeBridge.renderPlanInfo(id))
            assertEquals(9,info.getInt("version"));val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
            val gpu=EglMovieRenderer(null,64,64,project,id,info);val pixels=ByteBuffer.allocateDirect(64*64*4)
            try {assertTrue(NativeBridge.sampleRenderPlanInto(id,frame,plan)>0);gpu.draw(plan);gpu.readPixelsInto(pixels);glesParity(ref,pixels)}
            finally{gpu.close();ref.recycle()}
            return pixels
        }
        try {
            command(JSONObject().put("op","add_shape").put("id",1).put("name","Group").put("shape","rectangle")
                .put("size",JSONArray(listOf(20,20))).put("position",JSONArray(listOf(32,32,0))))
            vector(JSONObject().put("action","convert_to_group"))
            vector(JSONObject().put("action","set_group_parameter").put("item",1).put("parameter","opacity").put("frame",0).put("value",50))
            val half=compare();assertTrue(kotlin.math.abs((half.get((32*64+32)*4+3).toInt() and 255)-128)<=2)
            val v=current();val g=v.getJSONObject("source").getJSONObject("group")
            val child=JSONObject(g.toString()).put("id",5).put("name","Child")
            child.getJSONArray("items").getJSONObject(0).put("id",6)
            g.put("items",JSONArray().put(JSONObject().put("kind","group").put("group",child)))
            vector(JSONObject().put("action","replace").put("vector",v));val nested=compare()
            assertTrue(kotlin.math.abs((nested.get((32*64+32)*4+3).toInt() and 255)-64)<=2)
            val copy=JSONObject().put("copies",track(23)).put("offset",track(0)).put("position",track(JSONArray(listOf(1064,0))))
                .put("anchor",track(JSONArray(listOf(0,0)))).put("scale",track(JSONArray(listOf(100,100)))).put("rotation",track(0))
                .put("start_opacity",track(100)).put("end_opacity",track(100)).put("composite","below")
            g.getJSONArray("items").put(JSONObject().put("kind","repeater").put("id",7).put("name","X copies").put("repeater",copy))
                .put(JSONObject().put("kind","repeater").put("id",8).put("name","Y copies").put("repeater",JSONObject(copy.toString()).put("position",track(JSONArray(listOf(0,844))))))
            vector(JSONObject().put("action","replace").put("vector",v));compare()
            command(JSONObject().put("op","set_vector").put("object",1).put("property","position").put("frame",0).put("value",JSONArray(listOf(-1032,-812,0))))
            compare()
            assertEquals(12,data(NativeBridge.state(id)).getJSONObject("project").getInt("version"))
        }finally{NativeBridge.destroy(id)}
    }
}

class VectorGroupsUiTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(context.filesDir,"acceptance/groups-ui-${UUID.randomUUID()}").apply{mkdirs()}
        val p=nativeData(NativeBridge.projectTemplate(0)).put("width",512).put("height",512).put("frames",120).put("layers",JSONArray())
        p.getJSONObject("camera").put("created",false);File(root,"project.json").writeText(p.toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]};compose.waitUntil(20000){vm.state.project!=null}
        scenario.onActivity{vm.addVectorShape("rectangle","矩形")};settled{vm.vectorData()!=null&&vm.state.saved}
        compose.onNodeWithTag("footer-vector").performScrollTo().performClick()
        compose.onNodeWithTag("vector-convert-group").performScrollTo().performClick()
        settled{vm.vectorData()?.getJSONObject("source")?.optString("kind")=="group"}
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun settled(predicate:()->Boolean){compose.waitUntil(15000){vm.state.error!=null||predicate()};assertNull(vm.state.error);assertTrue(predicate())}
    private fun group()=vm.vectorData()!!.getJSONObject("source").getJSONObject("group")
    @Test fun groupPositionUsesSharedTouchpadAndUndoWhileKeepingPreviewAndTimeline() {
        compose.onNodeWithTag("preview-gesture").assertIsDisplayed();compose.onNodeWithTag("timeline").assertIsDisplayed()
        val before=group().getJSONObject("transform").getJSONObject("position").toString()
        compose.onNodeWithTag("group-pad-vector:group:1:position").performScrollTo().performClick()
        compose.onNodeWithTag("group-touchpad-vector:group:1:position").performScrollTo().performTouchInput{
            swipe(center,Offset(center.x+60f,center.y),400)
        }
        settled{group().getJSONObject("transform").getJSONObject("position").getJSONArray("value").getDouble(0)!=0.0}
        scenario.onActivity{vm.undo()};settled{group().getJSONObject("transform").getJSONObject("position").toString()==before}
        compose.waitForIdle();val shot=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"group-position.png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()
    }
    @Test fun addedRepeaterSharesKeyframesCurveEditorAndStoredLocalTime() {
        compose.onNodeWithTag("group-add").performScrollTo().performClick();compose.onNodeWithTag("group-add-repeater").performClick()
        settled{group().getJSONArray("items").length()==2}
        compose.onNodeWithTag("group-item-3").performScrollTo().performClick()
        compose.onNodeWithTag("vector-select-group:3:copies-0").performScrollTo().performClick()
        scenario.onActivity{vm.trimClip(vm.selected,0,80);vm.moveClip(vm.selected,20)};settled{vm.timelineLayer(vm.selected)?.optInt("offset_frame")==20}
        scenario.onActivity{vm.seek(30.0)};settled{vm.state.sample?.optDouble("frame")==30.0}
        compose.onNodeWithTag("vector-key").performClick();settled{vm.keys().size==1}
        scenario.onActivity{vm.seek(60.0)};settled{vm.state.sample?.optDouble("frame")==60.0}
        scenario.onActivity{vm.setValue(5.0)};settled{vm.keys().size==2}
        assertEquals(listOf(10,40),vm.vectorTrackRaw(vm.selected,"vector:group:3:copies")!!.getJSONArray("keys").objects().map{it.getInt("frame")})
        scenario.onActivity{vm.seek(45.0)};settled{(vm.vectorValue(vm.selected,"vector:group:3:copies") as? Number)?.toDouble()==4.0}
        scenario.onActivity{vm.ease("in_out")};settled{vm.keys().first().optString("ease")=="in_out"}
        compose.onNodeWithTag("vector-curve").performClick();compose.onNodeWithText("返回属性").performClick()
        compose.onNodeWithTag("vector-select-group:3:copies-0").assertExists()
        scenario.onActivity{vm.save()};settled{vm.state.saved}
        assertEquals(12,JSONObject(File(root,"project.json").readText()).getInt("version"))
        compose.waitForIdle();val shot=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"group-repeater.png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()
    }
}
