package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.ui.geometry.Offset
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
class RotationControlsTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    private var density=1f

    @Before fun setup() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        density=app.resources.displayMetrics.density
        root=File(app.filesDir,"acceptance/rotation-"+UUID.randomUUID()).apply{mkdirs()}
        scenario=ActivityScenario.launch(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null}
        scenario.onActivity{vm.select(2);vm.openProperty("rotation");vm.setValue(JSONArray(listOf(15,25,35)))}
        compose.waitUntil(10000){angles().getDouble(0)==15.0&&vm.state.saved}
        compose.waitForIdle()
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun angles()=vm.layer(2)!!.getJSONObject("transform").getJSONObject("rotation").getJSONArray("value")
    private fun chooseAxis(name:String) {
        compose.onNodeWithTag("rotation-axis-"+name).performScrollTo().performClick()
        compose.waitForIdle()
    }
    private fun photo(name:String) {
        compose.waitForIdle();InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        android.os.SystemClock.sleep(250)
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,name+".png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)}
        bitmap.recycle()
    }

    @Test fun axisChoiceIsReadOnlyAndEachAxisDragHasOneUndo() {
        val before=vm.state.project!!.toString()
        val preview=compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot
        for((axis,name) in listOf("X","Y","Z").withIndex()) {
            chooseAxis(name)
            assertEquals(axis,vm.rotationAxis)
            assertEquals(before,vm.state.project!!.toString())
            val original=JSONArray(angles().toString())
            compose.onNodeWithTag("rotation-ruler").performTouchInput {
                down(center);moveBy(Offset(32*density,0f),100);moveBy(Offset(16*density,0f),100);up()
            }
            compose.waitUntil(10000){angles().getDouble(axis)>original.getDouble(axis)+5&&vm.state.saved}
            for(other in 0..2)if(other!=axis)assertEquals(original.getDouble(other),angles().getDouble(other),.00001)
            assertEquals(preview,compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot)
            photo("rotation-"+name)
            scenario.onActivity{vm.undo()}
            compose.waitUntil(10000){vm.state.project!!.toString()==before}
        }
        compose.onNodeWithTag("transform-pad").assertDoesNotExist()
        assertNull(vm.state.error)
    }

    @Test fun verticalMotionIsNotAnAngleEditAndCancelledDragRestoresValues() {
        val before=vm.state.project!!.toString()
        compose.onNodeWithTag("rotation-ruler").performTouchInput {
            down(center);moveBy(Offset(0f,25*density),100);up()
        }
        compose.waitForIdle();assertEquals(before,vm.state.project!!.toString())
        compose.onNodeWithTag("rotation-ruler").performTouchInput {
            down(center);moveBy(Offset(50*density,0f),100);cancel()
        }
        compose.waitUntil(10000){vm.state.project!!.toString()==before}
        assertNull(vm.state.error)
    }

    @Test fun preciseInputSelectsItsAxisAndLockedLayersRejectRulerEdits() {
        compose.onNodeWithTag("value-Y").performTouchInput{click()}
        compose.onNode(hasSetTextAction()).performTextReplacement("90.125")
        compose.onNodeWithText("确定").performClick()
        compose.waitUntil(10000){angles().getDouble(1)==90.125&&vm.state.saved}
        assertEquals(1,vm.rotationAxis)
        assertEquals(15.0,angles().getDouble(0),.00001)
        assertEquals(35.0,angles().getDouble(2),.00001)
        photo("rotation-precise-Y")
        scenario.onActivity{vm.flags(2,true,true)}
        compose.waitUntil(10000){vm.layer(2)!!.getBoolean("locked")}
        val before=vm.state.project!!.toString()
        chooseAxis("X")
        compose.onNodeWithTag("rotation-ruler").performTouchInput {
            down(center);moveBy(Offset(60*density,0f),100);up()
        }
        compose.waitForIdle();assertEquals(before,vm.state.project!!.toString())
        assertNull(vm.state.error)
    }

    @Test fun rotationControlsRemainUsableWithNarrowWindowsAndLargeFonts() {
        compose.onNodeWithTag("rotation-axis-menu").assertDoesNotExist()
        compose.onNodeWithTag("rotation-axis-X").performScrollTo().assertIsDisplayed()
        chooseAxis("X")
        val menu=compose.onNodeWithTag("rotation-axis-X").fetchSemanticsNode().boundsInRoot
        assertTrue(menu.height>=48*density-.5f)
        val ruler=compose.onNodeWithTag("rotation-ruler").fetchSemanticsNode().boundsInRoot
        assertTrue(ruler.height>=48*density-.5f)
        chooseAxis("X")
        assertEquals(0,vm.rotationAxis)
        photo("rotation-layout-initial")
        val scale=InstrumentationRegistry.getInstrumentation().targetContext.resources.configuration.fontScale
        val body=compose.onNodeWithTag("property-values").fetchSemanticsNode().config
        if(scale>1.3f||body.contains(androidx.compose.ui.semantics.SemanticsActions.ScrollBy)) {
            compose.onNodeWithTag("value-Z").performScrollTo()
        }
        compose.onNodeWithTag("value-Z").assertIsDisplayed().performTouchInput{click()}
        compose.onNode(hasSetTextAction()).performTextReplacement("-27.75")
        compose.onNodeWithText("确定").performClick()
        compose.waitUntil(10000){angles().getDouble(2)==-27.75}
        assertEquals(2,vm.rotationAxis)
        photo("rotation-layout")
        scenario.onActivity{activity->
            val config=activity.resources.configuration
            val metrics=activity.resources.displayMetrics
            File(root,"layout-profile-report.json").writeText(JSONObject()
                .put("widthPixels",metrics.widthPixels).put("heightPixels",metrics.heightPixels)
                .put("densityDpi",metrics.densityDpi).put("screenWidthDp",config.screenWidthDp)
                .put("screenHeightDp",config.screenHeightDp).put("fontScale",config.fontScale)
                .put("preciseInput",-27.75).put("axis",vm.rotationAxis).toString(2))
        }
    }
}
