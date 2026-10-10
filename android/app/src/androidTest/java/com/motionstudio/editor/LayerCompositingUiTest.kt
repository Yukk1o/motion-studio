package com.motionstudio.editor

import android.content.Intent
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File
import java.util.UUID

class LayerCompositingUiTest {
    @get:Rule val compose=createEmptyComposeRule()
    @Test fun sharedPanelEditsSourcesModesAndSpaceWithUndoAndLivePreview() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        val root=File(app.filesDir,"acceptance/compositing-ui-${UUID.randomUUID()}").apply{mkdirs()}
        val project=nativeData(NativeBridge.projectTemplate(0)).put("frames",60)
        val layer=project.getJSONArray("layers").getJSONObject(0)
        val source=JSONObject(layer.toString()).put("id",2).put("name","遮罩来源").put("parent",JSONObject.NULL)
        project.put("layers",JSONArray().put(layer).put(source))
        File(root,"project.json").writeText(project.toString())
        val scenario=ActivityScenario.launch<AcceptanceActivity>(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        lateinit var vm:EditorViewModel
        try {
            scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
            fun settled(ready:()->Boolean){compose.waitUntil(20000){vm.state.error!=null||ready()};assertNull(vm.state.error);assertTrue(ready())}
            settled{vm.state.project!=null};scenario.onActivity{vm.select(1,false)};settled{vm.selected==1L}
            compose.onNodeWithTag("footer-compositing").performScrollTo().performClick()
            compose.onNodeWithTag("layer-compositing-panel").assertIsDisplayed()
            compose.onNodeWithTag("layer-blend-mode").performClick();compose.onNodeWithTag("layer-blend-mode-multiply").performClick()
            settled{vm.layer(1)!!.optJSONObject("blend")?.optString("mode")=="multiply"}
            compose.onNodeWithTag("layer-blend-space").performClick();compose.onNodeWithTag("layer-blend-space-srgb").performClick()
            settled{vm.layer(1)!!.getJSONObject("blend").getString("space")=="srgb"}
            compose.onNodeWithTag("layer-matte-source").performClick();compose.onNodeWithTag("layer-matte-source-2").performClick()
            settled{vm.layer(1)!!.optJSONObject("track_matte")?.optLong("source")==2L}
            compose.onNodeWithTag("layer-matte-mode").performClick();compose.onNodeWithTag("layer-matte-mode-luma_inverted").performClick()
            settled{vm.layer(1)!!.getJSONObject("track_matte").getString("mode")=="luma_inverted"}
            scenario.onActivity{vm.undo()};settled{vm.layer(1)!!.getJSONObject("track_matte").getString("mode")=="alpha"}
            compose.onNodeWithTag("preview-gesture").assertIsDisplayed();compose.onNodeWithTag("timeline").assertIsDisplayed()
            assertNull(vm.state.sample?.optString("renderError")?.takeIf{it.isNotEmpty()&&it!="null"})
        }finally{scenario.close()}
    }
}
