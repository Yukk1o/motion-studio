package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
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
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class EditorUiRefreshTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File

    @Before fun setup() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(app.filesDir,"acceptance/ui-refresh-"+UUID.randomUUID()).apply{mkdirs()}
        scenario=ActivityScenario.launch(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null}
        scenario.onActivity{vm.select(2,false)}
        compose.waitForIdle()
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}

    private fun photo(name:String) {
        compose.waitForIdle()
        InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        android.os.SystemClock.sleep(250)
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,name+".png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)}
        bitmap.recycle()
    }
    private fun projected(id:Long):String {
        val a=vm.state.sample!!.getJSONArray("projectedLayers")
        return (0 until a.length()).map{a.getJSONObject(it)}.first{it.getLong("id")==id}.getJSONArray("corners").toString()
    }

    @Test fun addingAndBindingRequireExplicitChoiceAndKeepTheCurrentPose() {
        photo("01-main")
        compose.onNodeWithContentDescription("添加图层").performClick()
        compose.onNodeWithTag("add-camera").assertIsNotEnabled()
        photo("02-add-layer")
        compose.onNodeWithTag("add-null").performClick()
        compose.waitUntil(10000){vm.layer(vm.selected)?.optJSONObject("content")?.optString("kind")=="null"}
        val parent=vm.selected
        scenario.onActivity{vm.rename("运动控制器");vm.select(2)}
        compose.waitUntil(10000){vm.layer(parent)?.optString("name")=="运动控制器"}
        compose.waitForIdle()
        photo("03-properties")
        val before=vm.state.project!!.toString()
        val pose=projected(2)
        compose.onNodeWithTag("open-parent").performClick()
        compose.onNodeWithTag("parent-2").assertDoesNotExist()
        compose.onNodeWithTag("parent-search").performTextReplacement("运动")
        compose.onNodeWithTag("parent-"+parent).assertIsDisplayed().performClick()
        assertEquals("Choosing a row must not mutate the project",before,vm.state.project!!.toString())
        photo("04-parent-picker")
        compose.onNodeWithTag("parent-apply").performClick()
        compose.waitUntil(10000){parentOf(vm,2)==parent&&vm.state.saved}
        assertEquals("Binding must preserve the current projected pose",pose,projected(2))
        compose.onNodeWithTag("open-parent").assertTextContains("父级 · 运动控制器")
        photo("05-parent-bound")
        val bound=vm.state.project!!.toString()
        compose.onNodeWithTag("open-parent").performClick()
        compose.onNodeWithTag("parent-none").performClick()
        compose.onNodeWithContentDescription("关闭父子级绑定").performClick()
        assertEquals("Dismiss must leave the existing binding intact",bound,vm.state.project!!.toString())
        scenario.onActivity{vm.select(parent)}
        compose.waitForIdle()
        compose.onNodeWithTag("open-parent").performClick()
        compose.onNodeWithTag("parent-2").assertDoesNotExist()
        compose.onNodeWithContentDescription("关闭父子级绑定").performClick()
        scenario.onActivity{vm.select(2)}
        compose.waitForIdle()
        compose.onNodeWithTag("open-parent").performClick()
        compose.onNodeWithTag("parent-none").performClick()
        compose.onNodeWithTag("parent-apply").performClick()
        compose.waitUntil(10000){parentOf(vm,2)==null&&vm.state.saved}
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.state.project!!.toString()==bound}
        // Adding from a scalar/camera property always opens usable position fields.
        scenario.onActivity{vm.openProperty("opacity");vm.panelOpen=false}
        compose.waitForIdle()
        compose.onNodeWithContentDescription("添加图层").performClick()
        compose.onNodeWithTag("add-solid").performClick()
        compose.waitUntil(10000){vm.layer(vm.selected)?.optString("name")=="矩形"}
        assertEquals("position",vm.property)
        compose.onNodeWithTag("value-X").assertIsDisplayed()
        assertNull(vm.state.error)
        File(root,"binding-report.json").writeText(JSONObject().put("stagedSelection",true).put("search",true)
            .put("selfAndCycleExcluded",true).put("preservedPose",true).put("cancelPreservesBinding",true)
            .put("unbindUndo",true).put("newLayerResetsProperty",true).toString(2))
    }

    @Test fun curvesExpandWithoutMovingThePreviewAndGuideEmptyTracks() {
        scenario.onActivity{vm.select(2)}
        compose.waitForIdle()
        val preview=compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot
        val transport=compose.onNodeWithTag("transport").fetchSemanticsNode().boundsInRoot
        val normalPanel=compose.onNodeWithTag("properties-panel").fetchSemanticsNode().boundsInRoot.height
        compose.onNodeWithContentDescription("缓动曲线").performClick()
        compose.waitUntil(10000){compose.onNodeWithTag("properties-panel").fetchSemanticsNode().boundsInRoot.height>normalPanel}
        compose.onNodeWithText("先为此属性添加两个关键帧").assertIsDisplayed()
        photo("06-curve-empty")
        compose.onNodeWithContentDescription("返回变换参数").performClick()
        compose.onNodeWithTag("value-X").assertIsDisplayed()
        scenario.onActivity{vm.addKey();vm.seek(60.0)}
        compose.waitUntil(10000){vm.frame==60.0&&vm.keys().size==1}
        scenario.onActivity{vm.setValue(JSONArray(listOf(820,960,0)))}
        compose.waitUntil(10000){vm.keys().size==2}
        scenario.onActivity{vm.seek(30.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==30.0}
        compose.onNodeWithContentDescription("缓动曲线").performClick()
        compose.onNodeWithTag("curve-kind-cubic").performScrollTo().performClick()
        compose.waitUntil(10000){vm.easingDefinition()?.optJSONObject("curve")?.optJSONObject("shape")?.optString("kind")=="cubic"}
        assertEquals(preview,compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot)
        assertEquals(transport,compose.onNodeWithTag("transport").fetchSemanticsNode().boundsInRoot)
        photo("07-curve-cubic")
        val graphHeight=compose.onNodeWithTag("easing-graph").fetchSemanticsNode().boundsInRoot.height
        assertTrue("Expanded graph needs a usable dragging area",graphHeight>normalPanel*.35f)
        compose.onNodeWithTag("curve-view-velocity").performClick()
        photo("08-curve-velocity")
        compose.onNodeWithContentDescription("返回变换参数").performClick()
        compose.onNodeWithTag("value-X").assertIsDisplayed()
        assertEquals(30.0,vm.frame,.001)
        assertNull(vm.state.error)
        File(root,"curve-layout-report.json").writeText(JSONObject().put("normalPanelPixels",normalPanel)
            .put("graphPixels",graphHeight).put("previewAndTransportStable",true).put("emptyState",true).toString(2))
    }
}
