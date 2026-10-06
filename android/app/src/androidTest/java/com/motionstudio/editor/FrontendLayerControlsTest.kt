package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import org.junit.rules.TestName
import java.io.File
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class FrontendLayerControlsTest {
    @get:Rule val compose=createEmptyComposeRule()
    @get:Rule val testName=TestName()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    private var density=1f
    private val rowHeight get()=timelineRowHeightDp(InstrumentationRegistry.getInstrumentation().targetContext.resources.configuration.fontScale)*density
    private val firstRowMiddle get()=44*density+rowHeight/2
    private val secondRowMiddle get()=44*density+rowHeight*1.5f
    @Before fun setup() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        density=app.resources.displayMetrics.density
        root=File(app.filesDir,"acceptance/layer-ui-"+UUID.randomUUID()).apply{mkdirs()}
        File(root,"project.json").writeText((if(testName.methodName.startsWith("previewSelects"))crossingFrontendFixture()else LayerClipTimelineTest().fixture()).toString())
        scenario=ActivityScenario.launch(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null}
        scenario.onActivity{vm.select(2,false);vm.seek(40.0);vm.timelineScale=3f}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==40.0}
        compose.waitForIdle()
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun rotation()=vm.layer(2)!!.getJSONObject("transform").getJSONObject("rotation")
    private fun axis(name:String)=rotation().getJSONObject("axes").getJSONObject(name)
    private fun clip()=vm.timelineLayer(2)!!
    private fun separate() {
        scenario.onActivity{vm.openProperty("rotation")}
        compose.onNodeWithContentDescription("图层操作").performClick()
        compose.onNodeWithTag("separate-dimensions").performClick()
        compose.waitUntil(10000){vm.isSeparated()&&vm.state.saved}
    }
    private fun choose(prefix:String,name:String) {
        compose.onNodeWithTag(prefix+"-menu").performClick()
        compose.onNodeWithTag(prefix+"-"+name).performClick()
    }
    private fun openIndependentPad(key:String) {
        scenario.onActivity{vm.openProperty(key)}
        compose.onNodeWithContentDescription("图层操作").performClick()
        compose.onNodeWithTag("separate-dimensions").performClick()
        compose.waitUntil(10000){vm.isSeparated()&&vm.state.saved}
        val body=compose.onNodeWithTag("property-values").fetchSemanticsNode().config
        if(body.contains(androidx.compose.ui.semantics.SemanticsActions.ScrollBy))compose.onNodeWithTag("transform-pad").performScrollTo()
    }
    private fun photo(name:String) {
        compose.waitForIdle();InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        android.os.SystemClock.sleep(250)
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"$name.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
    }
    private fun undo(before:String) {
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.state.project!!.toString()==before&&vm.state.saved}
    }

    @Test fun independentAxisEditsAndCurvesPreserveOtherAxesAndUndoTogether() {
        separate();choose("rotation-axis","X")
        val before=vm.state.project!!.toString();val y=axis("y").toString();val z=axis("z").toString()
        compose.onNodeWithTag("rotation-ruler").performTouchInput {
            down(center);moveBy(Offset(40*density,0f),100);moveBy(Offset(20*density,0f),100);up()
        }
        compose.waitUntil(10000){axis("x").getJSONArray("keys").length()==3&&vm.state.saved}
        assertEquals(y,axis("y").toString());assertEquals(z,axis("z").toString())
        assertEquals(20,vm.keys().first().getInt("frame"));assertEquals(0,vm.keys().first().getInt("local_frame"))
        photo("separated-X");undo(before)
        compose.onNodeWithTag("open-curves").performClick()
        compose.onNodeWithTag("curve-kind-cubic").performScrollTo().performClick()
        compose.waitUntil(10000){axis("x").getJSONArray("keys").getJSONObject(0).getJSONObject("curve").getJSONObject("shape").getString("kind")=="cubic"&&vm.state.saved}
        assertEquals(y,axis("y").toString());assertEquals(z,axis("z").toString())
        compose.onNodeWithContentDescription("复制曲线").performClick()
        choose("curve-axis","Y")
        compose.onNodeWithContentDescription("粘贴曲线").performClick()
        compose.waitUntil(10000){axis("y").getJSONArray("keys").getJSONObject(0).getJSONObject("curve").getJSONObject("shape").getString("kind")=="cubic"}
        assertEquals(z,axis("z").toString());photo("separated-Y-curve")
        assertNull(vm.state.error)
    }

    @Test fun longPressMovesClipAndEdgeTrimKeepsLocalKeysWithSingleUndoAndCancellation() {
        val before=vm.state.project!!.toString();val tracks=vm.layer(2)!!.getJSONObject("transform").toString()
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x+10*density,firstRowMiddle));advanceEventTime(650)
            moveBy(Offset(30*density,0f),100);up()
        }
        compose.waitUntil(10000){clip().getInt("in_frame")==30&&vm.state.saved}
        assertEquals(110,clip().getInt("out_frame"));assertEquals(30,clip().getInt("offset_frame"))
        assertEquals(tracks,vm.layer(2)!!.getJSONObject("transform").toString());photo("clip-moved");undo(before)
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x-60*density,firstRowMiddle));moveBy(Offset(30*density,0f),100);up()
        }
        compose.waitUntil(10000){clip().getInt("in_frame")==30&&vm.state.saved}
        assertEquals(100,clip().getInt("out_frame"));assertEquals(20,clip().getInt("offset_frame"))
        assertEquals(tracks,vm.layer(2)!!.getJSONObject("transform").toString());photo("clip-trimmed");undo(before)
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x-60*density,firstRowMiddle));moveBy(Offset(36*density,0f),100);cancel()
        }
        compose.waitUntil(10000){vm.state.project!!.toString()==before}
        assertNull(vm.state.error)
    }

    @Test fun separatedKeyDragUsesCompositionTimeAndOnlyCapturedAxis() {
        separate();choose("rotation-axis","X")
        scenario.onActivity{vm.addKey();vm.panelOpen=false}
        compose.waitUntil(10000){vm.keys().size==3&&vm.state.saved}
        val before=vm.state.project!!.toString();val y=axis("y").toString();val z=axis("z").toString()
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x,9*density));moveBy(Offset(60*density,0f),100);up()
        }
        compose.waitUntil(10000){vm.keys().any{it.getInt("frame")==60}&&vm.state.saved}
        assertEquals(40,vm.keys().first{it.getInt("frame")==60}.getInt("local_frame"))
        assertEquals(y,axis("y").toString());assertEquals(z,axis("z").toString());undo(before)
    }

    @Test fun cancelledSeparatedNumericDragRestoresAllTracks() {
        separate();choose("rotation-axis","X")
        val before=vm.state.project!!.toString()
        compose.onNodeWithTag("value-X").performTouchInput {
            down(center);moveBy(Offset(50*density,0f),100);cancel()
        }
        compose.waitUntil(10000){vm.state.project!!.toString()==before}
        assertNull(vm.state.error)
    }

    @Test fun separatedCameraPositionAndTargetPadEditOnlyTheDisplayedAxis() {
        scenario.onActivity{vm.addCamera()}
        compose.waitUntil(10000){vm.hasCamera()&&vm.state.saved}
        for(key in listOf("position","target")) {
            openIndependentPad(key)
            compose.onNodeWithTag("pad-z-toggle").assertDoesNotExist()
            for((index,name) in listOf("X","Y","Z").withIndex()) {
                choose("property-axis",name)
                compose.onNodeWithText(if(index==0)"左右滑动调整 X"else"上下滑动调整 "+name).assertIsDisplayed()
                val before=vm.state.project!!.toString()
                val axes=vm.state.project!!.getJSONObject("camera").getJSONObject(key).getJSONObject("axes")
                val tracks=listOf("x","y","z").map{axes.getJSONObject(it).toString()}
                compose.onNodeWithTag("transform-pad").performTouchInput {
                    down(center);moveBy(if(index==0)Offset(24*density,0f)else Offset(0f,24*density),100);up()
                }
                compose.waitUntil(10000){vm.state.project!!.getJSONObject("camera").getJSONObject(key).getJSONObject("axes").getJSONObject(name.lowercase()).toString()!=tracks[index]&&vm.state.saved}
                val changed=vm.state.project!!.getJSONObject("camera").getJSONObject(key).getJSONObject("axes")
                for(other in 0..2)if(other!=index)assertEquals(tracks[other],changed.getJSONObject(listOf("x","y","z")[other]).toString())
                if(index==2)photo("camera-"+key+"-Z-pad")
                undo(before)
            }
            val before=vm.state.project!!.toString()
            compose.onNodeWithTag("transform-pad").performTouchInput {
                down(center);moveBy(Offset(0f,-24*density),100);cancel()
            }
            compose.waitUntil(10000){vm.state.project!!.toString()==before}
        }
        assertNull(vm.state.error)
    }

    @Test fun separatedScalePadUsesTheChosenAxisAndUnlinksByDefault() {
        openIndependentPad("scale");assertFalse(vm.scaleLinked)
        choose("property-axis","Z")
        compose.onNodeWithText("左右滑动缩放 Z").assertIsDisplayed()
        val before=vm.state.project!!.toString()
        val axes=vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONObject("axes")
        val x=axes.getJSONObject("x").toString();val y=axes.getJSONObject("y").toString();val z=axes.getJSONObject("z").toString()
        compose.onNodeWithTag("transform-pad").performTouchInput {
            down(center);moveBy(Offset(32*density,0f),100);up()
        }
        compose.waitUntil(10000){vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONObject("axes").getJSONObject("z").toString()!=z&&vm.state.saved}
        val changed=vm.layer(2)!!.getJSONObject("transform").getJSONObject("scale").getJSONObject("axes")
        assertEquals(x,changed.getJSONObject("x").toString());assertEquals(y,changed.getJSONObject("y").toString())
        photo("separated-scale-Z");undo(before)
        choose("property-axis","X");scenario.onActivity{vm.scaleLinked=true}
        compose.onNodeWithText("左右滑动缩放 X · XY 联动").assertIsDisplayed()
        assertNull(vm.state.error)
    }

    @Test fun preciseTrimValidatesRangeAndLockedTimelineRejectsClipMotion() {
        val before=vm.state.project!!.toString();val tracks=vm.layer(2)!!.getJSONObject("transform").toString()
        compose.onNodeWithTag("timeline").performTouchInput{longClick(Offset(center.x,firstRowMiddle))}
        compose.onNodeWithText("精确裁剪片段").performClick()
        compose.onNodeWithTag("clip-in").performTextReplacement("-1")
        compose.onNodeWithText("应用裁剪").assertIsNotEnabled()
        compose.onNodeWithTag("clip-in").performTextReplacement("25")
        compose.onNodeWithTag("clip-out").performTextReplacement("90")
        compose.onNodeWithText("应用裁剪").performClick()
        compose.waitUntil(10000){clip().getInt("in_frame")==25&&clip().getInt("out_frame")==90&&vm.state.saved}
        assertEquals(20,clip().getInt("offset_frame"));assertEquals(tracks,vm.layer(2)!!.getJSONObject("transform").toString())
        undo(before);scenario.onActivity{vm.flags(2,true,true)}
        compose.waitUntil(10000){!vm.editable()&&vm.state.saved}
        val locked=vm.state.project!!.toString()
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x,firstRowMiddle));advanceEventTime(650);moveBy(Offset(30*density,0f),100);up()
        }
        compose.waitForIdle();assertEquals(locked,vm.state.project!!.toString());assertNull(vm.state.error)
    }

    @Test fun layerReorderWithoutCameraAndSplitSelectsNewRightClip() {
        scenario.onActivity{vm.addRectangle();vm.panelOpen=false}
        compose.waitUntil(10000){vm.state.project!!.getJSONArray("layers").length()==2&&vm.state.saved}
        assertFalse(vm.threeD(3))
        val before=vm.state.project!!.toString()
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x,secondRowMiddle));advanceEventTime(650);moveBy(Offset(0f,-rowHeight),100);up()
        }
        compose.waitUntil(10000){vm.state.project!!.getJSONArray("layers").getJSONObject(1).getLong("id")==2L&&vm.state.saved}
        undo(before);scenario.onActivity{vm.select(2,false);vm.seek(60.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==60.0}
        compose.onNodeWithTag("timeline").performTouchInput{longClick(Offset(center.x,secondRowMiddle))}
        compose.onNodeWithText("在当前帧分割").performClick()
        compose.waitUntil(10000){vm.state.project!!.getJSONArray("layers").length()==3&&vm.state.saved}
        assertEquals(4L,vm.selected);assertEquals(60,vm.timelineLayer(4)!!.getInt("in_frame"))
        assertEquals(20,vm.timelineLayer(4)!!.getInt("offset_frame"));photo("clip-split")
        undo(before)
        assertNull(vm.state.error)
    }

    @Test fun modeSwitchPreservesInactiveTracksAndLockedControlsStayReadOnly() {
        scenario.onActivity{vm.openProperty("rotation")}
        val tracks=vm.layer(2)!!.getJSONObject("transform").toString()
        compose.onNodeWithTag("layer-3d-toggle").performClick()
        compose.waitUntil(10000){!vm.threeD()&&vm.state.saved}
        compose.onNodeWithTag("value-X").assertDoesNotExist();compose.onNodeWithTag("value-Y").assertDoesNotExist()
        compose.onNodeWithTag("value-Z").assertIsDisplayed();assertEquals(2,vm.rotationAxis)
        assertEquals(tracks,vm.layer(2)!!.getJSONObject("transform").toString());photo("flat-rotation")
        compose.onNodeWithTag("layer-3d-toggle").performClick()
        compose.waitUntil(10000){vm.threeD()&&vm.state.saved}
        compose.onNodeWithTag("value-X").assertIsDisplayed()
        assertEquals(tracks,vm.layer(2)!!.getJSONObject("transform").toString())
        scenario.onActivity{vm.flags(2,true,true)}
        compose.waitUntil(10000){!vm.editable()}
        compose.onNodeWithTag("layer-3d-toggle").assertIsNotEnabled()
        compose.onNodeWithContentDescription("图层操作").performClick()
        compose.onNodeWithTag("separate-dimensions").assertIsNotEnabled()
        assertNull(vm.state.error)
    }

    @Test fun previewSelectsActualFrontPlaneOnEachSideOfIntersection() {
        val preview=compose.onNodeWithTag("preview-gesture")
        val size=preview.fetchSemanticsNode().size
        val fit=kotlin.math.min(size.width/256f,size.height/256f)
        preview.performTouchInput{click(Offset(center.x-32*fit,center.y))}
        compose.waitUntil(10000){vm.selected==2L}
        preview.performTouchInput{click(Offset(center.x+32*fit,center.y))}
        compose.waitUntil(10000){vm.selected==1L}
        photo("crossing-plane-selected");assertNull(vm.state.error)
    }

    @Test fun axisControlsRemainUsableWithNarrowWindowsAndLargeFonts() {
        separate();choose("rotation-axis","X")
        compose.onNodeWithContentDescription("关闭属性面板").assertIsDisplayed()
        compose.onNodeWithTag("layer-3d-toggle").assertIsDisplayed()
        val bounds=compose.onNodeWithTag("rotation-axis-menu").fetchSemanticsNode().boundsInRoot
        assertTrue(bounds.height>=48*density-.5f)
        photo("axis-layout")
        compose.onNodeWithTag("open-curves").performClick()
        choose("curve-axis","Y");assertEquals(1,vm.rotationAxis)
        photo("curve-layout");compose.onNodeWithTag("easing-graph").assertIsDisplayed()
        assertTrue("Curve handles need vertical room",compose.onNodeWithTag("easing-graph").fetchSemanticsNode().boundsInRoot.height>=64*density)
        scenario.onActivity{activity->
            val config=activity.resources.configuration;val metrics=activity.resources.displayMetrics
            File(root,"layout-profile-report.json").writeText(JSONObject().put("widthPixels",metrics.widthPixels)
                .put("heightPixels",metrics.heightPixels).put("densityDpi",metrics.densityDpi)
                .put("screenWidthDp",config.screenWidthDp).put("screenHeightDp",config.screenHeightDp)
                .put("fontScale",config.fontScale).put("axis",vm.rotationAxis).toString(2))
        }
    }
}
