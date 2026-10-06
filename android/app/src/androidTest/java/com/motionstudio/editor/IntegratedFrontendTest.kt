package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.PixelCopy
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import androidx.compose.ui.graphics.toPixelMap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.math.roundToInt

@RunWith(AndroidJUnit4::class)
class IntegratedFrontendTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var activity:AcceptanceActivity
    private lateinit var root:File
    private val context get()=InstrumentationRegistry.getInstrumentation().targetContext
    @Before fun setup() {
        root=File(context.filesDir,"acceptance/integrated-ui-${UUID.randomUUID()}").apply{mkdirs()}
        val p=nativeData(NativeBridge.projectTemplate(0)).put("width",256).put("height",144).put("frames",60).put("layers",JSONArray())
        p.getJSONObject("camera").put("created",false)
        File(root,"project.json").writeText(p.toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{activity=it;vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(30000){vm.state.project!=null&&vm.catalogue!=null}
    }
    @After fun teardown(){
        if(::vm.isInitialized){
            File(root,"final-state.json").writeText(JSONObject().put("error",vm.state.error).put("property",vm.property).put("sample",vm.state.sample).toString())
            runCatching{File(root,"semantics.txt").writeText(compose.onRoot(useUnmergedTree=true).printToString())}
            runCatching{val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot();File(root,"final-screen.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()}
        }
        if(::scenario.isInitialized)scenario.close()
    }
    private fun photo(name:String) {
        compose.waitUntil(15000) {
            scenario.onActivity{vm.refreshDiagnostics()}
            val data=vm.state.sample
            data!=null&&!data.isNull("graphics")&&!data.isNull("lastPresentedFrame")&&data.getDouble("lastPresentedFrame")==vm.frame&&
                data.getLong("lastPresentedRevision")==data.getLong("revision")&&data.getLong("lastPresentedViewRevision")==data.getLong("viewRevision")
        }
        assertNull(vm.state.error)
        compose.waitForIdle()
        val instrumentation=InstrumentationRegistry.getInstrumentation()
        val file=File(root,"$name.png")
        instrumentation.uiAutomation.takeScreenshot().also{bitmap->file.outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()}
    }
    private fun solid() {
        scenario.onActivity{vm.addRectangle()}
        compose.waitUntil(10000){vm.layer(vm.selected)!=null&&vm.state.saved}
    }
    private fun effects() {
        scenario.onActivity{vm.openEffects()}
        compose.onNodeWithTag("effects-add").performClick()
    }
    private fun addEffect(id:String) {
        compose.onNodeWithTag("effect-add-$id").performScrollTo().performClick()
        compose.waitUntil(15000){vm.layer(vm.selected)?.optJSONArray("effects")?.length()==1&&vm.state.saved}
    }
    private fun enterEffect() {
        val instance=vm.layer(vm.selected)!!.getJSONArray("effects").getJSONObject(0).getLong("id")
        compose.onNodeWithTag("effect-instance-$instance").onChildren()[0].performClick()
    }
    private fun assertTimelineVisible() {
        val preview=compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot
        val timeline=compose.onNodeWithTag("timeline").fetchSemanticsNode().boundsInRoot
        val panel=compose.onNodeWithTag("effects-panel").fetchSemanticsNode().boundsInRoot
        val transport=compose.onNodeWithTag("transport").fetchSemanticsNode().boundsInRoot
        val density=context.resources.displayMetrics.density
        assertTrue("timeline does not show a complete layer",timeline.height/density>=83)
        assertTrue("preview is too small",preview.height/density>=48&&preview.width/density>=120)
        assertFalse("effects cover timeline",panel.overlaps(timeline))
        assertFalse("effects cover preview",panel.overlaps(preview))
        assertFalse("effects cover playback controls",panel.overlaps(transport))
        assertFalse("preview covers timeline",preview.overlaps(timeline))
        compose.onNodeWithTag("effect-time").assertDoesNotExist()
    }
    private fun assertWheelPainted(tag:String) {
        val pixels=compose.onNodeWithTag(tag).captureToImage().toPixelMap()
        assertTrue("wheel has no drawing space",pixels.width>=20&&pixels.height>=20)
        val centre=pixels.width/2
        val marker=(centre-2..centre+2).sumOf{x->(pixels.height/4 until pixels.height*3/4).count{y->
            val color=pixels[x,y]
            color.green>.5f&&color.blue>.4f&&color.red<.5f
        }}
        assertTrue("wheel centre marker is invisible",marker>=10)
    }
    private fun longPressKey(frame:Int) {
        val offset=(frame-vm.frame).toFloat()*vm.timelineScale*context.resources.displayMetrics.density
        val y=9*context.resources.displayMetrics.density
        compose.onNodeWithTag("timeline").performTouchInput{longClick(androidx.compose.ui.geometry.Offset(centerX+offset,y))}
    }
    private fun verifyLayoutAdjustment(recreate:Boolean=false,prefix:String="layout-profile") {
        fun assertGripsHidden() {
            compose.onNodeWithTag("layout-resize-side").assertDoesNotExist()
            compose.onNodeWithTag("layout-resize-height").assertDoesNotExist()
            compose.onNodeWithTag("finish-layout").assertDoesNotExist()
        }
        fun enterLayoutMode() {
            compose.onNodeWithContentDescription("合成设置").performClick()
            compose.onNodeWithTag("adjust-layout").performScrollTo().performClick()
            compose.onNodeWithTag("finish-layout").assertIsDisplayed()
        }
        assertGripsHidden();photo("$prefix-before-resize")
        enterLayoutMode();photo("$prefix-editing")
        val side=compose.onAllNodesWithTag("layout-resize-side").fetchSemanticsNodes().isNotEmpty()
        val tag=if(side)"layout-resize-side"else"layout-resize-height"
        val density=context.resources.displayMetrics.density
        fun extent():Float=compose.onNodeWithTag("effects-panel").fetchSemanticsNode().boundsInRoot.let{if(side)it.width else it.height}
        fun drag(amount:Float,cancelled:Boolean=false) {
            compose.onNodeWithTag(tag).performTouchInput {
                down(center);moveBy(if(side)androidx.compose.ui.geometry.Offset(amount,0f)else androidx.compose.ui.geometry.Offset(0f,amount),200)
                if(cancelled)cancel()else up()
            }
        }
        val original=extent();val project=vm.state.project!!.toString();val property=vm.property
        val target=compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
        assertTrue("small resize target",target.width/density>=47.5f&&target.height/density>=47.5f)
        drag(-48*density,true)
        compose.waitUntil(10000){kotlin.math.abs(extent()-original)<2}
        drag(64*density)
        compose.waitUntil(10000){original-extent()>16*density}
        val adjusted=extent()
        assertTimelineVisible();assertEquals(project,vm.state.project!!.toString())
        assertTrue(vm.layoutPreferences.all.isNotEmpty())
        photo("$prefix-resized")
        compose.onNodeWithTag("finish-layout").performClick()
        assertGripsHidden();assertEquals(adjusted,extent(),2f);photo("$prefix-finished")
        if(recreate) {
            scenario.recreate()
            scenario.onActivity{activity=it;vm=ViewModelProvider(it)[EditorViewModel::class.java]}
            compose.waitUntil(15000){compose.onAllNodesWithTag("effects-panel").fetchSemanticsNodes().isNotEmpty()}
            assertGripsHidden();assertEquals(adjusted,extent(),2f);photo("$prefix-reopened")
        }
        enterLayoutMode()
        drag(-400*density)
        assertTimelineVisible();photo("$prefix-expanded")
        drag(400*density)
        assertTimelineVisible()
        assertEquals(project,vm.state.project!!.toString())
        compose.onNodeWithTag("finish-layout").performClick()
        assertGripsHidden()
        compose.onNodeWithContentDescription("合成设置").performClick()
        compose.onNodeWithTag("reset-layout").performScrollTo().performClick()
        compose.waitUntil(10000){kotlin.math.abs(extent()-original)<2}
        assertTrue(vm.layoutPreferences.all.isEmpty())
        enterLayoutMode();UiDevice.getInstance(InstrumentationRegistry.getInstrumentation()).pressBack()
        compose.waitUntil(10000){compose.onAllNodesWithTag("finish-layout").fetchSemanticsNodes().isEmpty()}
        assertGripsHidden();assertTimelineVisible();assertEquals(original,extent(),2f);assertEquals(property,vm.property)
        File(root,"$prefix-adjustment-report.json").writeText(JSONObject().put("sidePanel",side).put("originalExtentDp",original/density)
            .put("adjustedExtentDp",adjusted/density).put("cancelRestored",true).put("recreationChecked",recreate)
            .put("projectUnchanged",true).put("resetRestored",true).put("gripsOnlyInLayoutMode",true)
            .put("finishPreserved",true).put("backExitsLayoutMode",true).toString(2))
    }
    @Test fun layoutRatioDraggingCancelsPersistsAcrossRecreationAndResets() {
        solid();effects();addEffect("brightness_contrast");enterEffect()
        verifyLayoutAdjustment(true,"layout-custom")
    }
    private fun multipleLayers() {
        solid()
        scenario.onActivity {
            val original=vm.layer(vm.selected)!!
            val commands=JSONArray()
            for(id in 2..14)commands.put(JSONObject().put("op","add").put("layer",JSONObject(original.toString()).put("id",id).put("name","图层 $id")))
            vm.editBatch(commands);vm.select(14,false);vm.panelOpen=false;vm.timelineScale=3f;vm.seek(15.0)
        }
        compose.waitUntil(10000){vm.state.project!!.getJSONArray("layers").length()==14&&vm.state.saved&&vm.state.sample?.optDouble("frame")==15.0}
        scenario.onActivity{vm.addKey()}
        compose.waitUntil(10000){vm.keys().size==1&&vm.state.saved}
    }
    private fun assertCompactLayerGap() {
        val density=context.resources.displayMetrics.density
        val pixels=compose.onNodeWithTag("timeline").captureToImage().toPixelMap()
        val x=(pixels.width/2+16*density).roundToInt().coerceAtMost(pixels.width-1)
        val spans=mutableListOf<IntRange>();var start:Int?=null
        for(y in (44*density).roundToInt() until minOf(pixels.height,(154*density).roundToInt())) {
            if(pixels[x,y].blue>.21f){if(start==null)start=y}
            else if(start!=null){if(y-start>=20*density)spans+=start until y;start=null}
        }
        assertTrue("two complete clips must be visible",spans.size>=2)
        val gap=(spans[1].first-spans[0].last-1)/density
        assertTrue("layer gap is $gap dp",gap in 2f..6f)
        File(root,"timeline-density-report.json").writeText(JSONObject().put("observedClipGapDp",gap).put("layerCount",14).toString(2))
    }
    private fun import(kind:String,file:String,withAudio:Boolean=true) {
        scenario.onActivity{vm.panelOpen=false;vm.importMedia(Uri.parse("content://com.motionstudio.editor.test.audio-fixtures/$file"),kind,withAudio)}
        compose.waitUntil(45000){vm.importTask==null&&vm.contentKind()==kind&&vm.state.saved}
        assertNull(vm.state.error)
    }
    @Test fun catalogueChainParametersAndUndoWorkFromThePanel() {
        solid();effects()
        assertEquals(65,availableEffects(vm).size)
        photo("effect-catalogue")
        addEffect("brightness_contrast");enterEffect()
        photo("effect-parameters")
        assertTimelineVisible()
        compose.onNodeWithContentDescription("播放/暂停").performClick()
        compose.waitUntil(10000){vm.playing&&vm.frame>3.0}
        // Playback continuously recomposes the playhead, so use a real system
        // tap rather than asking Compose/Espresso to become idle first.
        UiDevice.getInstance(InstrumentationRegistry.getInstrumentation())
            .wait(Until.findObject(By.desc("播放/暂停")),10000).also{assertNotNull(it)}!!.click()
        compose.waitUntil(10000){!vm.playing}
        val instance=vm.layer(vm.selected)!!.getJSONArray("effects").getJSONObject(0).getLong("id")
        compose.onNodeWithTag("effect-wheel-p0001-0").performScrollTo().performTouchInput{swipe(androidx.compose.ui.geometry.Offset(width*.8f,centerY),androidx.compose.ui.geometry.Offset(width*.2f,centerY),400)}
        compose.waitUntil(15000){vm.state.saved&&vm.effectParam(vm.selected,instance,"p0001")!!.getJSONObject("track").getJSONArray("value").getDouble(0)>0}
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.effectParam(vm.selected,instance,"p0001")!!.getJSONObject("track").getJSONArray("value").getDouble(0)==0.0}
        assertNull(vm.state.error)
    }
    @Test fun effectAnimationRoutesCompositionFramesAndReusesCurveHandles() {
        solid();effects();addEffect("brightness_contrast");enterEffect()
        val objectId=vm.selected
        val instance=vm.layer(objectId)!!.getJSONArray("effects").getJSONObject(0).getLong("id")
        compose.onNodeWithTag("effect-select-p0001").performScrollTo().performClick()
        compose.onNodeWithTag("effect-animate-p0001").performClick()
        compose.waitUntil(10000){vm.effectParam(objectId,instance,"p0001")!!.getJSONObject("track").getJSONArray("keys").length()==1}
        longPressKey(0)
        compose.onNodeWithTag("timeline-key-copy").assertIsNotEnabled()
        compose.onNodeWithTag("timeline-key-destination").performTextReplacement("20")
        compose.onNodeWithTag("timeline-key-copy").performClick()
        compose.waitUntil(10000){vm.keys().size==2&&vm.state.saved}
        longPressKey(20)
        compose.onNodeWithTag("timeline-key-destination").performTextReplacement("25")
        compose.onNodeWithTag("timeline-key-move").performClick()
        compose.waitUntil(10000){vm.keys().any{it.getInt("frame")==25}&&vm.state.saved}
        longPressKey(25)
        compose.onNodeWithTag("timeline-key-delete").performClick()
        compose.waitUntil(10000){vm.keys().size==1&&vm.state.saved}
        scenario.onActivity{vm.seek(30.0);vm.chooseEffectParam(instance,"p0001");vm.setValue(JSONArray(listOf(80,0,0,0)))}
        compose.waitUntil(10000){vm.keys().size==2&&vm.state.saved}
        scenario.onActivity{vm.seek(15.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==15.0}
        compose.onNodeWithTag("effect-easing").performClick()
        compose.onNodeWithTag("curve-kind-elastic").performScrollTo().performClick()
        compose.waitUntil(10000){vm.easingDefinition()?.optJSONObject("curve")?.getJSONObject("shape")?.getString("kind")=="elastic"}
        photo("effect-easing")
        assertTimelineVisible()
        scenario.onActivity{vm.trimClip(objectId,0,45);vm.moveClip(objectId,10)}
        compose.waitUntil(10000){vm.timelineLayer(objectId)?.optInt("in_frame")==10}
        assertEquals(listOf(10,40),vm.keys().map{it.getInt("frame")})
        assertNull(vm.state.error)
    }
    @Test fun independentColorCurvesSaveFiveChannels() {
        solid();effects();addEffect("curves");enterEffect()
        compose.onNodeWithTag("effect-color-curve").performScrollTo().performTouchInput{click(androidx.compose.ui.geometry.Offset(width*.5f,height*.25f))}
        compose.onNodeWithTag("effect-color-curve-actions").performScrollTo()
        compose.onNodeWithTag("effect-color-curve-apply").performScrollTo().performClick()
        val instance=vm.layer(vm.selected)!!.getJSONArray("effects").getJSONObject(0).getLong("id")
        compose.waitUntil(10000){vm.state.saved&&vm.effectParam(vm.selected,instance,"p0001")?.getJSONObject("curve")?.getJSONObject("value")?.getJSONArray("channels")?.getJSONArray(0)?.length()==3}
        assertEquals(5,vm.effectParam(vm.selected,instance,"p0001")!!.getJSONObject("curve").getJSONObject("value").getJSONArray("channels").length())
        photo("effect-color-curves")
        assertTimelineVisible()
    }
    @Test fun scrubbingWhileEditingUpdatesTheSelectedParameterAndPreview() {
        solid();effects();addEffect("brightness_contrast");enterEffect()
        compose.onNodeWithTag("effect-select-p0001").performScrollTo().performClick()
        compose.onNodeWithTag("effect-animate-p0001").performClick()
        val objectId=vm.selected
        val instance=vm.layer(objectId)!!.getJSONArray("effects").getJSONObject(0).getLong("id")
        val property="effect:$instance:p0001"
        val contrastBefore=vm.effectParam(objectId,instance,"p0002")!!.toString()
        compose.waitUntil(10000){vm.keys().size==1&&vm.state.saved}
        scenario.onActivity{vm.seek(30.0);vm.setValue(JSONArray(listOf(100,0,0,0)))}
        compose.waitUntil(10000){vm.keys().size==2&&vm.state.saved}
        scenario.onActivity{vm.seek(0.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==0.0}
        val distance=15*vm.timelineScale*context.resources.displayMetrics.density
        compose.onNodeWithTag("timeline").performTouchInput {
            down(androidx.compose.ui.geometry.Offset(centerX,24*context.resources.displayMetrics.density))
            moveBy(androidx.compose.ui.geometry.Offset(-distance,0f),300);advanceEventTime(150);up()
        }
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==15.0}
        assertEquals(property,vm.property);assertEquals(objectId,vm.selected)
        assertEquals(50.0,(vm.sampleValue() as JSONArray).getDouble(0),.01)
        compose.onNodeWithTag("effect-value-p0001-0").assertTextContains("50.000")
        assertTimelineVisible();photo("effect-timeline-scrubbed")
        compose.onNodeWithTag("effect-animate-p0001").performClick()
        compose.waitUntil(10000){vm.keys().size==3&&vm.state.saved}
        assertEquals(listOf(0,15,30),vm.keys().map{it.getInt("frame")})
        assertEquals(contrastBefore,vm.effectParam(objectId,instance,"p0002")!!.toString())
    }
    @Test fun focusedTimelineKeepsLayerOrderAndBackRestoresTheEditor() {
        solid()
        scenario.onActivity{vm.addRectangle()}
        compose.waitUntil(10000){vm.state.project!!.getJSONArray("layers").length()==2&&vm.state.saved}
        effects();addEffect("brightness_contrast");enterEffect()
        val order=vm.state.project!!.getJSONArray("layers").objects().map{it.getLong("id")}
        val density=context.resources.displayMetrics.density
        compose.onNodeWithTag("timeline").performTouchInput {
            down(androidx.compose.ui.geometry.Offset(centerX+40*density,65*density));advanceEventTime(650)
            moveBy(androidx.compose.ui.geometry.Offset(0f,25*density),100);up()
        }
        compose.waitForIdle()
        assertEquals(order,vm.state.project!!.getJSONArray("layers").objects().map{it.getLong("id")})
        assertTimelineVisible()
        compose.onNodeWithContentDescription("关闭效果").performClick()
        compose.onNodeWithTag("effects-panel").assertDoesNotExist()
        compose.onNodeWithTag("timeline").assertIsDisplayed()
        compose.onNodeWithTag("add-layer").assertIsDisplayed()
        assertNull(vm.state.error)
    }
    @Test fun wheelUsesRelativeContinuousMotionAndAccessibleInput() {
        solid();effects();addEffect("brightness_contrast");enterEffect()
        val instance=vm.layer(vm.selected)!!.getJSONArray("effects").getJSONObject(0).getLong("id")
        val wheel=compose.onNodeWithTag("effect-wheel-p0001-0").performScrollTo()
        assertWheelPainted("effect-wheel-p0001-0")
        wheel.performTouchInput{click(androidx.compose.ui.geometry.Offset(width*.9f,centerY))}
        assertEquals(0.0,vm.effectParam(vm.selected,instance,"p0001")!!.getJSONObject("track").getJSONArray("value").getDouble(0),.00001)
        wheel.performTouchInput{swipe(androidx.compose.ui.geometry.Offset(width*.85f,centerY),androidx.compose.ui.geometry.Offset(width*.15f,centerY),500)}
        compose.waitUntil(10000){vm.state.saved&&vm.effectParam(vm.selected,instance,"p0001")!!.getJSONObject("track").getJSONArray("value").getDouble(0)>0}
        val value=vm.effectParam(vm.selected,instance,"p0001")!!.getJSONObject("track").getJSONArray("value").getDouble(0)
        assertTrue("wheel jumps across the range",value<50)
        compose.onNodeWithTag("effect-value-p0001-0").assertTextContains(String.format(java.util.Locale.US,"%.3f",value))
        wheel.performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.SetProgress){it(149.9f)}
        compose.waitUntil(10000){vm.state.saved&&vm.effectParam(vm.selected,instance,"p0001")!!.getJSONObject("track").getJSONArray("value").getDouble(0)>149.89}
        wheel.performTouchInput{swipe(androidx.compose.ui.geometry.Offset(width*.85f,centerY),androidx.compose.ui.geometry.Offset(width*.15f,centerY),300)}
        compose.waitUntil(10000){vm.state.saved&&vm.effectParam(vm.selected,instance,"p0001")!!.getJSONObject("track").getJSONArray("value").getDouble(0)==150.0}
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.state.saved&&vm.effectParam(vm.selected,instance,"p0001")!!.getJSONObject("track").getJSONArray("value").getDouble(0)<150}
        assertNull(vm.state.error);photo("effect-numeric-wheel")
    }
    @Test fun queuedGestureUpdatesKeepAllTargetsAndOneUndo() {
        solid()
        val objectId=vm.selected
        scenario.onActivity{vm.setThreeD(true)}
        compose.waitUntil(10000){vm.state.saved&&vm.threeD()}
        scenario.onActivity{vm.openProperty("position");vm.separateDimensions()}
        compose.waitUntil(10000){vm.state.saved&&vm.isSeparated()}
        effects();addEffect("brightness_contrast");enterEffect()
        val instance=vm.layer(objectId)!!.getJSONArray("effects").getJSONObject(0).getLong("id")
        val original=vm.layer(objectId)!!.toString()
        val revision=vm.state.sample!!.getLong("revision")
        val blocked=CountDownLatch(1);val release=CountDownLatch(1)
        val field=EditorViewModel::class.java.getDeclaredField("worker").apply{isAccessible=true}
        (field.get(vm) as Handler).post{blocked.countDown();release.await(10,TimeUnit.SECONDS)}
        assertTrue(blocked.await(5,TimeUnit.SECONDS))
        try {
            scenario.onActivity {
                vm.beginGesture()
                repeat(500){i->
                    val v=i/10.0
                    vm.effectAction(objectId,instance,"set",JSONObject().put("param","p0001").put("frame",0).put("value",JSONArray(listOf(v,0,0,0))),false)
                    vm.effectAction(objectId,instance,"set",JSONObject().put("param","p0002").put("frame",0).put("value",JSONArray(listOf(-v,0,0,0))),false)
                    vm.setPropertyValue(objectId,"position",0,JSONArray(listOf(100+v,200+v,300+v,0)),false,listOf(0,1,2))
                }
                vm.endGesture()
            }
        }finally{release.countDown()}
        compose.waitUntil(15000){vm.state.saved&&kotlin.math.abs(vm.effectParam(objectId,instance,"p0001")!!.getJSONObject("track").getJSONArray("value").getDouble(0)-49.9)<.00001}
        assertEquals(-49.9,vm.effectParam(objectId,instance,"p0002")!!.getJSONObject("track").getJSONArray("value").getDouble(0),.00001)
        val axes=vm.propertyTrack(objectId,"position")!!.getJSONObject("axes")
        listOf("x","y","z").forEachIndexed{i,axis->assertEquals(100*(i+1)+49.9,axes.getJSONObject(axis).getDouble("value"),.00001)}
        val applied=vm.state.sample!!.getLong("revision")-revision
        assertTrue("obsolete gesture commands accumulated: $applied",applied<=8)
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.state.saved&&vm.layer(objectId)!!.toString()==original}
        assertNull(vm.state.error)
        File(root,"gesture-queue-report.json").writeText(JSONObject().put("submittedAssignments",2500).put("appliedRevisionChanges",applied).put("allTargetsPreserved",true).put("singleUndo",true).toString(2))
    }
    private fun copyPreview():Bitmap {
        fun find(view:View):SurfaceView? {
            if(view is SurfaceView)return view
            if(view is ViewGroup)for(i in 0 until view.childCount)find(view.getChildAt(i))?.let{return it}
            return null
        }
        var surface:SurfaceView?=null
        onUi{surface=find(activity.window.decorView)}
        val view=surface!!;val bitmap=Bitmap.createBitmap(view.width,view.height,Bitmap.Config.ARGB_8888)
        val complete=CountDownLatch(1);var result=-1
        onUi{PixelCopy.request(view,bitmap,{result=it;complete.countDown()},Handler(Looper.getMainLooper()))}
        assertTrue(complete.await(5,TimeUnit.SECONDS));assertEquals(PixelCopy.SUCCESS,result)
        return bitmap
    }
    private fun onUi(action:()->Unit) {
        val complete=CountDownLatch(1);var failure:Throwable?=null
        Handler(Looper.getMainLooper()).post{try{action()}catch(e:Throwable){failure=e}finally{complete.countDown()}}
        assertTrue("main thread stopped responding",complete.await(5,TimeUnit.SECONDS))
        failure?.let{throw it}
    }
    private fun verifyVideoPlayback(withAudio:Boolean,file:String="sound-24fps.mp4") {
        import("video",file,withAudio)
        scenario.onActivity{vm.panelOpen=false;vm.clearMediaNotice();vm.seek(0.0)}
        photo("video-playback-start")
        val presented=mutableSetOf<Int>();val pixels=mutableSetOf<Int>()
        onUi{vm.togglePlay()}
        compose.waitUntil(10000){vm.playing}
        val end=SystemClock.elapsedRealtime()+2500
        try {
            while(SystemClock.elapsedRealtime()<end) {
                onUi{vm.refreshDiagnostics()}
                SystemClock.sleep(100)
                vm.state.sample?.takeUnless{it.isNull("lastPresentedFrame")}?.let{presented+=it.getDouble("lastPresentedFrame").toInt()}
                val bitmap=copyPreview()
                val sample=Bitmap.createScaledBitmap(bitmap,64,36,false);val colors=IntArray(64*36)
                sample.getPixels(colors,0,64,0,0,64,36);sample.recycle()
                if(pixels.add(colors.contentHashCode())&&pixels.size<=6)File(root,"video-playing-${pixels.size}.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)}
                bitmap.recycle()
            }
        }finally{onUi{vm.pause()}}
        File(root,"video-playback-report.json").writeText(JSONObject().put("source",file).put("withAudio",withAudio).put("presentedFrames",JSONArray(presented.sorted())).put("differentPreviewImages",pixels.size).toString(2))
        assertTrue("video stays on the frame before playback: $presented",presented.size>=4)
        assertTrue("presented video pixels never change: ${pixels.size}",pixels.size>=3)
        scenario.onActivity{vm.seek(30.0)};photo("video-playback-after-seek")
        assertNull(vm.state.error)
    }
    @Test fun videoPlaybackUpdatesDecodedFramesWithOriginalSound(){verifyVideoPlayback(true)}
    @Test fun videoPlaybackUpdatesDecodedFramesWithoutSound(){verifyVideoPlayback(false)}
    @Test fun videoPlaybackUpdates1080pDecodedFramesWithoutSound(){verifyVideoPlayback(false,"preview-1080p.mp4")}
    @Test fun compositionSurroundMarksTheFrameAndKeepsItsExportedBackground() {
        val background=vm.state.project!!.getJSONArray("background").toString()
        photo("composition-boundary")
        val pixels=compose.onNodeWithTag("composition-boundary").captureToImage().toPixelMap()
        val fit=minOf(pixels.width/256f,pixels.height/144f)
        val left=(pixels.width-256*fit)/2;val top=(pixels.height-144*fit)/2
        assertTrue("composition must have an outside region in this window",maxOf(left,top)>4)
        val x=if(left>top)(left/2).roundToInt()else pixels.width/2
        val y=if(left>top)pixels.height/2 else (top/2).roundToInt()
        val outside=pixels[x,y]
        assertEquals(48f,outside.red*255,2f);assertEquals(54f,outside.green*255,2f);assertEquals(64f,outside.blue*255,2f)
        assertEquals(background,vm.state.project!!.getJSONArray("background").toString())
        var output:File?=null
        scenario.onActivity{vm.output(true){output=it}}
        compose.waitUntil(30000){output!=null||vm.state.error!=null}
        assertNull(vm.state.error);assertNotNull(output)
        val bitmap=BitmapFactory.decodeFile(output!!.absolutePath)
        assertEquals(256,bitmap.width);assertEquals(144,bitmap.height)
        val actual=bitmap.getPixel(128,72);bitmap.recycle()
        val expected=JSONArray(background)
        assertEquals(expected.getDouble(0)*255,Color.red(actual).toDouble(),2.0)
        assertEquals(expected.getDouble(1)*255,Color.green(actual).toDouble(),2.0)
        assertEquals(expected.getDouble(2)*255,Color.blue(actual).toDouble(),2.0)
        File(root,"composition-boundary-report.json").writeText(JSONObject().put("surroundRgb",JSONArray(listOf(48,54,64))).put("compositionWidth",256).put("compositionHeight",144).put("backgroundPreserved",true).toString(2))
    }
    @Test fun videoEffectMenusOpenFromPropertiesAndFooter() {
        import("video","sound-24fps.mp4")
        scenario.onActivity{vm.clearMediaNotice()}
        compose.onNodeWithContentDescription("图层操作").performClick()
        compose.onNodeWithTag("open-effects").assertIsDisplayed().performClick()
        compose.onNodeWithTag("effects-panel").assertIsDisplayed()
        assertTimelineVisible();photo("video-effects-from-properties")
        compose.onNodeWithContentDescription("关闭效果").performClick()
        compose.onNodeWithContentDescription("图层快捷操作").performClick()
        compose.onNodeWithTag("open-effects").assertIsDisplayed().performClick()
        compose.onNodeWithTag("effects-add").performClick();addEffect("brightness_contrast")
        assertNotNull(vm.audioClip());assertTimelineVisible();photo("video-effects-from-footer")
    }
    private fun meanRgbDifference(a:Bitmap,b:Bitmap):Double {
        assertEquals(a.width,b.width);assertEquals(a.height,b.height)
        var sum=0L;var count=0
        for(y in 0 until a.height step 2)for(x in 0 until a.width step 2) {
            val left=a.getPixel(x,y);val right=b.getPixel(x,y)
            sum+=kotlin.math.abs(Color.red(left)-Color.red(right))+kotlin.math.abs(Color.green(left)-Color.green(right))+kotlin.math.abs(Color.blue(left)-Color.blue(right))
            count+=3
        }
        return sum.toDouble()/count
    }
    @Test fun videoEffectsUseMovingFramesInPreviewPngAndEncodedOutput() {
        import("video","sound-24fps.mp4")
        scenario.onActivity{vm.clearMediaNotice();vm.seek(0.0)}
        effects();photo("video-effect-original")
        val original=copyPreview()
        addEffect("brightness_contrast");photo("video-effect-neutral")
        val neutral=copyPreview();val neutralError=meanRgbDifference(original,neutral)
        original.recycle();neutral.recycle()
        assertTrue("neutral video effect replaced the decoded image: $neutralError",neutralError<2)
        val objectId=vm.selected;val instance=vm.layer(objectId)!!.getJSONArray("effects").getJSONObject(0).getLong("id")
        scenario.onActivity{vm.effectAction(objectId,instance,"set",JSONObject().put("param","p0001").put("frame",0).put("value",JSONArray(listOf(40,0,0,0))))}
        compose.waitUntil(10000){vm.effectParam(objectId,instance,"p0001")!!.getJSONObject("track").getJSONArray("value").getDouble(0)==40.0&&vm.state.saved}
        photo("video-effect-adjusted");val adjusted=copyPreview()
        scenario.onActivity{vm.effectAction(objectId,instance,"enable",JSONObject().put("enabled",false))}
        compose.waitUntil(10000){!vm.effectInstance(objectId,instance)!!.getBoolean("enabled")&&vm.state.saved}
        photo("video-effect-disabled");val disabled=copyPreview()
        val effectDifference=meanRgbDifference(adjusted,disabled);disabled.recycle()
        assertTrue("video effect did not change preview pixels: $effectDifference",effectDifference>5)
        scenario.onActivity{vm.effectAction(objectId,instance,"enable",JSONObject().put("enabled",true));vm.seek(30.0)}
        photo("video-effect-later-frame");val later=copyPreview()
        val frameDifference=meanRgbDifference(adjusted,later);adjusted.recycle();later.recycle()
        assertTrue("video effect froze the decoded frames: $frameDifference",frameDifference>5)
        scenario.onActivity{vm.seek(0.0)};photo("video-effect-export-frame")
        var png:File?=null
        scenario.onActivity{vm.output(true){png=it}}
        compose.waitUntil(30000){png!=null||vm.state.error!=null}
        assertNull(vm.state.error);assertNotNull(png)
        val reference=BitmapFactory.decodeFile(png!!.absolutePath)
        File(root,"video-effect-capture.png").outputStream().use{reference.compress(Bitmap.CompressFormat.PNG,100,it)}
        var output:File?=null
        scenario.onActivity{vm.exportVideo{output=it}}
        compose.waitUntil(90000){output!=null||(!vm.exporting&&vm.state.error!=null)}
        assertNull(vm.state.error);assertNotNull(output)
        val retriever=MediaMetadataRetriever()
        val decoded=try{retriever.setDataSource(output!!.absolutePath);retriever.getFrameAtIndex(0)!!}finally{retriever.release()}
        val exportError=meanRgbDifference(reference,decoded);reference.recycle();decoded.recycle()
        assertTrue("encoded video effect differs from PNG: $exportError",exportError<8)
        File(root,"video-effects-report.json").writeText(JSONObject().put("neutralMeanRgb",neutralError).put("enabledDifferenceRgb",effectDifference).put("laterFrameDifferenceRgb",frameDifference).put("encodedVsPngMeanRgb",exportError).toString(2))
        val extractor=MediaExtractor()
        try {
            extractor.setDataSource(output!!.absolutePath)
            assertEquals(setOf("video/avc","audio/mp4a-latm"),(0 until extractor.trackCount).map{extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)}.toSet())
        }finally{extractor.release()}
    }
    @Test fun audioImportsWaveformPlaysAndMutesWithoutSpatialControls() {
        import("audio","tone-stereo-48000.wav")
        compose.onNodeWithTag("audio-properties").assertExists()
        compose.onNodeWithTag("layer-3d-toggle").assertDoesNotExist()
        val objectId=vm.selected
        compose.waitUntil(15000){vm.waveforms[objectId]?.getJSONArray("buckets")?.length()?.let{it>0}==true}
        assertTrue(vm.waveforms[objectId]!!.getJSONArray("buckets").objects().any{it.getDouble("rms")>.05})
        photo("audio-waveform")
        scenario.onActivity{vm.togglePlay()}
        compose.waitUntil(10000){vm.playing&&vm.frame>3.0}
        scenario.onActivity{vm.pause()}
        compose.onNodeWithTag("audio-enabled").performClick()
        compose.waitUntil(10000){vm.audioClip(objectId)!!.getBoolean("muted")&&vm.state.saved}
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){!vm.audioClip(objectId)!!.getBoolean("muted")}
        assertNull(vm.state.error)
    }
    @Test fun videoImportsRealFramesOriginalSoundAndExportsTwoTracks() {
        import("video","sound-24fps.mp4")
        assertNotNull(vm.audioClip())
        scenario.onActivity{vm.panelOpen=false;vm.clearMediaNotice();vm.seek(15.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==15.0}
        photo("video-preview")
        var output:File?=null
        scenario.onActivity{vm.exportVideo{output=it}}
        compose.waitUntil(90000){output!=null||(!vm.exporting&&vm.state.error!=null)}
        assertNull(vm.state.error);assertNotNull(output)
        val extractor=MediaExtractor()
        try {
            extractor.setDataSource(output!!.absolutePath)
            val types=(0 until extractor.trackCount).map{extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)}
            assertEquals(setOf("video/avc","audio/mp4a-latm"),types.toSet())
            val report=JSONObject(File(output!!.parentFile,output!!.nameWithoutExtension+"-report.json").readText())
            assertTrue(report.getBoolean("completed"));assertTrue(report.getLong("applicationFrameUploads")>0);assertTrue(report.getBoolean("audioMuxed"))
        }finally{extractor.release()}
    }
    @Test fun cancelledImportAndInvalidPluginKeepTheProject() {
        val before=vm.state.project!!.getJSONArray("layers").length()
        scenario.onActivity{vm.importMedia(Uri.parse("content://com.motionstudio.editor.test.audio-fixtures/slow.wav"),"audio");vm.cancelMediaImport()}
        compose.waitUntil(10000){vm.importTask==null}
        assertEquals(before,vm.state.project!!.getJSONArray("layers").length())
        scenario.onActivity{vm.installPlugin(Uri.parse("content://com.motionstudio.editor.test.audio-fixtures/tone-stereo-48000.wav"))}
        compose.waitUntil(10000){vm.state.error!=null}
        assertEquals(before,vm.state.project!!.getJSONArray("layers").length())
    }
    @Test fun compactRowsRouteNeighboursAndKeysAndResetScrollForEffects() {
        multipleLayers();photo("layout-timeline-multiple-layers");assertCompactLayerGap()
        val density=context.resources.displayMetrics.density
        val rowHeight=timelineRowHeightDp(context.resources.configuration.fontScale)*density
        val timeline=compose.onNodeWithTag("timeline")
        timeline.performTouchInput{click(androidx.compose.ui.geometry.Offset(centerX+16*density,44*density+rowHeight*1.5f))}
        compose.waitUntil(10000){vm.selected==13L}
        val before=vm.state.project!!.toString()
        timeline.performTouchInput{click(androidx.compose.ui.geometry.Offset(24*density,44*density+rowHeight*1.5f))}
        compose.waitUntil(10000){vm.layer(13)?.optBoolean("visible")==false&&vm.state.saved}
        assertTrue(vm.layer(14)!!.getBoolean("visible"))
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.state.project!!.toString()==before&&vm.state.saved}
        scenario.onActivity{vm.addKey()}
        compose.waitUntil(10000){vm.keys().size==1&&vm.state.saved}
        val beforeKey=vm.state.project!!.toString()
        scenario.onActivity{vm.select(14,false)}
        timeline.performTouchInput {
            down(androidx.compose.ui.geometry.Offset(centerX,37*density+rowHeight*2));moveBy(androidx.compose.ui.geometry.Offset(30*density,0f),100);up()
        }
        compose.waitUntil(10000){vm.selected==13L&&vm.keys().any{it.getInt("frame")==25}&&vm.state.saved}
        assertEquals(15,vm.propertyTrack(14,"position")!!.getJSONArray("keys").getJSONObject(0).getInt("frame"))
        scenario.onActivity{vm.undo();vm.seek(15.0)}
        compose.waitUntil(10000){vm.state.project!!.toString()==beforeKey&&vm.state.saved&&vm.state.sample?.optDouble("frame")==15.0}
        timeline.performTouchInput {
            down(androidx.compose.ui.geometry.Offset(width-28*density,44*density+rowHeight/2));moveBy(androidx.compose.ui.geometry.Offset(0f,-720*density),100);up()
        }
        photo("layout-timeline-scrolled")
        timeline.performTouchInput{click(androidx.compose.ui.geometry.Offset(centerX+16*density,height-rowHeight/2))}
        compose.waitUntil(10000){vm.selected==1L}
        scenario.onActivity{vm.openEffects()}
        compose.onNodeWithTag("effects-panel").assertIsDisplayed()
        photo("layout-timeline-focused")
        val pixels=timeline.captureToImage().toPixelMap()
        val x=(pixels.width/2+16*density).roundToInt().coerceAtMost(pixels.width-1)
        assertTrue("selected clip was lost after focusing a scrolled timeline",pixels[x,(49*density).roundToInt()].blue>.21f)
        assertNull(vm.state.error)
    }
    @Test fun effectsAndAddMediaControlsFitTheRealWindow() {
        multipleLayers();photo("layout-timeline-multiple-layers");assertCompactLayerGap()
        solid();effects();photo("layout-effect-catalogue")
        assertTimelineVisible()
        val nodes=compose.onAllNodes(hasClickAction()).fetchSemanticsNodes()
        val density=context.resources.displayMetrics.density
        val visible=nodes.filter{it.boundsInRoot.width>0&&it.boundsInRoot.height>0}
        assertTrue(visible.size>=7)
        visible.filter{it.config.getOrElse(androidx.compose.ui.semantics.SemanticsProperties.TestTag){""}.startsWith("effect-category-")}.forEach{node->
            if(node.boundsInRoot.height>=1)assertTrue("short touch target ${node.boundsInRoot}",node.boundsInRoot.height/density>=47.5)
        }
        addEffect("brightness_contrast");enterEffect();photo("layout-effect-parameters")
        assertTimelineVisible()
        compose.onNodeWithTag("effect-wheel-p0001-0").performScrollTo().assertIsDisplayed()
        assertWheelPainted("effect-wheel-p0001-0")
        verifyLayoutAdjustment()
        compose.onNodeWithTag("effect-select-p0001").performScrollTo().performClick()
        compose.onNodeWithTag("effect-animate-p0001").performClick()
        compose.waitUntil(10000){vm.keys().size==1&&vm.state.saved}
        scenario.onActivity{vm.seek(30.0);vm.setValue(JSONArray(listOf(80,0,0,0)))}
        compose.waitUntil(10000){vm.keys().size==2&&vm.state.saved}
        scenario.onActivity{vm.seek(15.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==15.0}
        compose.onNodeWithTag("effect-easing").performClick()
        assertTimelineVisible();photo("layout-effect-easing")
        assertTrue("curve graph is too small",compose.onNodeWithTag("easing-graph").fetchSemanticsNode().boundsInRoot.height/density>=60)
        scenario.onActivity{vm.effectsOpen=false;vm.property="position"}
        import("audio","tone-stereo-48000.wav")
        photo("layout-audio")
        compose.onNodeWithTag("audio-volume").performScrollTo().assertIsDisplayed()
        scenario.onActivity{vm.panelOpen=false;vm.clearMediaNotice()}
        compose.onNodeWithTag("add-layer").performClick()
        compose.onNodeWithTag("add-video").performScrollTo().assertIsDisplayed()
        photo("layout-add-media")
        val config=context.resources.configuration
        File(root,"layout-profile-report.json").writeText(JSONObject().put("fontScale",config.fontScale).put("densityDpi",context.resources.displayMetrics.densityDpi)
            .put("screenWidthDp",config.screenWidthDp).put("screenHeightDp",config.screenHeightDp).put("passed",true).toString(2))
    }
}
