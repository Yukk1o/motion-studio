package com.motionstudio.editor

import android.app.Application
import android.graphics.Bitmap
import android.view.SurfaceView
import android.view.SurfaceHolder
import android.view.View
import android.view.ViewGroup
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModelStore
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import java.util.concurrent.atomic.AtomicInteger

@RunWith(AndroidJUnit4::class)
class PropertyOverlayTest {
    @get:Rule val compose=createComposeRule()
    private lateinit var vm:EditorViewModel
    private val store=ViewModelStore()
    private val landscape=mutableStateOf(false)
    private lateinit var root:File

    @Before fun setup() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as Application
        root=File(app.filesDir,"acceptance/overlay-"+UUID.randomUUID())
        compose.runOnUiThread {
            vm=EditorViewModel(app,root,org.json.JSONObject(NativeBridge.projectTemplate(0)).getJSONObject("data").toString())
            store.put("overlay",vm)
        }
        compose.setContent {
            StudioTheme {
                Box(Modifier.fillMaxSize(),contentAlignment=Alignment.Center) {
                    Box(if(landscape.value)Modifier.fillMaxWidth().height(300.dp) else Modifier.fillMaxSize()) {Editor(vm)}
                }
            }
        }
        compose.waitUntil(20000){vm.state.project!=null}
        compose.runOnIdle{vm.selected=2;vm.seek(72.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==72.0}
        compose.runOnIdle{vm.save()}
        compose.waitUntil(10000){(vm.state.sample?.optLong("presented")?:0)>0}
        compose.waitForIdle()
    }
    @After fun teardown(){compose.runOnUiThread{store.clear()}}

    private fun previewView():SurfaceView {
        fun find(view:View):SurfaceView? {
            if(view is SurfaceView)return view
            if(view is ViewGroup)for(i in 0 until view.childCount)find(view.getChildAt(i))?.let{return it}
            return null
        }
        return compose.runOnIdle {
            val activity=ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).single()
            find(activity.window.decorView)!!
        }
    }
    private fun snapshot(name:String) {
        val image=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,name+".png").outputStream().use{image.compress(Bitmap.CompressFormat.PNG,100,it)}
        image.recycle()
    }

    private fun verifyStableOverlay(name:String) {
        val tags=listOf("preview-gesture","transport","timeline")
        val bounds=tags.associateWith{compose.onNodeWithTag(it).fetchSemanticsNode().boundsInRoot}
        val surface=previewView()
        val nativeSurface=surface.holder.surface
        val surfaceEvents=AtomicInteger()
        val callback=object:SurfaceHolder.Callback {
            override fun surfaceCreated(holder:SurfaceHolder){surfaceEvents.incrementAndGet()}
            override fun surfaceChanged(holder:SurfaceHolder,format:Int,width:Int,height:Int){surfaceEvents.incrementAndGet()}
            override fun surfaceDestroyed(holder:SurfaceHolder){surfaceEvents.incrementAndGet()}
        }
        compose.runOnIdle{surface.holder.addCallback(callback)}
        val project=vm.state.project!!.toString()
        val scale=vm.timelineScale
        snapshot(name+"-closed")
        compose.onNodeWithTag("footer-transform").performScrollTo().performClick()
        compose.waitForIdle()
        compose.onNodeWithTag("properties-panel").assertIsDisplayed()
        for(tag in tags)assertEquals("Opening properties moved "+tag,bounds[tag],compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot)
        assertSame(surface,previewView())
        assertSame(nativeSurface,surface.holder.surface)
        assertEquals("Opening properties recreated or resized Surface",0,surfaceEvents.get())
        assertEquals(72.0,vm.frame,.001)
        assertEquals(scale,vm.timelineScale)
        snapshot(name+"-open")
        compose.onNodeWithText("缩放").performTouchInput{click()}
        compose.waitUntil(10000){vm.property=="scale"}
        compose.onNodeWithText("位置").performTouchInput{click()}
        compose.waitUntil(10000){vm.property=="position"}
        compose.onNodeWithText("540.0").performTouchInput{click()}
        compose.onNode(hasSetTextAction()).assertIsDisplayed()
        for(tag in tags)assertEquals("Numeric editing moved "+tag,bounds[tag],compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot)
        compose.onNodeWithText("取消").performClick()
        compose.waitForIdle()
        val values=compose.onNodeWithTag("property-values").fetchSemanticsNode().config
        if(values.contains(androidx.compose.ui.semantics.SemanticsActions.ScrollBy))compose.onNodeWithTag("transform-pad").performScrollTo()
        compose.onNodeWithTag("transform-pad").assertIsDisplayed()
        compose.onNodeWithTag("property-key").assertIsDisplayed()
        // Empty header space overlays timeline/preview content. It must consume
        // the drag without moving the playhead or changing the selected object.
        compose.onNodeWithTag("properties-panel").performTouchInput {
            down(Offset(4f,4f));moveBy(Offset(55f,0f),100);up()
        }
        compose.waitForIdle()
        assertEquals(72.0,vm.frame,.001)
        assertEquals(2L,vm.selected)
        assertEquals(project,vm.state.project!!.toString())
        compose.onNodeWithContentDescription("关闭属性面板").performTouchInput{click()}
        compose.waitForIdle()
        compose.onNodeWithTag("properties-panel").assertDoesNotExist()
        for(tag in tags)assertEquals("Closing properties moved "+tag,bounds[tag],compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot)
        assertSame(surface,previewView())
        assertSame(nativeSurface,surface.holder.surface)
        assertEquals("Closing properties recreated or resized Surface",0,surfaceEvents.get())
        assertEquals(project,vm.state.project!!.toString())
        compose.runOnIdle{surface.holder.removeCallback(callback)}
    }
    @Test fun portraitEditingOverlaysTheOriginalEditor(){verifyStableOverlay("portrait")}
    @Test fun landscapeEditingOverlaysTheOriginalEditor() {
        compose.runOnIdle{landscape.value=true}
        compose.waitForIdle()
        verifyStableOverlay("landscape")
    }
    @Test fun systemBackClosesPropertiesAndKeepsEditorOpen() {
        compose.onNodeWithTag("footer-transform").performScrollTo().performClick()
        compose.waitForIdle()
        InstrumentationRegistry.getInstrumentation().uiAutomation.performGlobalAction(android.accessibilityservice.AccessibilityService.GLOBAL_ACTION_BACK)
        compose.waitUntil(10000){!vm.panelOpen}
        compose.waitForIdle()
        compose.onNodeWithTag("properties-panel").assertDoesNotExist()
        compose.onNodeWithTag("preview-gesture").assertIsDisplayed()
        assertEquals(72.0,vm.frame,.001)
    }
}
