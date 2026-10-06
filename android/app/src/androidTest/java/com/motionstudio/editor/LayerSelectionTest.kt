package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.geometry.Offset
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

class LayerSelectionTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    private var density=1f
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        density=context.resources.displayMetrics.density
        root=File(context.filesDir,"acceptance/selection-${UUID.randomUUID()}").apply{mkdirs()}
        val project=LayerClipTimelineTest().fixture()
        project.getJSONObject("camera").put("created",true)
        val original=project.getJSONArray("layers").getJSONObject(0)
        project.put("layers",JSONArray((2..4).map{id->JSONObject(original.toString()).put("id",id).put("name","图层 $id")
            .put("timeline",JSONObject().put("in_frame",20*(id-1)).put("out_frame",20*(id-1)+80).put("offset_frame",20*(id-1)))}))
        File(root,"project.json").writeText(project.toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null}
        scenario.onActivity{vm.select(4,false);vm.seek(40.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==40.0}
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun start() {
        compose.onNodeWithContentDescription("图层快捷操作").performClick()
        compose.onNodeWithTag("start-layer-selection").performClick()
        compose.onNodeWithTag("layer-selection-bar").assertIsDisplayed()
    }
    private fun more()=compose.onNodeWithContentDescription("批量图层操作").performClick()
    private fun selectTwo() {
        start()
        val row=timelineRowHeightDp(InstrumentationRegistry.getInstrumentation().targetContext.resources.configuration.fontScale)*density
        compose.onNodeWithTag("timeline").performTouchInput{click(Offset(90*density,44*density+2.5f*row))}
        assertEquals(setOf(3L,4L),vm.selectedLayerIds)
    }
    private fun undo(before:String) {
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.state.saved&&vm.state.project!!.toString()==before}
    }
    private fun photo(name:String) {
        compose.waitForIdle();InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        android.os.SystemClock.sleep(150)
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"$name.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
    }
    @Test fun rowTapsToggleLayersAndExcludeCameraAndExitWithoutOpeningProperties() {
        selectTwo();photo("layer-multiselect")
        compose.onNodeWithTag("selected-layer-count").assertTextEquals("已选 2")
        compose.onNodeWithTag("timeline").performTouchInput{click(Offset(90*density,62*density))}
        assertEquals(setOf(3L,4L),vm.selectedLayerIds)
        compose.onNodeWithTag("select-all-layers").performClick()
        assertEquals(setOf(2L,3L,4L),vm.selectedLayerIds)
        compose.onNodeWithTag("select-all-layers").performClick()
        assertTrue(vm.selectedLayerIds.isEmpty());photo("layer-empty-selection")
        compose.onAllNodesWithContentDescription("退出多选").onFirst().performClick()
        assertFalse(vm.layerSelectionMode);assertFalse(vm.panelOpen)
    }
    @Test fun batchVisibilityAndLocksUndoTogetherAndLockedSelectionCannotDelete() {
        selectTwo();val before=vm.state.project!!.toString()
        more();compose.onNodeWithTag("selected-visibility").performClick()
        compose.waitUntil(10000){vm.state.saved&&!vm.layer(3)!!.getBoolean("visible")&&!vm.layer(4)!!.getBoolean("visible")}
        assertTrue(vm.layer(2)!!.getBoolean("visible"));undo(before)
        more();compose.onNodeWithTag("selected-lock").performClick()
        compose.waitUntil(10000){vm.state.saved&&vm.layer(3)!!.getBoolean("locked")&&vm.layer(4)!!.getBoolean("locked")}
        more();compose.onNodeWithTag("selected-delete").assertIsNotEnabled();compose.onNodeWithTag("selected-duplicate").assertIsNotEnabled();photo("layer-locked-selection")
        compose.onNodeWithTag("selected-lock").performClick()
        compose.waitUntil(10000){vm.state.saved&&!vm.layer(3)!!.getBoolean("locked")&&!vm.layer(4)!!.getBoolean("locked")}
        assertNull(vm.state.error)
    }
    @Test fun groupClipMovePreservesSpacingRejectsInvalidDeltaAndUndoesOnce() {
        selectTwo();val before=vm.state.project!!.toString()
        scenario.onActivity{assertFalse(vm.moveSelectedClips(41))}
        assertEquals(before,vm.state.project!!.toString())
        more();compose.onNodeWithTag("selected-move").performClick()
        compose.onNodeWithTag("selected-move-offset").performTextReplacement("10")
        compose.onNodeWithTag("confirm-selected-move").performClick()
        compose.waitUntil(10000){vm.state.saved&&vm.timelineLayer(3)!!.getInt("in_frame")==50&&vm.timelineLayer(4)!!.getInt("in_frame")==70}
        assertEquals(20,vm.timelineLayer(4)!!.getInt("in_frame")-vm.timelineLayer(3)!!.getInt("in_frame"))
        photo("layer-batch-move");undo(before);assertNull(vm.state.error)
    }
    @Test fun duplicateAndConfirmedDeleteAreAtomicAndEachUndoRestoresAllLayers() {
        selectTwo();val before=vm.state.project!!.toString()
        more();compose.onNodeWithTag("selected-duplicate").performClick()
        compose.waitUntil(10000){vm.state.saved&&vm.state.project!!.getJSONArray("layers").length()==5}
        undo(before)
        more();compose.onNodeWithTag("selected-delete").performClick();photo("layer-delete-confirmation")
        compose.onNodeWithTag("confirm-selected-delete").performClick()
        compose.waitUntil(10000){vm.state.saved&&vm.state.project!!.getJSONArray("layers").length()==1}
        assertFalse(vm.layerSelectionMode)
        assertTrue(vm.hasCamera());undo(before);assertNull(vm.state.error)
    }
}
