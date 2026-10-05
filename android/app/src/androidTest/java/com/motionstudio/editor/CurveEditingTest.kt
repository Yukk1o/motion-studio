package com.motionstudio.editor

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class CurveEditingTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    private lateinit var clipboard:ClipboardManager
    private var oldClip:ClipData?=null
    @Before fun setup() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(app.filesDir,"acceptance/curves-"+UUID.randomUUID()).apply{mkdirs()}
        launch()
        scenario.onActivity{clipboard=it.getSystemService(ClipboardManager::class.java);oldClip=clipboard.primaryClip}
        val commands=JSONArray()
        fun track(objectId:Long,key:String,start:Int,end:Int,a:Any,b:Any) {
            commands.put(JSONObject().put("op","animate").put("object",objectId).put("property",key).put("frame",start).put("enabled",true))
            for((frame,value) in listOf(start to a,end to b))commands.put(JSONObject().put("op",if(value is JSONArray)"set_vector"else"set_scalar")
                .put("object",objectId).put("property",key).put("frame",frame).put("value",value))
        }
        track(2,"position",0,60,JSONArray(listOf(540,960,0)),JSONArray(listOf(800,960,0)))
        track(3,"opacity",10,90,.2,.8)
        track(0,"fov",0,120,30,60)
        scenario.onActivity{vm.editBatch(commands)}
        compose.waitUntil(10000){vm.layer(2)!!.getJSONObject("transform").getJSONObject("position").getJSONArray("keys").length()==2&&vm.state.saved}
    }
    private fun launch() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        scenario=ActivityScenario.launch(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null}
    }
    @After fun teardown(){if(::clipboard.isInitialized){if(oldClip!=null)clipboard.setPrimaryClip(oldClip!!)else clipboard.clearPrimaryClip()};if(::scenario.isInitialized)scenario.close()}
    private fun track(id:Long,key:String)=if(id==0L)vm.state.project!!.getJSONObject("camera").getJSONObject(key)
        else vm.layer(id)!!.getJSONObject("transform").getJSONObject(key)
    private fun curve(id:Long=2,key:String="position")=track(id,key).getJSONArray("keys").getJSONObject(0).optJSONObject("curve")
    private fun openGraph(id:Long,key:String,frame:Double) {
        scenario.onActivity{vm.select(id);vm.openProperty(key);vm.seek(frame)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==frame}
        compose.waitForIdle()
        compose.onNodeWithContentDescription("缓动曲线").performClick()
        compose.onNodeWithTag("easing-graph").assertIsDisplayed()
    }
    private fun applyQuadratic() {
        compose.onNodeWithTag("curve-kind-quadratic").performScrollTo().performClick()
        compose.waitUntil(10000){curve()?.optJSONObject("shape")?.optString("kind")=="quadratic"}
        compose.onNodeWithContentDescription("曲线参数").performScrollTo().performClick()
        compose.onNodeWithTag("curve-param-control.x").performTextReplacement("0.2")
        compose.onNodeWithTag("curve-param-control.y").performTextReplacement("0.8")
        compose.onNodeWithText("应用").performClick()
        compose.waitUntil(10000){curve()?.getJSONObject("shape")?.getJSONArray("control")?.getDouble(1)?.let{it>.79&&it<.81}==true}
    }
    private fun photo(name:String) {
        compose.waitForIdle();InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,name+".png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
    }
    private fun verifyExportParameters() {
        val originalFrame=vm.frame
        val frozen=NativeBridge.create(root.absolutePath,vm.state.project!!.toString())
        assertTrue(frozen>0)
        val buffer=ByteBuffer.allocateDirect(128*128).order(ByteOrder.nativeOrder())
        try {
            for(frame in listOf(0,21,60,119,179)) {
                scenario.onActivity{vm.seek(frame.toDouble())}
                compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==frame.toDouble()}
                val projected=vm.state.sample!!.getJSONArray("projectedLayers")
                assertEquals(projected.length(),NativeBridge.sampleInto(frozen,frame,buffer))
                for(i in 0 until projected.length()) {
                    val corners=projected.getJSONObject(i).getJSONArray("corners")
                    val base=i*128;val width=buffer.getFloat(base+20*4);val height=buffer.getFloat(base+21*4)
                    for((index,p) in listOf(-.5f to .5f,.5f to .5f,.5f to -.5f,-.5f to -.5f).withIndex()) {
                        val x=p.first*width;val y=p.second*height
                        fun component(row:Int)=buffer.getFloat(base+row*4)*x+buffer.getFloat(base+(4+row)*4)*y+buffer.getFloat(base+(12+row)*4)
                        val w=component(3);val px=(component(0)/w*.5+.5)*1080;val py=(.5-component(1)/w*.5)*1920
                        assertEquals(corners.getJSONArray(index).getDouble(0),px,.01)
                        assertEquals(corners.getJSONArray(index).getDouble(1),py,.01)
                    }
                }
            }
        } finally {
            NativeBridge.destroy(frozen)
            scenario.onActivity{vm.seek(originalFrame)}
            compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==originalFrame}
        }
    }

    @Test fun customCurveCopiesAcrossLayersAndCameraWithoutChangingValuesOrTimes() {
        openGraph(2,"position",21.0);applyQuadratic()
        val source=curve()!!.toString()
        val sampled=(vm.sampleValue() as JSONArray).getDouble(0)
        assertEquals(709.0,sampled,.02) // p=.65, 540 + 260*.65.
        val previewBounds=compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot
        val revision=vm.state.sample!!.getLong("revision")
        compose.onNodeWithTag("curve-view-velocity").performClick()
        compose.waitForIdle();assertEquals(source,curve()!!.toString());assertEquals(revision,vm.state.sample!!.getLong("revision"))
        assertEquals(previewBounds,compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot)
        photo("quadratic-speed")
        compose.onNodeWithContentDescription("复制曲线").performClick()
        for((id,key,time) in listOf(Triple(3L,"opacity",30.0),Triple(0L,"fov",40.0))) {
            compose.onNodeWithContentDescription("关闭属性面板").performClick();openGraph(id,key,time)
            val before=track(id,key).toString()
            compose.onNodeWithContentDescription("粘贴曲线").performClick()
            compose.waitUntil(10000){curve(id,key)?.toString()==source}
            val original=JSONObject(before).getJSONArray("keys");val after=track(id,key).getJSONArray("keys")
            for(i in 0 until original.length()) {
                assertEquals(original.getJSONObject(i).getInt("frame"),after.getJSONObject(i).getInt("frame"))
                assertEquals(original.getJSONObject(i).getDouble("value"),after.getJSONObject(i).getDouble("value"),.000001)
            }
            scenario.onActivity{vm.undo()};compose.waitUntil(10000){track(id,key).toString()==before}
            scenario.onActivity{vm.redo()};compose.waitUntil(10000){curve(id,key)?.toString()==source&&vm.state.saved}
        }
        val saved=vm.state.project!!.toString()
        verifyExportParameters()
        scenario.close();launch()
        assertEquals(saved,vm.state.project!!.toString());assertNull(vm.state.error)
        File(root,"copy-report.json").writeText(JSONObject().put("crossLayerScalarCamera",true).put("valuesAndTimesPreserved",true)
            .put("undoRedo",true).put("viewSwitchDoesNotEdit",true).put("exportParametersMatchPreview",true).put("reopen",true).toString(2))
    }

    @Test fun draggingHandlesAndEditingVelocityAndElasticUseTheCoreEvaluator() {
        openGraph(2,"position",21.0)
        compose.onNodeWithTag("curve-kind-quadratic").performScrollTo().performClick()
        compose.waitUntil(10000){curve()!=null};compose.waitForIdle()
        val before=curve()!!.toString()
        // Default progress control is (.5,0); graph range has 8% top/bottom margin.
        compose.onNodeWithTag("easing-graph").performTouchInput {
            down(Offset(width*.5f,height*(1.08f/1.16f)));moveBy(Offset(width*.08f,-height*.23f),150);up()
        }
        compose.waitUntil(10000){curve()!!.getJSONObject("shape").getJSONArray("control").getDouble(1)>.1&&vm.state.canUndo}
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){curve()!!.toString()==before}
        compose.onNodeWithTag("curve-view-velocity").performClick()
        compose.onNodeWithTag("curve-kind-quadratic").performScrollTo().performClick()
        compose.waitUntil(10000){curve()!!.getString("space")=="velocity"}
        val easing=vm.easingDefinition()!!
        val graph=JSONObject(NativeBridge.curveGraph(easing.toString())).getJSONObject("data").getJSONArray("points")
        assertEquals(.5,graph.getJSONObject(80).getDouble("progress"),.0001)
        assertEquals(1.5,graph.getJSONObject(80).getDouble("velocity"),.0001)
        val expected=540+260*(3*.35*.35-2*.35*.35*.35)
        compose.waitUntil(10000){kotlin.math.abs((vm.sampleValue() as JSONArray).getDouble(0)-expected)<.02}
        photo("velocity-definition")
        verifyExportParameters()
        compose.onNodeWithTag("curve-kind-elastic").performScrollTo().performClick()
        compose.waitUntil(10000){curve()!!.getJSONObject("shape").getString("kind")=="elastic"}
        compose.onNodeWithContentDescription("曲线参数").performScrollTo().performClick()
        compose.onNodeWithTag("curve-param-damping").performTextReplacement("8.0")
        compose.onNodeWithText("应用").performClick()
        compose.waitUntil(10000){curve()!!.getJSONObject("shape").getDouble("damping")==8.0}
        photo("elastic-definition");verifyExportParameters();assertNull(vm.state.error)
    }

    @Test fun elasticHandleEditsFrequencyAndDampingInBothViewsWithOneUndoPerDrag() {
        openGraph(2,"position",21.0)
        compose.onNodeWithTag("curve-kind-elastic").performScrollTo().performClick()
        compose.waitUntil(10000){curve()?.optJSONObject("shape")?.optString("kind")=="elastic"}
        val original=vm.easingDefinition()!!.toString()
        val originalKeys=track(2,"position").getJSONArray("keys").toString()
        fun dragHandle(view:String,dx:Float,dy:Float) {
            compose.waitForIdle()
            val definition=vm.easingDefinition()!!
            val graph=JSONObject(NativeBridge.curveGraph(definition.toString())).getJSONObject("data").getJSONArray("points")
            val oscillations=definition.getJSONObject("curve").getJSONObject("shape").getDouble("oscillations")
            val time=(if(view=="progress").5 else .25)/oscillations
            val index=time*(graph.length()-1);val left=kotlin.math.floor(index).toInt();val right=minOf(left+1,graph.length()-1)
            val y=graph.getJSONObject(left).getDouble(view)+(graph.getJSONObject(right).getDouble(view)-graph.getJSONObject(left).getDouble(view))*(index-left)
            val samples=(0 until graph.length()).map{graph.getJSONObject(it).getDouble(view)}+listOf(0.0,1.0)
            val low=samples.min();val high=samples.max();val rangeLow=low-.08*(high-low);val rangeHigh=high+.08*(high-low)
            compose.onNodeWithTag("easing-graph").performTouchInput {
                down(Offset((width*time).toFloat(),(height*(rangeHigh-y)/(rangeHigh-rangeLow)).toFloat()))
                moveBy(Offset(width*dx,height*dy),150);up()
            }
        }
        dragHandle("progress",-.04f,0f)
        compose.waitUntil(10000){curve()!!.getJSONObject("shape").getDouble("oscillations")>2.8&&vm.state.saved}
        assertEquals(6.0,curve()!!.getJSONObject("shape").getDouble("damping"),.00001)
        // Let the system clipboard notification finish before visual evidence.
        android.os.SystemClock.sleep(3500)
        photo("09-elastic-frequency-handle")
        verifyExportParameters()
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.easingDefinition()!!.toString()==original}
        dragHandle("progress",0f,.12f)
        compose.waitUntil(10000){curve()!!.getJSONObject("shape").getDouble("damping")>8&&vm.state.saved}
        assertEquals(2.5,curve()!!.getJSONObject("shape").getDouble("oscillations"),.00001)
        verifyExportParameters()
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.easingDefinition()!!.toString()==original}
        compose.onNodeWithTag("curve-view-velocity").performClick()
        assertEquals(original,vm.easingDefinition()!!.toString())
        dragHandle("velocity",-.025f,0f)
        compose.waitUntil(10000){curve()!!.getJSONObject("shape").getDouble("oscillations")>3&&vm.state.saved}
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.easingDefinition()!!.toString()==original}
        dragHandle("velocity",0f,.10f)
        compose.waitUntil(10000){curve()!!.getJSONObject("shape").getDouble("damping")>7.5&&vm.state.saved}
        photo("10-elastic-velocity-handles")
        verifyExportParameters()
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){track(2,"position").getJSONArray("keys").toString()==originalKeys}
        dragHandle("velocity",-.02f,.06f)
        compose.waitUntil(10000){curve()!!.getJSONObject("shape").getDouble("oscillations")>3&&curve()!!.getJSONObject("shape").getDouble("damping")>7&&vm.state.saved}
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){track(2,"position").getJSONArray("keys").toString()==originalKeys}
        assertNull(vm.state.error)
        File(root,"elastic-handle-report.json").writeText(JSONObject().put("progressAndVelocityHandles",true)
            .put("independentFrequencyAndDamping",true).put("singleUndoPerDrag",true).put("keyValuesAndTimesPreserved",true)
            .put("exportMatchesPreview",true).toString(2))
    }

    @Test fun draggingAPresetHandleConvertsOnlyItsTimingAndUndoRestoresThePreset() {
        openGraph(2,"position",21.0)
        val before=track(2,"position").toString()
        compose.onNodeWithTag("easing-graph").performTouchInput {
            down(Offset(width/3f,height*((1.08f-1f/3f)/1.16f)))
            moveBy(Offset(0f,-height*.12f),150);up()
        }
        compose.waitUntil(10000){curve()?.optJSONObject("shape")?.optString("kind")=="cubic"&&vm.state.saved}
        assertEquals(.0,track(2,"position").getJSONArray("keys").getJSONObject(0).getDouble("frame"),.00001)
        assertEquals(60,track(2,"position").getJSONArray("keys").getJSONObject(1).getInt("frame"))
        verifyExportParameters()
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){track(2,"position").toString()==before}
        assertNull(vm.state.error)
    }

    @Test fun insertingKeysDuringElasticOvershootKeepsPhysicalPropertiesEditable() {
        val easing=JSONObject().put("ease","linear").put("curve",JSONObject().put("space","progress")
            .put("shape",JSONObject().put("kind","elastic").put("oscillations",2.5).put("damping",6.0)))
        val commands=JSONArray()
        for(case in listOf(listOf(3,"opacity",10,90,.8,1.0),listOf(0,"fov",0,120,110,120))) {
            val id=case[0];val key=case[1];val start=case[2];val end=case[3];val a=case[4];val b=case[5]
            for((frame,value) in listOf(start to a,end to b))commands.put(JSONObject().put("op","set_scalar").put("object",id).put("property",key).put("frame",frame).put("value",value))
            commands.put(JSONObject().put("op","curve").put("object",id).put("property",key).put("frame",start).put("easing",easing))
        }
        scenario.onActivity{vm.editBatch(commands);vm.select(3);vm.openProperty("opacity");vm.seek(22.0)}
        compose.waitUntil(10000){curve(3,"opacity")!=null&&vm.state.sample?.optDouble("frame")==22.0}
        assertEquals(1.0,(vm.sampleValue() as Number).toDouble(),.00001)
        scenario.onActivity{vm.addKey()}
        compose.waitUntil(10000){track(3,"opacity").getJSONArray("keys").length()==3}
        assertEquals(1.0,track(3,"opacity").getJSONArray("keys").getJSONObject(1).getDouble("value"),.00001)
        scenario.onActivity{vm.undo();vm.select(0);vm.openProperty("fov")}
        compose.waitUntil(10000){track(3,"opacity").getJSONArray("keys").length()==2}
        assertEquals(120.0,(vm.sampleValue() as Number).toDouble(),.00001)
        scenario.onActivity{vm.addKey()}
        compose.waitUntil(10000){track(0,"fov").getJSONArray("keys").length()==3}
        assertEquals(120.0,track(0,"fov").getJSONArray("keys").getJSONObject(1).getDouble("value"),.00001)
        assertNull(vm.state.error)
    }
}
