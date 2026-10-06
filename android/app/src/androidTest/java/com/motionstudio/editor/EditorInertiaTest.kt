package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.toPixelMap
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

class EditorInertiaTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    private var density=1f
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        density=context.resources.displayMetrics.density
        root=File(context.filesDir,"acceptance/inertia-${UUID.randomUUID()}").apply{mkdirs()}
        val project=LayerClipTimelineTest().fixture().put("frames",3000)
        val layer=project.getJSONArray("layers").getJSONObject(0)
        val layers=JSONArray()
        for(id in 2..21)layers.put(JSONObject(layer.toString()).put("id",id).put("name","图层 $id"))
        project.put("layers",layers)
        File(root,"project.json").writeText(project.toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(30000){vm.state.project!=null&&vm.catalogue!=null}
        scenario.onActivity{vm.select(21,false);vm.timelineScale=3f;vm.seek(1500.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==1500.0}
        compose.waitForIdle()
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun photo(name:String) {
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"$name.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
    }
    @Test fun timelineReleaseContinuesAndAnyNewTouchStopsIt() {
        compose.mainClock.autoAdvance=false
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x,28*density));moveBy(Offset(-30*density,0f),40);moveBy(Offset(-30*density,0f),40);up()
        }
        val released=vm.frame
        compose.mainClock.advanceTimeBy(80)
        assertTrue("playhead stopped at release",vm.frame>released)
        compose.onNodeWithContentDescription("合成设置").performTouchInput{down(center)}
        val stopped=vm.frame
        compose.mainClock.advanceTimeBy(500)
        assertEquals(stopped,vm.frame,0.0)
        compose.onNodeWithContentDescription("合成设置").performTouchInput{up()}
        photo("timeline-interrupted")
        assertNull(vm.state.error)
    }
    @Test fun timelineFlingClampsToCompositionEnd() {
        scenario.onActivity{vm.seek(2995.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==2995.0}
        compose.mainClock.autoAdvance=false
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x,28*density));moveBy(Offset(-50*density,0f),40);up()
        }
        compose.mainClock.advanceTimeBy(1500)
        assertEquals(2999.0,vm.frame,0.0)
        assertNull(vm.state.error)
    }
    @Test fun accessibleCategoryActivationStopsTimelineInertia() {
        compose.mainClock.autoAdvance=false
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x,28*density));moveBy(Offset(-30*density,0f),40);moveBy(Offset(-30*density,0f),40);up()
        }
        val released=vm.frame
        compose.mainClock.advanceTimeBy(80)
        assertTrue(vm.frame>released)
        compose.onNodeWithTag("footer-transform").performClick()
        val stopped=vm.frame
        compose.mainClock.advanceTimeBy(500)
        assertEquals(stopped,vm.frame,0.0);assertTrue(vm.panelOpen);assertNull(vm.state.error)
    }
    private fun timelinePixels():List<Int> {
        val pixels=compose.onNodeWithTag("timeline").captureToImage().toPixelMap()
        return buildList {for(y in (44*density).toInt() until pixels.height step 3)for(x in 0 until pixels.width step 5)add(pixels[x,y].hashCode())}
    }
    @Test fun verticalLayerBrowsingContinuesAndStopsOnTouch() {
        compose.mainClock.autoAdvance=false
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(75*density,height-8*density));moveBy(Offset(0f,-25*density),40);moveBy(Offset(0f,-25*density),40);up()
        }
        val released=timelinePixels()
        compose.mainClock.advanceTimeBy(80)
        assertNotEquals(released,timelinePixels())
        compose.onNodeWithTag("timeline").performTouchInput{down(Offset(75*density,height/2f))}
        compose.mainClock.advanceTimeByFrame()
        val stopped=timelinePixels()
        compose.mainClock.advanceTimeBy(500)
        assertEquals(stopped,timelinePixels())
        compose.onNodeWithTag("timeline").performTouchInput{up()}
        assertEquals(1500.0,vm.frame,0.0)
        photo("timeline-vertical")
    }
    @Test fun parameterDragAndTailUndoAsOneOperation() {
        scenario.onActivity{vm.openEffects()}
        compose.onNodeWithTag("effects-add").performClick()
        compose.onNodeWithTag("effect-add-brightness_contrast").performScrollTo().performClick()
        compose.waitUntil(15000){vm.layer(21)?.optJSONArray("effects")?.length()==1&&vm.state.saved}
        val instance=vm.layer(21)!!.getJSONArray("effects").getJSONObject(0).getLong("id")
        compose.onNodeWithTag("effect-instance-$instance").onChildren()[0].performClick()
        compose.onNodeWithTag("effect-wheel-p0001-0").performScrollTo()
        val before=vm.state.project!!.toString()
        compose.mainClock.autoAdvance=false
        val wheel=compose.onNodeWithTag("effect-wheel-p0001-0")
        wheel.performTouchInput {
            down(center);moveBy(Offset(-15*density,0f),40);moveBy(Offset(-15*density,0f),40);up()
        }
        val released=wheel.fetchSemanticsNode().config[androidx.compose.ui.semantics.SemanticsProperties.ProgressBarRangeInfo].current
        compose.mainClock.advanceTimeBy(80)
        val continued=wheel.fetchSemanticsNode().config[androidx.compose.ui.semantics.SemanticsProperties.ProgressBarRangeInfo].current
        assertTrue("parameter stopped at release",continued>released)
        compose.mainClock.advanceTimeBy(2000)
        compose.mainClock.autoAdvance=true
        compose.waitUntil(15000){vm.state.saved&&vm.state.project!!.toString()!=before}
        photo("wheel-settled")
        scenario.onActivity{vm.undo()}
        compose.waitUntil(15000){vm.state.saved&&vm.state.project!!.toString()==before}
        assertNull(vm.state.error)
        File(root,"inertia-report.json").writeText(JSONObject().put("released",released).put("continued",continued).put("singleUndoRestored",true).toString(2))
    }
}
