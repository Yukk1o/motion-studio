package com.motionstudio.editor

import android.content.Intent
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
import java.io.File
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class CameraRigTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup(){val app=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(app.filesDir,"acceptance/rig-"+UUID.randomUUID()).apply{mkdirs()}
        scenario=ActivityScenario.launch(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath).putExtra("emptyProject",true))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]};compose.waitUntil(20000){vm.state.project!=null}}
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    @Test fun cameraAndNullRigAreExplicitAndParentControlIsUndoable() {
        assertFalse(vm.hasCamera());assertEquals(0,vm.state.project!!.getJSONArray("layers").length())
        compose.onNodeWithContentDescription("添加图层").performClick();compose.onNodeWithText("摄影机").performClick()
        compose.waitUntil(10000){vm.hasCamera()};assertEquals(0,vm.track()!!.getJSONArray("keys").length())
        compose.onNodeWithContentDescription("关闭属性面板").performClick()
        compose.onNodeWithContentDescription("添加图层").performClick();compose.onNodeWithText("空对象").performClick()
        compose.waitUntil(10000){vm.state.project!!.getJSONArray("layers").length()==1};val parent=vm.selected
        assertEquals("null",vm.layer(parent)!!.getJSONObject("content").getString("kind"))
        scenario.onActivity{vm.select(0)};compose.waitForIdle()
        compose.onNodeWithContentDescription("图层操作").performClick();compose.onNodeWithText("父级").performClick()
        compose.onNodeWithTag("parent-"+parent).performClick()
        compose.onNodeWithTag("parent-apply").performClick()
        compose.waitUntil(10000){vm.state.project!!.getJSONObject("camera").optJSONObject("parent")?.optLong("object")==parent}
        val linked=vm.state.project!!.toString()
        scenario.onActivity{vm.select(parent);vm.setValue(org.json.JSONArray(listOf(640,960,0)))}
        compose.waitUntil(10000){vm.layer(parent)!!.getJSONObject("transform").getJSONObject("position").getJSONArray("value").getDouble(0)==640.0}
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){vm.state.project!!.toString()==linked}
        scenario.onActivity{vm.select(0);vm.deleteLayer()};compose.waitUntil(10000){!vm.hasCamera()}
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){vm.hasCamera()}
        File(root,"rig-ui-report.json").writeText(JSONObject().put("explicitCamera",true).put("nullObject",true).put("parentPicker",true).put("undo",true).toString(2))
    }
}
