package com.motionstudio.editor

import android.content.Intent
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
import java.util.UUID

class VectorGroupContentsTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(context.filesDir,"acceptance/group-contents-${UUID.randomUUID()}").apply{mkdirs()}
        val p=nativeData(NativeBridge.projectTemplate(0)).put("width",512).put("height",512).put("frames",120).put("layers",JSONArray())
        p.getJSONObject("camera").put("created",false);File(root,"project.json").writeText(p.toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]};compose.waitUntil(20000){vm.state.project!=null}
        scenario.onActivity{vm.addVectorShape("rectangle","路径")};settled{vm.vectorData()!=null&&vm.state.saved}
        scenario.onActivity{vm.vectorAction(JSONObject().put("action","convert_to_path").put("frame",0))}
        settled{vm.vectorData()!!.getJSONObject("source").getString("kind")=="paths"}
        scenario.onActivity{vm.vectorAction(JSONObject().put("action","convert_to_group"));vm.openVector()}
        settled{vm.vectorData()!!.getJSONObject("source").getString("kind")=="group"}
        val capabilities=vm.state.sample!!.getJSONObject("capabilities").getJSONObject("vector_drawing").getJSONObject("groups")
        assertEquals(6,capabilities.getJSONObject("node_parameters").getInt("components"));assertTrue(capabilities.getBoolean("stroke_dash_parameters"))
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun settled(predicate:()->Boolean){compose.waitUntil(15000){vm.state.error!=null||predicate()};assertNull(vm.state.error);assertTrue(predicate())}
    private fun group()=vm.vectorData()!!.getJSONObject("source").getJSONObject("group")
    private fun screenshot(name:String) {
        compose.waitForIdle();val shot=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"$name.png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()
    }
    @Test fun pathNodeAxesAndHandlesUseSharedTimelineTouchpadAndUndo() {
        val key="vector:group:2:node:1:1"
        compose.onNodeWithTag("group-item-2").performScrollTo().performClick()
        compose.onNodeWithTag("vector-wheel-group:2:size-0").assertDoesNotExist()
        compose.onNodeWithTag("vector:group:2-nodes").performScrollTo()
        compose.onNodeWithTag("vector:group:2-node-1-1").assertIsDisplayed().performClick().assertIsSelected()
        settled{vm.property==key}
        compose.onNodeWithTag("preview-gesture").assertIsDisplayed();compose.onNodeWithTag("timeline").assertIsDisplayed()
        val before=vm.vectorTrackRaw(vm.selected,key)!!.toString()
        compose.onNodeWithTag("vector-wheel-group:2:node:1:1-0").performScrollTo().performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.SetProgress){it(-80f)}
        settled{vm.vectorTrackRaw(vm.selected,key)!!.getJSONArray("value").getDouble(0)==-80.0}
        scenario.onActivity{vm.undo()};settled{vm.vectorTrackRaw(vm.selected,key).toString()==before}
        compose.onNodeWithTag("group-pad-$key").performScrollTo().performClick()
        compose.onNodeWithTag("group-touchpad-$key").performScrollTo().performTouchInput{swipe(center,center+androidx.compose.ui.geometry.Offset(50f,0f),400)}
        settled{vm.vectorTrackRaw(vm.selected,key).toString()!=before}
        scenario.onActivity{vm.undo()};settled{vm.vectorTrackRaw(vm.selected,key).toString()==before}
        compose.onNodeWithTag("group-pad-$key").performScrollTo().performClick()
        scenario.onActivity{vm.trimClip(vm.selected,0,80);vm.moveClip(vm.selected,20)};settled{vm.timelineLayer(vm.selected)?.optInt("offset_frame")==20}
        scenario.onActivity{vm.seek(30.0)};settled{vm.state.sample?.optDouble("frame")==30.0}
        compose.onNodeWithTag("vector-key").performClick();settled{vm.keys().size==1}
        scenario.onActivity{vm.seek(60.0)};settled{vm.state.sample?.optDouble("frame")==60.0}
        compose.onNodeWithTag("vector-wheel-group:2:node:1:1-4").performScrollTo().performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.SetProgress){it(30f)}
        settled{vm.keys().size==2}
        assertEquals(listOf(10,40),vm.vectorTrackRaw(vm.selected,key)!!.getJSONArray("keys").objects().map{it.getInt("frame")})
        scenario.onActivity{vm.seek(45.0)};settled{(vm.vectorValue(vm.selected,key) as? JSONArray)?.getDouble(4)==15.0}
        scenario.onActivity{vm.ease("in_out")};settled{vm.keys().first().optString("ease")=="in_out"}
        compose.onNodeWithTag("vector-curve").performClick();compose.onNodeWithText("返回属性").performClick()
        compose.onNodeWithTag("vector-wheel-group:2:node:1:1-4").assertExists()
        scenario.onActivity{vm.save()};settled{vm.state.saved}
        compose.onNodeWithTag("vector-wheel-group:2:node:1:1-1").performScrollTo()
        screenshot("group-path-node")
        val saved=JSONObject(File(root,"project.json").readText());assertEquals(12,saved.getInt("version"))
        assertEquals(2,vm.vectorTrackRaw(vm.selected,key)!!.getJSONArray("keys").length())
        compose.onNodeWithTag("vector:group:2-delete-node").performScrollTo().performClick()
        settled{group().getJSONArray("items").getJSONObject(0).getJSONObject("vector").getJSONObject("source").getJSONArray("paths").getJSONObject(0).getJSONArray("nodes").length()==3}
        assertEquals("vector:group:2:position",vm.property)
        scenario.onActivity{vm.undo()};settled{vm.vectorTrackRaw(vm.selected,key)?.getJSONArray("keys")?.length()==2}
    }
    @Test fun separateStrokeReusesDashPairsCapJoinAndAnimatedOffsetControls() {
        compose.onNodeWithTag("group-add").performScrollTo().performClick();compose.onNodeWithTag("group-add-stroke").performClick()
        settled{group().getJSONArray("items").length()==2}
        compose.onNodeWithTag("group-item-3").performScrollTo().performClick()
        fun stroke()=group().getJSONArray("items").getJSONObject(1).getJSONObject("stroke")
        compose.onNodeWithText("平头").performScrollTo().performClick();settled{stroke().getString("cap")=="butt"}
        compose.onNodeWithText("尖角").performScrollTo().performClick();settled{stroke().getString("join")=="miter"}
        compose.onNodeWithTag("vector:group:3-dashes-toggle").performScrollTo().performClick();settled{stroke().optJSONObject("dashes")!=null}
        compose.onNodeWithTag("vector:group:3-dash-add").performScrollTo().performClick();settled{stroke().getJSONObject("dashes").getJSONArray("pattern").length()==4}
        compose.onNodeWithTag("vector:group:3-dash-remove").performScrollTo().performClick();settled{stroke().getJSONObject("dashes").getJSONArray("pattern").length()==2}
        val key="vector:group:3:dash_offset"
        scenario.onActivity{vm.trimClip(vm.selected,0,80);vm.moveClip(vm.selected,20)};settled{vm.timelineLayer(vm.selected)?.optInt("offset_frame")==20}
        scenario.onActivity{vm.seek(30.0);vm.selectVectorTrack(key)};settled{vm.state.sample?.optDouble("frame")==30.0}
        compose.onNodeWithTag("vector-key").performClick();settled{vm.keys().size==1}
        scenario.onActivity{vm.seek(60.0)};settled{vm.state.sample?.optDouble("frame")==60.0}
        scenario.onActivity{vm.setValue(10.0)};settled{vm.keys().size==2}
        scenario.onActivity{vm.seek(45.0)};settled{(vm.vectorValue(vm.selected,key) as? Number)?.toDouble()==5.0}
        val before=vm.vectorTrackRaw(vm.selected,key).toString()
        compose.onNodeWithTag("vector-wheel-group:3:dash_offset-0").performScrollTo().performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.SetProgress){it(12f)}
        settled{(vm.vectorValue(vm.selected,key) as? Number)?.toDouble()==12.0}
        scenario.onActivity{vm.undo()};settled{vm.vectorTrackRaw(vm.selected,key).toString()==before}
        screenshot("group-stroke-dashes")
        compose.onNodeWithTag("vector:group:3-dashes-toggle").performScrollTo().performClick();settled{stroke().optJSONObject("dashes")==null}
        assertEquals("vector:group:3:width",vm.property)
        scenario.onActivity{vm.undo()};settled{vm.vectorTrackRaw(vm.selected,key)?.getJSONArray("keys")?.length()==2}
    }
}
