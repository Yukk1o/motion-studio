package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
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
}
