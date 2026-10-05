package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import android.media.MediaExtractor
import android.media.MediaFormat
import android.net.Uri
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

@RunWith(AndroidJUnit4::class)
class IntegratedFrontendTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    private val context get()=InstrumentationRegistry.getInstrumentation().targetContext
    @Before fun setup() {
        root=File(context.filesDir,"acceptance/integrated-ui-${UUID.randomUUID()}").apply{mkdirs()}
        val p=nativeData(NativeBridge.projectTemplate(0)).put("width",256).put("height",144).put("frames",60).put("layers",JSONArray())
        p.getJSONObject("camera").put("created",false)
        File(root,"project.json").writeText(p.toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
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
        assertTrue("timeline does not show a complete layer",timeline.height/density>=103)
        assertTrue("preview is too small",preview.height/density>=48&&preview.width/density>=120)
        assertFalse("effects cover timeline",panel.overlaps(timeline))
        assertFalse("effects cover preview",panel.overlaps(preview))
        assertFalse("effects cover playback controls",panel.overlaps(transport))
        assertFalse("preview covers timeline",preview.overlaps(timeline))
        compose.onNodeWithTag("effect-time").assertDoesNotExist()
    }
    private fun longPressKey(frame:Int) {
        val offset=(frame-vm.frame).toFloat()*vm.timelineScale*context.resources.displayMetrics.density
        val y=9*context.resources.displayMetrics.density
        compose.onNodeWithTag("timeline").performTouchInput{longClick(androidx.compose.ui.geometry.Offset(centerX+offset,y))}
    }
    private fun import(kind:String,file:String,withAudio:Boolean=true) {
        scenario.onActivity{vm.panelOpen=false;vm.importMedia(Uri.parse("content://com.motionstudio.editor.test.audio-fixtures/$file"),kind,withAudio)}
        compose.waitUntil(45000){vm.importTask==null&&vm.contentKind()==kind&&vm.state.saved}
        assertNull(vm.state.error)
    }
    @Test fun catalogueChainParametersAndUndoWorkFromThePanel() {
        solid();effects()
        assertEquals(36,availableEffects(vm).size)
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
        compose.onNodeWithTag("effect-slider-p0001-0").performScrollTo().performTouchInput{swipe(center,androidx.compose.ui.geometry.Offset(width*.8f,centerY),400)}
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
            moveBy(androidx.compose.ui.geometry.Offset(-distance,0f),300);up()
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
    @Test fun effectsAndAddMediaControlsFitTheRealWindow() {
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
        compose.onNodeWithTag("effect-slider-p0001-0").performScrollTo().assertIsDisplayed()
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
