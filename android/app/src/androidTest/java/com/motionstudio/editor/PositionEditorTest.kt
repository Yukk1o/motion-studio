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

class PositionEditorTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(app.filesDir,"acceptance/position-${UUID.randomUUID()}").apply{mkdirs()}
        val p=nativeData(NativeBridge.projectTemplate(0));p.getJSONObject("camera").put("created",false)
        p.put("width",320).put("height",240).put("frames",90)
        val l=p.getJSONArray("layers").getJSONObject(0).put("id",1).put("three_d",false)
        l.put("size",JSONArray(listOf(32,32))).put("parent",JSONObject.NULL)
        l.getJSONObject("transform").getJSONObject("position").put("value",JSONArray(listOf(20,40,0))).put("keys",JSONArray()
            .put(JSONObject().put("frame",0).put("value",JSONArray(listOf(20,40,0))).put("ease","linear"))
            .put(JSONObject().put("frame",60).put("value",JSONArray(listOf(260,40,0))).put("ease","linear")))
        p.put("layers",JSONArray().put(l))
        val engine=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),engine>0)
        try{nativeData(NativeBridge.save(engine))}finally{NativeBridge.destroy(engine)}
        scenario=ActivityScenario.launch(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null};scenario.onActivity{vm.select(1);vm.openProperty("position")}
    }
    @After fun close(){if(::scenario.isInitialized)scenario.close()}
    private fun track()=vm.layer(1)!!.getJSONObject("transform").getJSONObject("position")
    private fun photo(name:String){compose.waitForIdle();val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot();File(root,"$name.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()}
    @Test fun normalPositionKeepsTheExistingFieldsAndTouchpad() {
        val original=track().toString()
        val preview=compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot
        val timeline=compose.onNodeWithTag("timeline").fetchSemanticsNode().boundsInRoot
        compose.onNodeWithTag("value-X").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("value-Y").assertIsDisplayed()
        compose.onNodeWithTag("value-Z").assertDoesNotExist()
        compose.onNodeWithTag("position-wheel").assertDoesNotExist()
        compose.onNodeWithTag("transform-pad").performScrollTo().performTouchInput{down(Offset(width*.3f,height*.7f));moveBy(Offset(35f,20f),150);up()}
        compose.waitUntil(10000){track().getJSONArray("keys").getJSONObject(0).getJSONArray("value").getDouble(0)>20&&vm.state.saved}
        assertEquals(preview,compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot)
        assertEquals(timeline,compose.onNodeWithTag("timeline").fetchSemanticsNode().boundsInRoot)
        photo("position-pad")
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){track().toString()==original}
        photo("position-axes")
        assertNull(vm.state.error)
    }
    @Test fun draggingSpatialHandleChangesTheActualMidpointAndHasOneUndo() {
        val original=track().toString()
        val overlay=compose.onNodeWithTag("position-preview-overlay")
        // Source is 320x240, fitted and letterboxed in the shared preview.
        compose.waitUntil(10000){overlay.fetchSemanticsNode().config.getOrElse(androidx.compose.ui.semantics.SemanticsProperties.StateDescription){""}=="位置轨迹就绪"}
        compose.waitForIdle()
        compose.onNodeWithTag("open-curves").performClick()
        compose.waitUntil(10000){vm.curvePanelOpen}
        overlay.performTouchInput {
            val fit=minOf(width/320f,height/240f);val left=(width-320*fit)/2;val top=(height-240*fit)/2
            down(Offset(left+100*fit,top+40*fit));moveBy(Offset(0f,80*fit),200);up()
        }
        compose.waitUntil(10000){track().getJSONArray("keys").getJSONObject(0).optJSONObject("spatial")!=null&&vm.state.saved}
        assertEquals(9,vm.state.project!!.getInt("version"))
        photo("position-arc-handles")
        scenario.onActivity{vm.seek(30.0)}
        compose.waitUntil(10000){(vm.sampleValueFor(1,"position") as? JSONArray)?.optDouble(1,40.0)?.let{it>50}==true}
        photo("position-arc-midpoint")
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){track().toString()==original}
        scenario.onActivity{vm.redo()};compose.waitUntil(10000){track().getJSONArray("keys").getJSONObject(0).optJSONObject("spatial")!=null}
        assertNull(vm.state.error)
    }
    @Test fun pathPointsCanOnlyBeAddedFromTheEasingPanel() {
        scenario.onActivity{vm.seek(30.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==30.0}
        val overlay=compose.onNodeWithTag("position-preview-overlay")
        compose.waitUntil(10000){overlay.fetchSemanticsNode().config.getOrElse(androidx.compose.ui.semantics.SemanticsProperties.StateDescription){""}=="位置轨迹就绪"}
        overlay.performTouchInput{val fit=minOf(width/320f,height/240f);click(Offset((width-320*fit)/2+150*fit,(height-240*fit)/2+140*fit))}
        compose.waitForIdle();assertEquals(2,track().getJSONArray("keys").length())
        compose.onNodeWithTag("open-curves").performClick();compose.waitUntil(10000){vm.curvePanelOpen}
        overlay.performTouchInput{val fit=minOf(width/320f,height/240f);click(Offset((width-320*fit)/2+150*fit,(height-240*fit)/2+140*fit))}
        compose.waitUntil(10000){track().getJSONArray("keys").objects().any{it.getInt("frame")==30}&&vm.state.saved}
        val value=track().getJSONArray("keys").objects().first{it.getInt("frame")==30}.getJSONArray("value")
        assertEquals(150.0,value.getDouble(0),.1);assertEquals(140.0,value.getDouble(1),.1)
        compose.onNodeWithContentDescription("返回变换参数").performClick();compose.waitUntil(10000){!vm.curvePanelOpen}
        assertNull(vm.state.error)
    }
}
