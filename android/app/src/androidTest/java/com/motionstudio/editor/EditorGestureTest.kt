package com.motionstudio.editor

import android.app.Application
import androidx.compose.ui.geometry.Offset
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.lifecycle.ViewModelStore
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import kotlin.math.min

@RunWith(AndroidJUnit4::class)
class EditorGestureTest {
    @get:Rule val compose=createComposeRule()
    private lateinit var vm:EditorViewModel
    private val store=ViewModelStore()
    private val visible=mutableStateOf(true)
    @Before fun setup() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as Application
        compose.runOnUiThread {
            vm=EditorViewModel(app,File(app.filesDir,"acceptance/ui-"+UUID.randomUUID()),org.json.JSONObject(NativeBridge.projectTemplate(0)).getJSONObject("data").toString())
            store.put("test",vm)
        }
        compose.setContent {StudioTheme {if(visible.value)Editor(vm)}}
        compose.waitUntil(20000){vm.state.project!=null}
        compose.runOnIdle{vm.select(2)}
        compose.waitForIdle()
    }
    @After fun teardown(){compose.runOnUiThread{store.clear()}}

    @Test fun previewDraggingAfterEditingOtherVectorPropertiesKeepsImageGeometry() {
        for(property in listOf("scale","rotation")) {
            compose.runOnIdle{vm.property=property}
            compose.waitForIdle()
            val before=vm.layer(2)!!
            val transform=before.getJSONObject("transform")
            val point=transform.getJSONObject("position").getJSONArray("value")
            val preview=compose.onNodeWithTag("preview-gesture")
            val size=preview.fetchSemanticsNode().size
            preview.performTouchInput {
                down(center)
                moveBy(Offset(30f,12f),100)
                moveBy(Offset(20f,8f),100)
                up()
            }
            compose.waitUntil(10000){
                vm.layer(2)!!.getJSONObject("transform").getJSONObject("position").getJSONArray("value").getDouble(0)>point.getDouble(0)+1
            }
            compose.waitForIdle()
            val after=vm.layer(2)!!
            val moved=after.getJSONObject("transform").getJSONObject("position").getJSONArray("value")
            val p=vm.state.project!!
            val fit=min(size.width.toDouble()/p.getInt("width"),size.height.toDouble()/p.getInt("height"))
            assertEquals(50.0,(moved.getDouble(0)-point.getDouble(0))*fit,.1)
            assertEquals(20.0,(moved.getDouble(1)-point.getDouble(1))*fit,.1)
            assertEquals(point.getDouble(2),moved.getDouble(2),.001)
            for(channel in listOf("scale","rotation"))assertEquals(transform.getJSONObject(channel).toString(),after.getJSONObject("transform").getJSONObject(channel).toString())
            assertEquals(before.getJSONArray("size").toString(),after.getJSONArray("size").toString())
            assertEquals("position",vm.property)
        }
        assertNull(vm.state.error)
    }

    @Test fun rebuildingPreviewSurfaceKeepsProjectAndFrame() {
        compose.runOnIdle{vm.seek(72.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==72.0}
        val before=vm.state.project!!.toString()
        val revision=vm.state.sample!!.getLong("revision")
        repeat(3) {
            compose.runOnIdle{visible.value=false}
            compose.waitForIdle()
            compose.runOnIdle{visible.value=true}
            compose.waitForIdle()
            compose.runOnIdle{vm.seek(72.0)}
            compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==72.0}
            assertEquals(before,vm.state.project!!.toString())
            assertEquals(revision,vm.state.sample!!.getLong("revision"))
            assertEquals(72.0,vm.frame,.001)
            assertNull(vm.state.error)
        }
        // Request state after the queued render to prove the rebuilt Surface presents.
        compose.runOnIdle{vm.save()}
        compose.waitUntil(10000){(vm.state.sample?.optLong("presented")?:0)>0}
    }
}
