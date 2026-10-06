package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.geometry.Offset
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

class VectorFrontendTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(context.filesDir,"acceptance/vector-ui-${UUID.randomUUID()}").apply{mkdirs()}
        val p=JSONObject(NativeBridge.projectTemplate(0)).getJSONObject("data").put("width",512).put("height",512).put("frames",120).put("layers",JSONArray())
        p.getJSONObject("camera").put("created",false)
        File(root,"project.json").writeText(p.toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null}
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun settled(predicate:()->Boolean){compose.waitUntil(15000){vm.state.error!=null||predicate()};assertNull(vm.state.error);assertTrue(predicate())}
    private fun photo(name:String) {
        compose.waitForIdle();InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"$name.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
    }
    @Test fun catalogueCreatesShapeAndParameterAnimationUsesTimelineOffset() {
        assertEquals(25,vm.shapeCatalogue().size)
        compose.onNodeWithTag("add-layer").performClick();compose.onNodeWithTag("add-shapes").performScrollTo().performClick()
        compose.onNodeWithTag("shape-search").performTextInput("星形");photo("shape-catalogue")
        compose.onNodeWithTag("shape-star").performClick();settled{vm.vectorData()!=null}
        compose.onNodeWithTag("footer-vector").performClick();compose.onNodeWithTag("vector-panel").assertIsDisplayed();compose.onNodeWithTag("timeline").assertIsDisplayed();photo("shape-properties")
        scenario.onActivity{vm.trimClip(vm.selected,0,80);vm.moveClip(vm.selected,20)}
        settled{vm.timelineLayer(vm.selected)?.optInt("offset_frame")==20}
        scenario.onActivity{vm.seek(30.0);vm.selectVectorTrack("vector:parameter:inner_ratio")}
        settled{vm.state.sample?.optDouble("frame")==30.0}
        scenario.onActivity{vm.addKey()};settled{vm.keys().size==1}
        scenario.onActivity{vm.seek(60.0)};settled{vm.state.sample?.optDouble("frame")==60.0}
        scenario.onActivity{vm.setValue(.2)};settled{vm.keys().size==2}
        assertEquals(listOf(10,40),vm.vectorTrackRaw(vm.selected,vm.property)!!.getJSONArray("keys").objects().map{it.getInt("frame")})
        assertEquals(listOf(30,60),vm.keys().map{it.getInt("frame")})
        scenario.onActivity{vm.seek(45.0)};settled{vm.state.sample?.optDouble("frame")==45.0}
        scenario.onActivity{vm.ease("in_out")};settled{vm.keys().first().optString("ease")=="in_out"}
        compose.onNodeWithTag("vector-curve").performClick();photo("shape-curve")
        scenario.onActivity{vm.closeWorkspace();vm.save()};settled{vm.state.saved}
        val disk=JSONObject(File(root,"project.json").readText())
        assertEquals(2,disk.getJSONArray("layers").getJSONObject(0).getJSONObject("content").getJSONObject("vector").getJSONObject("source").getJSONObject("parameters").getJSONObject("inner_ratio").getJSONArray("keys").length())
    }
    @Test fun shapeConversionRequiresConfirmationAndStyleIsUndoable() {
        scenario.onActivity{vm.addVectorShape("ellipse","椭圆")};settled{vm.vectorData()!=null}
        compose.onNodeWithTag("footer-vector-style").performClick();photo("vector-style")
        compose.onNodeWithTag("vector-stroke-toggle").performScrollTo().performClick();settled{vm.vectorData()?.optJSONObject("stroke")!=null}
        scenario.onActivity{vm.undo()};settled{vm.vectorData()?.optJSONObject("stroke")==null}
        compose.onNodeWithTag("vector-tab-geometry").performClick();compose.onNodeWithTag("vector-convert").performScrollTo().performClick()
        assertEquals("shape",vm.vectorData()!!.getJSONObject("source").getString("kind"))
        compose.onNodeWithTag("vector-convert-confirm").performClick();settled{vm.vectorData()?.getJSONObject("source")?.optString("kind")=="paths"}
        compose.onNodeWithTag("vector-preview-overlay").assertIsDisplayed();photo("path-editor")
        scenario.onActivity{vm.undo()};settled{vm.vectorData()?.getJSONObject("source")?.optString("kind")=="shape"}
    }
    @Test fun penNodesAndGeometryGesturesCancelAndUndoTogether() {
        scenario.onActivity{vm.addPenLayer()};settled{vm.vectorData()?.getJSONObject("source")?.optString("kind")=="paths"}
        val overlay=compose.onNodeWithTag("vector-preview-overlay")
        overlay.performTouchInput{click(Offset(width*.36f,height*.55f))};settled{vm.vectorData()!!.getJSONObject("source").getJSONArray("paths").getJSONObject(0).getJSONArray("nodes").length()==1}
        overlay.performTouchInput{swipe(Offset(width*.65f,height*.55f),Offset(width*.72f,height*.65f),400)};settled{vm.vectorData()!!.getJSONObject("source").getJSONArray("paths").getJSONObject(0).getJSONArray("nodes").length()==2}
        val before=vm.vectorData().toString();val key="vector:node:${vm.vectorPathId}:${vm.vectorNodeId}"
        val original=vm.vectorValue(vm.selected,key) as JSONArray
        scenario.onActivity{vm.beginGesture();vm.setVectorNode(vm.vectorPathId,vm.vectorNodeId,JSONArray(original.toString()).put(0,50),0);vm.cancelGesture()}
        settled{vm.vectorData().toString()==before}
        scenario.onActivity{vm.beginGesture();vm.setVectorNode(vm.vectorPathId,vm.vectorNodeId,JSONArray(original.toString()).put(0,30),0);vm.setVectorNode(vm.vectorPathId,vm.vectorNodeId,JSONArray(original.toString()).put(0,80),0);vm.endGesture()}
        settled{(vm.vectorValue(vm.selected,key) as JSONArray).getDouble(0)==80.0}
        photo("path-handles")
        scenario.onActivity{vm.undo()};settled{vm.vectorData().toString()==before}
    }
    @Test fun adjustmentOpensDockedEffectsAndErrorDialogOffersReport() {
        compose.onNodeWithTag("add-layer").performClick();compose.onNodeWithTag("add-adjustment").performScrollTo().performClick()
        settled{vm.contentKind()=="adjustment"};assertTrue(vm.effectsOpen);compose.onNodeWithTag("timeline").assertIsDisplayed();photo("adjustment-effects")
        scenario.onActivity{vm.showOperationError("诊断入口测试")}
        compose.onNodeWithTag("export-error-report").assertIsDisplayed();photo("error-report-entry")
    }
}
