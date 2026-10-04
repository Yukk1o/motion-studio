package com.motionstudio.editor

import android.app.Application
import android.graphics.Bitmap
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.lifecycle.ViewModelStore
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class MotionInteractionTest {
    @get:Rule val compose=createComposeRule()
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    private val store=ViewModelStore()
    private var density=1f
    @Before fun setup() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as Application
        density=app.resources.displayMetrics.density
        root=File(app.filesDir,"acceptance/interaction-"+UUID.randomUUID())
        compose.runOnUiThread{vm=EditorViewModel(app,root);store.put("interaction",vm)}
        compose.setContent{StudioTheme{Editor(vm)}}
        compose.waitUntil(20000){vm.state.project!=null}
        compose.waitForIdle()
    }
    @After fun teardown(){compose.runOnUiThread{store.clear()}}
    private fun position()=vm.layer(2)!!.getJSONObject("transform").getJSONObject("position").getJSONArray("value")
    private fun scopeKeys(key:String="position")=vm.layer(2)!!.getJSONObject("transform").getJSONObject(key).getJSONArray("keys")
    private fun seek(frame:Double) {
        compose.runOnIdle{vm.seek(frame)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==frame}
        compose.waitForIdle()
    }
    private fun photo(name:String) {
        compose.waitForIdle()
        InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,name+".png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
    }

    @Test fun selectingLayersAndScrubbingDoesNotOpenTheInspector() {
        val timeline=compose.onNodeWithTag("timeline")
        timeline.performTouchInput{click(Offset(230*density,171*density))}
        compose.waitUntil(10000){vm.selected==2L}
        assertFalse(vm.panelOpen)
        timeline.performTouchInput{down(Offset(230*density,171*density));moveBy(Offset(-60*density,0f),100);up()}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==40.0}
        assertEquals(40.0,vm.frame,.001);assertFalse(vm.panelOpen)
        photo("selected-timeline")
        timeline.performTouchInput{advanceEventTime(500);doubleClick(Offset(200*density,171*density))}
        compose.waitUntil(10000){vm.panelOpen}
        assertEquals(2L,vm.selected)
    }

    @Test fun numericScrubbingIsGroupedAndPreservesOtherComponents() {
        compose.runOnIdle{vm.select(2)}
        compose.waitForIdle()
        val before=JSONArray(position().toString())
        compose.onNodeWithTag("value-X").performTouchInput {
            down(center);moveBy(Offset(35*density,0f),100);moveBy(Offset(15*density,0f),100);up()
        }
        compose.waitUntil(10000){vm.state.canUndo&&position().getDouble(0)>before.getDouble(0)+10}
        assertEquals(before.getDouble(1),position().getDouble(1),.001)
        assertEquals(before.getDouble(2),position().getDouble(2),.001)
        photo("transform-inspector")
        compose.runOnIdle{vm.undo()}
        compose.waitUntil(10000){position().toString()==before.toString()}
        assertNull(vm.state.error)
    }

    @Test fun transformPadAndLinkedScaleKeepDepthAndAspectRatio() {
        compose.runOnIdle{vm.select(2)}
        compose.waitForIdle()
        val original=JSONArray(position().toString())
        compose.onNodeWithTag("transform-pad").performTouchInput {
            down(center);moveBy(Offset(30*density,20*density),100);up()
        }
        compose.waitUntil(10000){position().getDouble(0)>original.getDouble(0)+5}
        assertTrue(position().getDouble(1)>original.getDouble(1))
        assertEquals(original.getDouble(2),position().getDouble(2),.001)
        compose.runOnIdle{vm.openProperty("scale");vm.setValue(JSONArray(listOf(170,65,100)))}
        compose.waitUntil(10000){vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONArray("value").getDouble(0)==170.0}
        compose.onNodeWithTag("value-X").performTouchInput{down(center);moveBy(Offset(35*density,0f),150);up()}
        compose.waitUntil(10000){vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONArray("value").getDouble(0)>175}
        val scale=vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONArray("value")
        assertEquals(65.0/170,scale.getDouble(1)/scale.getDouble(0),.0001)
        assertEquals(100.0,scale.getDouble(2),.001)
    }

    @Test fun draggingAKeyMovesItDirectlyAndUndoRestoresItsValue() {
        compose.runOnIdle{vm.select(2,false);vm.addKey()}
        compose.waitUntil(10000){scopeKeys().length()==1}
        seek(30.0)
        compose.runOnIdle{vm.setValue(JSONArray(listOf(740,960,0)))}
        compose.waitUntil(10000){scopeKeys().length()==2}
        val key=scopeKeys().getJSONObject(1).toString()
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(width/2f,9*density));moveBy(Offset(45*density,0f),150);up()
        }
        compose.waitUntil(10000){scopeKeys().getJSONObject(1).getInt("frame")==60}
        assertEquals(740.0,scopeKeys().getJSONObject(1).getJSONArray("value").getDouble(0),.001)
        assertFalse(vm.panelOpen)
        compose.runOnIdle{vm.undo()}
        compose.waitUntil(10000){scopeKeys().getJSONObject(1).toString()==key}
    }

    @Test fun keyToggleAndEasingEditTheCurrentPropertyAndOutgoingSegment() {
        compose.runOnIdle{vm.select(2)}
        compose.waitForIdle()
        compose.onNodeWithTag("property-key").performTouchInput{click()}
        compose.waitUntil(10000){scopeKeys().length()==1}
        compose.onNodeWithTag("property-key").performTouchInput{click()}
        compose.waitUntil(10000){scopeKeys().length()==0}
        compose.onNodeWithTag("property-key").performTouchInput{click()}
        compose.waitUntil(10000){scopeKeys().length()==1}
        seek(60.0)
        compose.runOnIdle{vm.setValue(JSONArray(listOf(740,960,0)))}
        compose.waitUntil(10000){scopeKeys().length()==2}
        seek(50.0)
        val rotation=vm.layer(2)!!.getJSONObject("transform").getJSONObject("rotation").toString()
        compose.onNodeWithContentDescription("缓动曲线").performTouchInput{click()}
        compose.onNodeWithTag("easing-graph").assertIsDisplayed()
        compose.onNodeWithText("缓入").performTouchInput{click()}
        compose.waitUntil(10000){scopeKeys().getJSONObject(0).getString("ease")=="in"}
        assertEquals("linear",scopeKeys().getJSONObject(1).getString("ease"))
        assertEquals(rotation,vm.layer(2)!!.getJSONObject("transform").getJSONObject("rotation").toString())
        photo("easing-inspector")
    }

    @Test fun previewSelectionIsIndependentAndLockedLayersCannotBeDragged() {
        val preview=compose.onNodeWithTag("preview-gesture")
        val size=preview.fetchSemanticsNode().size
        val polygon=previewPolygons(vm,size.width.toFloat(),size.height.toFloat()).first{it.first==2L}.second
        val point=polygon.reduce{a,b->a+b}/polygon.size.toFloat()
        preview.performTouchInput{click(point)}
        compose.waitUntil(10000){vm.selected!=0L}
        assertFalse(vm.panelOpen)
        compose.runOnIdle{vm.select(2);vm.flags(2,true,true)}
        compose.waitUntil(10000){vm.layer(2)!!.getBoolean("locked")}
        val before=vm.layer(2)!!.toString()
        compose.onNodeWithTag("value-X").performTouchInput{down(center);moveBy(Offset(40*density,0f),100);up()}
        compose.waitForIdle()
        assertEquals(before,vm.layer(2)!!.toString());assertNull(vm.state.error)
    }
    @Test fun previewCornerResizesWithoutMovingOrChangingDepth() {
        compose.runOnIdle{vm.select(2,false)}
        compose.waitForIdle()
        val preview=compose.onNodeWithTag("preview-gesture")
        val size=preview.fetchSemanticsNode().size
        val point=previewPolygons(vm,size.width.toFloat(),size.height.toFloat()).first{it.first==2L}.second[2]
        val before=position().toString()
        preview.performTouchInput{down(point);moveBy(Offset(24*density,24*density),150);up()}
        compose.waitUntil(10000){vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONArray("value").getDouble(0)>110}
        val scale=vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONArray("value")
        assertEquals(scale.getDouble(0),scale.getDouble(1),.001)
        assertEquals(100.0,scale.getDouble(2),.001);assertEquals(before,position().toString())
        compose.runOnIdle{vm.undo()}
        compose.waitUntil(10000){vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONArray("value").getDouble(0)==100.0}
    }
    @Test fun preciseScaleInputHonorsTheAspectLock() {
        compose.runOnIdle{vm.select(2);vm.openProperty("scale");vm.setValue(JSONArray(listOf(170,65,100)))}
        compose.waitUntil(10000){vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONArray("value").getDouble(0)==170.0}
        compose.onNodeWithTag("value-X").performTouchInput{click()}
        compose.onNode(hasSetTextAction()).performTextReplacement("200")
        compose.onNodeWithText("确定").performClick()
        compose.waitUntil(10000){vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONArray("value").getDouble(0)==200.0}
        val scale=vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONArray("value")
        assertEquals(65.0/170,scale.getDouble(1)/scale.getDouble(0),.0001)
        assertEquals(100.0,scale.getDouble(2),.001)
    }
}
