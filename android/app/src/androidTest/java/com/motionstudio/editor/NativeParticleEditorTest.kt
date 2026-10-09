package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.UiDevice
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class NativeParticleEditorTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(app.filesDir,"acceptance/native-particle-${UUID.randomUUID()}").apply{mkdirs()}
        scenario=ActivityScenario.launch(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(30000){vm.state.project!=null&&vm.catalogue!=null}
        val pkg=vm.catalogue!!.getJSONArray("packages").objects().first{it.getJSONObject("manifest").getString("id")=="com.motionstudio.effects.particles"}
        scenario.onActivity{vm.select(2,false);vm.openEffects();vm.pluginOperation(JSONObject().put("op","add").put("object",2).put("plugin",pkg.getJSONObject("manifest").getString("id")).put("version","1.0.0").put("hash",pkg.getString("hash")).put("effect","particle_emitter"),true)}
        compose.waitUntil(20000){vm.effectParam(2,1,"position")!=null&&vm.state.saved}
        compose.onNodeWithTag("effect-open-1").performScrollTo().performClick()
        compose.waitUntil(15000){vm.pluginEditor.session!=null}
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun keys()=vm.effectParam(2,1,"position")!!.getJSONObject("track").getJSONArray("keys").objects().map{it.getInt("frame")}
    private fun photo(name:String){InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot().also{b->File(root,"$name.png").outputStream().use{b.compress(Bitmap.CompressFormat.PNG,100,it)};b.recycle()}}
    @Test fun nativePageSharesGpuPreviewPlayheadAndKeysWithOuterEditor() {
        compose.onNodeWithTag("plugin-editor-native").assertIsDisplayed();compose.onNodeWithTag("native-plugin-preview").assertIsDisplayed();compose.onNodeWithTag("native-plugin-timeline").assertIsDisplayed()
        assertNull(vm.pluginEditor.session!!.definition.optJSONObject("editor"));assertTrue(vm.pluginEditor.session!!.assets.isEmpty())
        compose.onNodeWithTag("native-param-select-position").performScrollTo().performClick()
        compose.onNodeWithTag("native-plugin-key").performClick();compose.waitUntil(10000){keys()==listOf(0)}
        scenario.onActivity{vm.seek(30.0,true)};compose.waitUntil(10000){vm.pluginEditor.state?.optInt("frame")==30}
        compose.onNodeWithTag("native-plugin-key").performClick();compose.waitUntil(10000){keys()==listOf(0,30)}
        compose.onNodeWithTag("native-param-position-0").performScrollTo().performClick()
        compose.onNode(hasSetTextAction()).performTextReplacement("120");compose.onNodeWithText("确定").performClick()
        compose.waitUntil(10000){vm.effectParam(2,1,"position")!!.getJSONObject("track").getJSONArray("keys").getJSONObject(1).getJSONArray("value").getDouble(0)==120.0}
        compose.waitUntil(10000){vm.pluginEditor.state?.getJSONObject("values")?.getJSONArray("position")?.getDouble(0)==120.0}
        compose.onNodeWithTag("native-param-position-0").assertIsEnabled()
        photo("native-particle-preview-timeline")
        compose.onNodeWithTag("native-slot-tab-appearance").performClick();compose.onNodeWithTag("native-slot-sprite_asset").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("native-editor-done").performClick();compose.waitUntil(10000){vm.pluginEditor.session==null&&vm.state.saved}
        assertEquals("effect:1:position",vm.property)
        scenario.onActivity{vm.seek(15.0)};compose.waitUntil(10000){(vm.sampleValueFor(2,"effect:1:position") as? org.json.JSONArray)?.optDouble(0)==60.0}
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){keys().isEmpty()}
        scenario.onActivity{vm.redo()};compose.waitUntil(10000){keys()==listOf(0,30)}
        assertNull(vm.state.error)
    }
    @Test fun cancellationRestoresTheWholePageAndPlaybackUsesTheSharedClock() {
        val before=vm.effectParam(2,1,"rate")!!.toString()
        val pauseButton=compose.onNodeWithTag("native-plugin-play").fetchSemanticsNode().boundsInWindow.center
        compose.onNodeWithTag("native-plugin-play").performClick();compose.waitUntil(10000){vm.playing&&vm.frame>0}
        // Playback intentionally recomposes continuously; inject the real touch
        // without requiring Compose to become idle before pressing pause.
        UiDevice.getInstance(InstrumentationRegistry.getInstrumentation()).click(pauseButton.x.toInt(),pauseButton.y.toInt());compose.waitUntil(10000){!vm.playing}
        compose.onNodeWithTag("native-param-rate-0").performScrollTo().performClick()
        compose.onNode(hasSetTextAction()).performTextReplacement("500");compose.onNodeWithText("确定").performClick()
        compose.waitUntil(10000){vm.effectParam(2,1,"rate")!!.getJSONObject("track").getJSONArray("value").getDouble(0)==500.0}
        compose.onNodeWithTag("native-editor-cancel").performClick();compose.waitUntil(10000){vm.pluginEditor.session==null&&vm.effectParam(2,1,"rate")!!.toString()==before}
        assertFalse(vm.playing);assertNull(vm.state.error)
    }
    @Test fun nativeColorPaletteKeepsPreviewAndTimelineAndCancelsOnlyItsOwnTrack() {
        fun openPalette(){
            val button=compose.onNodeWithTag("native-param-color-palette")
            repeat(5){
                val bounds=button.fetchSemanticsNode().boundsInRoot
                val visible=compose.onNodeWithTag("native-parameters").fetchSemanticsNode().boundsInRoot
                if(bounds.top>=visible.top&&bounds.bottom<=visible.bottom&&bounds.height>0){button.assertIsDisplayed().performClick();return}
                compose.onNodeWithTag("native-parameters").performTouchInput{swipeUp()}
            }
            button.assertIsDisplayed().performClick()
        }
        fun paletteReady(){
            try {compose.waitUntil(10000){compose.onAllNodesWithTag("color-palette").fetchSemanticsNodes().size==1||compose.onAllNodesWithTag("native-color-error").fetchSemanticsNodes().isNotEmpty()}}
            catch(failure:Throwable){File(root,"native-color-failure.json").writeText(JSONObject().put("state",vm.pluginEditor.state).put("error",vm.state.error).put("tree",compose.onRoot().printToString()).toString(2));photo("native-color-failure");throw failure}
            val errors=compose.onAllNodesWithTag("native-color-error").fetchSemanticsNodes().flatMap{it.config[SemanticsProperties.Text]}
            assertTrue("Color errors=$errors, revision=${vm.pluginEditor.state?.optLong("revision")}, scope=${vm.pluginEditor.state?.opt("color_edit")}",errors.isEmpty())
        }
        val original=vm.effectParam(2,1,"color")!!.getJSONObject("track").toString()
        val token=vm.pluginEditor.session!!.token
        compose.onNodeWithTag("native-param-rate-0").performScrollTo().performClick()
        compose.onNode(hasSetTextAction()).performTextReplacement("500");compose.onNodeWithText("确定").performClick()
        compose.waitUntil(10000){vm.effectParam(2,1,"rate")!!.getJSONObject("track").getJSONArray("value").getDouble(0)==500.0}
        compose.onNodeWithTag("native-slot-tab-appearance").performClick()
        compose.onNodeWithTag("native-param-color-0").assertDoesNotExist()
        val preview=compose.onNodeWithTag("native-plugin-preview").fetchSemanticsNode().boundsInRoot
        val timeline=compose.onNodeWithTag("native-plugin-timeline").fetchSemanticsNode().boundsInRoot
        openPalette()
        paletteReady()
        compose.onNodeWithTag("color-palette").assertIsDisplayed()
        assertEquals(preview,compose.onNodeWithTag("native-plugin-preview").fetchSemanticsNode().boundsInRoot)
        assertEquals(timeline,compose.onNodeWithTag("native-plugin-timeline").fetchSemanticsNode().boundsInRoot)
        compose.onNodeWithTag("color-code").performClick()
        compose.onNodeWithTag("color-hex").performScrollTo().performTextReplacement("#FF804080")
        compose.waitUntil(10000){kotlin.math.abs(vm.effectParam(2,1,"color")!!.getJSONObject("track").getJSONArray("value").getDouble(3)-128.0/255)<.001}
        assertEquals(token,vm.pluginEditor.session!!.token);assertTrue(vm.pluginEditor.gesture);assertNull(vm.colorEditor)
        compose.onNodeWithTag("color-values-back").performClick();photo("native-particle-color-palette")
        compose.onNodeWithTag("color-eyedropper").performClick();compose.waitUntil(10000){vm.eyedropperActive}
        compose.onNodeWithTag("preview-gesture").performTouchInput{click(center)}
        compose.waitUntil(10000){!vm.eyedropperActive}
        assertEquals(128.0/255,vm.effectParam(2,1,"color")!!.getJSONObject("track").getJSONArray("value").getDouble(3),.001)
        compose.onNodeWithTag("color-cancel").performClick()
        compose.waitUntil(10000){vm.pluginEditor.state?.optJSONObject("color_edit")==null&&vm.effectParam(2,1,"color")!!.getJSONObject("track").toString()==original}
        assertEquals(500.0,vm.effectParam(2,1,"rate")!!.getJSONObject("track").getJSONArray("value").getDouble(0),0.0)
        openPalette()
        paletteReady()
        compose.onNodeWithTag("color-code").performClick();compose.onNodeWithTag("color-hex").performScrollTo().performTextReplacement("#FF804080")
        compose.onNodeWithTag("color-confirm").performClick()
        compose.waitUntil(10000){vm.pluginEditor.state?.optJSONObject("color_edit")==null&&vm.effectParam(2,1,"color")!!.getJSONObject("track").getJSONArray("value").getDouble(0)==1.0}
        compose.onNodeWithTag("native-plugin-key").performClick()
        compose.waitUntil(10000){vm.effectParam(2,1,"color")!!.getJSONObject("track").getJSONArray("keys").length()==1}
        scenario.onActivity{vm.seek(30.0,true)};compose.waitUntil(10000){vm.pluginEditor.state?.getInt("frame")==30}
        val beforeIntermediate=vm.effectParam(2,1,"color")!!.getJSONObject("track").toString()
        openPalette()
        paletteReady()
        compose.onNodeWithTag("color-code").performClick();compose.onNodeWithTag("color-hex").performScrollTo().performTextReplacement("#00AAFFFF")
        compose.waitUntil(10000){vm.effectParam(2,1,"color")!!.getJSONObject("track").getJSONArray("keys").length()==2}
        compose.onNodeWithTag("color-cancel").performClick()
        compose.waitUntil(10000){vm.effectParam(2,1,"color")!!.getJSONObject("track").toString()==beforeIntermediate}
        compose.onNodeWithTag("native-editor-done").performClick();compose.waitUntil(10000){vm.pluginEditor.session==null&&vm.state.saved}
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){vm.effectParam(2,1,"color")!!.getJSONObject("track").toString()==original}
        assertEquals(360.0,vm.effectParam(2,1,"rate")!!.getJSONObject("track").getJSONArray("value").getDouble(0),0.0)
        scenario.onActivity{vm.redo()};compose.waitUntil(10000){vm.effectParam(2,1,"color")!!.getJSONObject("track").toString()==beforeIntermediate}
        assertNull(vm.state.error)
    }
    @Test fun directEyedropperKeepsControlsVisibleAndCancelPreservesColorTrack() {
        compose.onNodeWithTag("native-slot-tab-appearance").performClick()
        fun clickPicker() {
            val button=compose.onNodeWithTag("native-param-color-eyedropper")
            repeat(5){
                val bounds=button.fetchSemanticsNode().boundsInRoot
                val viewport=compose.onNodeWithTag("native-parameters").fetchSemanticsNode().boundsInRoot
                if(bounds.top>=viewport.top&&bounds.bottom<=viewport.bottom&&bounds.height>0){photo("native-color-compact-row");button.performClick();return}
                compose.onNodeWithTag("native-parameters").performTouchInput{swipeUp()}
            }
            button.assertIsDisplayed().performClick()
        }
        val original=vm.effectParam(2,1,"color")!!.getJSONObject("track").toString()
        val alpha=vm.effectParam(2,1,"color")!!.getJSONObject("track").getJSONArray("value").getDouble(3)
        clickPicker();compose.waitUntil(10000){vm.eyedropperActive}
        compose.onNodeWithTag("native-parameters").assertIsDisplayed()
        compose.onNodeWithTag("color-selection-panel").assertDoesNotExist()
        compose.onNodeWithTag("color-palette").assertDoesNotExist()
        photo("native-color-direct-eyedropper")
        compose.onNodeWithTag("eyedropper-cancel").performClick()
        compose.waitUntil(10000){!vm.eyedropperActive&&vm.pluginEditor.state?.optJSONObject("color_edit")==null}
        assertEquals(original,vm.effectParam(2,1,"color")!!.getJSONObject("track").toString())
        clickPicker();compose.waitUntil(10000){vm.eyedropperActive}
        compose.onNodeWithTag("preview-gesture").performTouchInput{click(center)}
        compose.waitUntil(10000){!vm.eyedropperActive&&vm.pluginEditor.state?.optJSONObject("color_edit")==null}
        assertEquals(alpha,vm.effectParam(2,1,"color")!!.getJSONObject("track").getJSONArray("value").getDouble(3),.0001)
        compose.onNodeWithTag("native-plugin-expression").performClick();compose.waitUntil(10000){vm.pluginEditor.session==null&&vm.state.saved}
        compose.onNodeWithTag("expression-workspace").assertIsDisplayed()
        assertEquals("color",vm.expressionTarget!!.getString("param"))
        assertEquals(1L,vm.expressionTarget!!.getLong("effect"))
        assertNull(vm.state.error)
    }
}
