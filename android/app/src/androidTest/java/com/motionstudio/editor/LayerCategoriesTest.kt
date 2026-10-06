package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.*
import org.junit.Assert.*
import java.io.File
import java.util.UUID

class LayerCategoriesTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(context.filesDir,"acceptance/categories-${UUID.randomUUID()}").apply{mkdirs()}
        val project=LayerClipTimelineTest().fixture()
        project.getJSONArray("layers").getJSONObject(0).put("name","矩形")
        File(root,"project.json").writeText(project.toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null&&vm.catalogue!=null}
        scenario.onActivity{vm.select(2,false);vm.seek(40.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==40.0}
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun photo(name:String) {
        compose.mainClock.advanceTimeBy(500)
        compose.waitForIdle();InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        android.os.SystemClock.sleep(150)
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"$name.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
    }
    private fun import(kind:String,name:String) {
        scenario.onActivity{vm.importMedia(Uri.parse("content://com.motionstudio.editor.test.audio-fixtures/$name"),kind)}
        compose.waitUntil(45000){vm.state.error!=null||(vm.importTask==null&&vm.contentKind()==kind&&vm.state.saved)}
        assertNull("Media import failed before category interaction",vm.state.error)
        scenario.onActivity{vm.clearMediaNotice();vm.closeWorkspace()}
        assertNull(vm.state.error)
    }
    @Test fun transformGroupsSpatialPropertiesAndEffectsOpenDirectly() {
        val before=vm.state.project!!.toString();val frame=vm.frame
        photo("footer-shape-categories")
        compose.onNodeWithTag("footer-audio").assertDoesNotExist()
        compose.onNodeWithTag("footer-transform").performScrollTo().performClick()
        compose.onNodeWithTag("properties-panel").assertIsDisplayed()
        compose.onNodeWithTag("property-tab-rotation").performClick();assertEquals("rotation",vm.property)
        compose.onNodeWithTag("property-tab-scale").performClick();assertEquals("scale",vm.property)
        compose.onNodeWithTag("property-tab-opacity").performClick();assertEquals("opacity",vm.property)
        compose.onNodeWithTag("property-tab-opacity").assertIsSelected()
        compose.onNodeWithContentDescription("关闭属性面板").performClick()
        compose.onNodeWithTag("footer-transform").performScrollTo().performClick()
        compose.onNodeWithTag("property-tab-opacity").assertIsSelected()
        compose.onNodeWithContentDescription("关闭属性面板").performClick()
        compose.onNodeWithTag("open-effects").performScrollTo().performClick()
        compose.onNodeWithTag("effects-panel").assertIsDisplayed()
        assertEquals(before,vm.state.project!!.toString());assertEquals(frame,vm.frame,0.0)
        compose.onNodeWithContentDescription("关闭效果").performClick()
        compose.onNodeWithTag("footer-opacity").assertDoesNotExist()
        assertEquals(before,vm.state.project!!.toString());assertNull(vm.state.error)
    }
    @Test fun videoCategoriesSwipeWithoutScrubbingAndOpenOriginalAudio() {
        import("video","sound-24fps.mp4")
        val frame=vm.frame;val objectId=vm.selected;val before=vm.state.project!!.toString()
        val range=compose.onNodeWithTag("layer-categories").fetchSemanticsNode().config[SemanticsProperties.HorizontalScrollAxisRange]
        val requireOverflow=InstrumentationRegistry.getArguments().getString("requireFooterOverflow")=="true"
        if(requireOverflow)assertTrue("video categories should exceed the available width",range.maxValue()>0f)
        photo("footer-video-categories")
        compose.onNodeWithTag("layer-categories").performTouchInput{swipeLeft(durationMillis=300)}
        compose.waitForIdle();if(range.maxValue()>0f)assertTrue(range.value()>0f)
        assertEquals(frame,vm.frame,0.0);assertEquals(objectId,vm.selected);assertEquals(before,vm.state.project!!.toString())
        compose.onNodeWithTag("footer-opacity").assertDoesNotExist();photo("footer-video-scrolled")
        compose.onNodeWithTag("footer-audio").performScrollTo().performClick()
        compose.onNodeWithTag("audio-properties").assertIsDisplayed();compose.onNodeWithText("视频原声").assertIsDisplayed()
        val muted=vm.audioClip()!!.optBoolean("muted")
        compose.onNodeWithTag("audio-enabled").performScrollTo().performClick()
        compose.waitUntil(10000){vm.state.saved&&vm.audioClip()!!.optBoolean("muted")!=muted}
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.state.saved&&vm.state.project!!.toString()==before}
        assertEquals(frame,vm.frame,0.0);assertEquals(objectId,vm.selected);assertNull(vm.state.error)
        compose.onNodeWithContentDescription("关闭声音面板").performClick()
        compose.onNodeWithTag("open-effects").performScrollTo().performClick()
        compose.onNodeWithTag("effects-panel").assertIsDisplayed()
        compose.onNodeWithTag("effects-add").performClick()
        compose.onNodeWithTag("effect-add-brightness_contrast").performScrollTo().performClick()
        compose.waitUntil(15000){vm.state.saved&&vm.layer(objectId)?.optJSONArray("effects")?.length()==1}
        assertNotNull(vm.audioClip());assertNull(vm.state.error);photo("footer-video-effects")
    }
    @Test fun audioLayersOfferSoundAndKeepBatchSelectionInMoreActions() {
        import("audio","tone-stereo-48000.wav")
        compose.onNodeWithTag("footer-transform").assertDoesNotExist()
        compose.onNodeWithTag("open-effects").assertDoesNotExist()
        compose.onNodeWithTag("footer-opacity").assertDoesNotExist();photo("footer-audio-categories")
        compose.onNodeWithTag("footer-audio").performScrollTo().performClick()
        compose.onNodeWithTag("audio-properties").assertIsDisplayed();assertEquals("audio",vm.property)
        compose.onNodeWithContentDescription("关闭声音面板").performClick()
        compose.onNodeWithContentDescription("图层快捷操作").performClick()
        compose.onNodeWithTag("start-layer-selection").performClick()
        compose.onNodeWithTag("layer-selection-bar").assertIsDisplayed()
        assertEquals(setOf(vm.selected),vm.selectedLayerIds);assertNull(vm.state.error)
    }
    @Test fun cameraCategoriesRespectOrbitModeAndDoNotOfferLayerEffects() {
        scenario.onActivity{vm.addCamera()}
        compose.waitUntil(10000){vm.hasCamera()&&vm.state.saved}
        scenario.onActivity{vm.cameraMode(true);vm.closeWorkspace();vm.select(0,false)}
        compose.waitUntil(10000){vm.state.project!!.getJSONObject("camera").optString("mode")=="orbit"&&vm.state.saved}
        compose.onNodeWithTag("open-effects").assertDoesNotExist();compose.onNodeWithTag("footer-opacity").assertDoesNotExist()
        photo("footer-camera-categories")
        val before=vm.state.project!!.toString();val frame=vm.frame
        val range=compose.onNodeWithTag("layer-categories").fetchSemanticsNode().config[SemanticsProperties.HorizontalScrollAxisRange]
        if(InstrumentationRegistry.getArguments().getString("requireFooterOverflow")=="true")
            assertTrue("camera categories should exceed the available width",range.maxValue()>0f)
        compose.onNodeWithTag("layer-categories").performTouchInput{swipeLeft(durationMillis=300)}
        compose.waitForIdle();if(range.maxValue()>0f)assertTrue(range.value()>0f)
        assertEquals(frame,vm.frame,0.0);assertEquals(before,vm.state.project!!.toString())
        compose.onNodeWithTag("footer-target").assertIsDisplayed();photo("footer-camera-scrolled")
        compose.onNodeWithTag("footer-transform").performScrollTo().performClick()
        assertEquals("radius",vm.property);compose.onNodeWithTag("property-tab-radius").assertIsSelected()
        compose.onNodeWithContentDescription("关闭属性面板").performClick()
        compose.onNodeWithTag("footer-lens").performScrollTo().performClick();assertEquals("fov",vm.property)
        compose.onNodeWithContentDescription("关闭属性面板").performClick()
        compose.onNodeWithTag("footer-target").performScrollTo().performClick();assertEquals("target",vm.property)
        assertNull(vm.state.error)
    }
}
