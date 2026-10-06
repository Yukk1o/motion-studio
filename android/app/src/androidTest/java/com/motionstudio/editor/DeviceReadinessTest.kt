package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
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
class DeviceReadinessTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(context.filesDir,"acceptance/device-"+UUID.randomUUID()).apply{mkdirs()}.canonicalFile
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null}
        awaitPresented()
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun awaitPresented() {
        compose.waitUntil(15000) {
            scenario.onActivity{vm.refreshDiagnostics()}
            val d=vm.state.sample
            d!=null&&!d.isNull("graphics")&&!d.isNull("lastPresentedFrame")&&d.getDouble("lastPresentedFrame")==vm.frame&&
                d.getLong("lastPresentedRevision")==d.getLong("revision")&&d.getLong("lastPresentedViewRevision")==d.getLong("viewRevision")
        }
        compose.waitForIdle();assertNull(vm.state.error)
    }
    private fun photo(name:String) {
        compose.waitForIdle();val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,name+".png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
    }
    private fun clickMenu(name:String) {
        compose.onNodeWithContentDescription("图层操作").performClick();compose.onNodeWithText(name).performClick()
    }
    @Test fun a11RealGpuDeviceLossAndValidationErrorRecoverThroughTheUi() {
        assertTrue("Build acceptance with --diagnostics",vm.state.sample!!.getBoolean("diagnosticsEnabled"))
        scenario.onActivity{vm.seek(72.0);vm.save()};awaitPresented()
        val project=vm.state.project!!.toString();val persisted=File(root,"project.json").readBytes()
        val initialEpoch=vm.state.sample!!.getLong("surfaceEpoch")
        val diagnostics=ArrayList<JSONObject>()
        for(kind in listOf(0,1,0)) {
            scenario.onActivity{vm.injectGraphicsFault(kind)}
            compose.waitUntil(15000){vm.state.error!=null&&vm.lastGpuFailure!=null}
            val detail=vm.lastGpuFailure!!
            assertTrue(detail.contains("GPU"));assertEquals(project,vm.state.project!!.toString());assertEquals(72.0,vm.frame,.001)
            assertArrayEquals(persisted,File(root,"project.json").readBytes())
            photo("gpu-fault-"+diagnostics.size)
            compose.onNodeWithText("重试预览").performTouchInput{click()}
            compose.waitUntil(15000){vm.lastGpuFailure==null&&vm.state.error==null}
            awaitPresented();assertEquals(project,vm.state.project!!.toString());assertEquals(72.0,vm.frame,.001)
            diagnostics.add(JSONObject().put("kind",kind).put("error",detail).put("surfaceEpoch",vm.state.sample!!.getLong("surfaceEpoch")))
        }
        assertTrue(vm.state.sample!!.getLong("surfaceEpoch")>=initialEpoch+3)
        photo("gpu-recovered")
        File(root,"gpu-recovery-report.json").writeText(JSONObject().put("realDeviceDestroyAndValidation",true).put("cycles",org.json.JSONArray(diagnostics))
            .put("projectAndSavedFilePreserved",true).put("frame",vm.frame).toString(2))
    }
    @Test fun a10LayoutProfileKeepsControlsTouchableAndNumericInputPrecise() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val metrics=context.resources.displayMetrics;val config=context.resources.configuration
        fun usable(node:SemanticsNodeInteraction) {
            node.assertIsDisplayed()
            val bounds=node.fetchSemanticsNode().boundsInRoot
            assertTrue("Control has no visible size",bounds.width>0&&bounds.height>0)
        }
        listOf("工程列表","合成设置","输出","播放/暂停","上一帧","下一帧").forEach{usable(compose.onNodeWithContentDescription(it))}
        val stableTags=listOf("preview-gesture","transport","timeline")
        val originalBounds=stableTags.associateWith{compose.onNodeWithTag(it).fetchSemanticsNode().boundsInRoot}
        scenario.onActivity{vm.select(2)};compose.waitForIdle()
        stableTags.forEach{assertEquals("Opening properties moved "+it,originalBounds[it],compose.onNodeWithTag(it).fetchSemanticsNode().boundsInRoot)}
        compose.onNodeWithTag("property-tab-position").assertIsSelected()
        // Real touches detect a sidebar intercepting controls even when their
        // semantics nodes still claim to be displayed beneath the overlay.
        compose.onNodeWithContentDescription("下一帧").performTouchInput{click()}
        compose.waitUntil(5000){vm.frame==1.0}
        compose.onNodeWithContentDescription("上一帧").performTouchInput{click()}
        compose.waitUntil(5000){vm.frame==0.0}
        usable(compose.onNodeWithContentDescription("关闭属性面板"));usable(compose.onNodeWithTag("property-key"))
        photo("layout-before-input")
        for(axis in listOf("X","Y","Z")) {
            val number=compose.onNodeWithTag("number-"+axis,useUnmergedTree=true)
            usable(number)
            val layouts=ArrayList<androidx.compose.ui.text.TextLayoutResult>()
            number.performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.GetTextLayoutResult){it(layouts)}
            val layout=layouts.single()
            File(root,"numeric-layout-"+axis+".json").writeText(JSONObject().put("width",layout.size.width).put("height",layout.size.height)
                .put("lineHeight",layout.getLineBottom(0)-layout.getLineTop(0)).put("lineWidth",layout.getLineRight(0)-layout.getLineLeft(0))
                .put("overflowHeight",layout.didOverflowHeight).put("overflowWidth",layout.didOverflowWidth).put("ellipsis",layout.isLineEllipsized(0)).toString(2))
            // Paragraph width can include fractional rounding beyond its integer
            // size. Check actual glyph bounds, ellipsis and height instead.
            assertTrue("Numeric text is clipped on axis "+axis,!layout.didOverflowHeight&&!layout.isLineEllipsized(0)&&
                layout.getLineRight(0)<=layout.size.width+1)
            val field=compose.onNodeWithTag("value-"+axis).fetchSemanticsNode().boundsInRoot
            val text=number.fetchSemanticsNode().boundsInRoot
            assertTrue("Numeric text extends outside its field",text.top>=field.top&&text.bottom<=field.bottom)
        }
        if(config.fontScale>1.3f) {
            compose.onNodeWithTag("transform-pad").performScrollTo().assertIsDisplayed()
            val pad=compose.onNodeWithTag("transform-pad").fetchSemanticsNode().boundsInRoot
            assertTrue("Large-font transform area must remain usable after scrolling",pad.height>=48*metrics.density)
            photo("layout-scrolled-transform")
            compose.onNodeWithTag("value-X").performScrollTo()
        }
        val project=vm.state.project!!.toString()
        compose.onNodeWithTag("value-X").performTouchInput{click()}
        compose.onNode(hasSetTextAction()).performTextReplacement("640.25")
        compose.onNode(hasSetTextAction()).performImeAction()
        compose.waitUntil(10000){vm.layer(2)!!.getJSONObject("transform").getJSONObject("position").getJSONArray("value").getDouble(0)==640.25}
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){vm.state.project!!.toString()==project}
        photo("layout-profile")
        compose.onNodeWithContentDescription("关闭属性面板").performTouchInput{click()};compose.waitForIdle()
        stableTags.forEach{assertEquals("Closing properties moved "+it,originalBounds[it],compose.onNodeWithTag(it).fetchSemanticsNode().boundsInRoot)}
        usable(compose.onNodeWithContentDescription("添加图层"));usable(compose.onNodeWithTag("footer-transform"))
        compose.onNodeWithContentDescription("工程列表").performTouchInput{click()}
        compose.onNodeWithText("打开工程").assertIsDisplayed()
        compose.onNodeWithText("关闭").performClick()
        compose.onNodeWithContentDescription("合成设置").performTouchInput{click()}
        compose.onNodeWithTag("new-1080-1920-30").performScrollTo();usable(compose.onNodeWithTag("new-1080-1920-30"))
        compose.onNodeWithTag("new-1080-1080-60").performScrollTo();usable(compose.onNodeWithTag("new-1080-1080-60"))
        compose.onNodeWithText("完成").performClick()
        File(root,"layout-profile-report.json").writeText(JSONObject().put("widthPixels",metrics.widthPixels).put("heightPixels",metrics.heightPixels)
            .put("densityDpi",metrics.densityDpi).put("screenWidthDp",config.screenWidthDp).put("screenHeightDp",config.screenHeightDp).put("fontScale",config.fontScale)
            .put("preciseInput",640.25).put("undoExact",true).toString(2))
    }
    @Test fun p0LayerRenameDuplicateHideAndDeleteAreUndoableFromTheUi() {
        scenario.onActivity{vm.select(2)};compose.waitForIdle()
        val before=vm.state.project!!.toString()
        clickMenu("重命名")
        compose.onNode(hasSetTextAction()).performTextReplacement("验收主体")
        compose.onNodeWithText("确定").performClick()
        compose.waitUntil(10000){vm.layer(2)!!.getString("name")=="验收主体"}
        clickMenu("创建副本")
        compose.waitUntil(10000){vm.state.project!!.getJSONArray("layers").length()==4}
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){vm.state.project!!.getJSONArray("layers").length()==3}
        compose.onNodeWithContentDescription("关闭属性面板").performClick();compose.waitForIdle()
        val density=InstrumentationRegistry.getInstrumentation().targetContext.resources.displayMetrics.density
        compose.onNodeWithTag("timeline").performTouchInput{click(androidx.compose.ui.geometry.Offset(23*density,44*density+2*52*density+25*density))}
        compose.waitUntil(10000){!vm.layer(2)!!.getBoolean("visible")}
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){vm.layer(2)!!.getBoolean("visible")}
        scenario.onActivity{vm.select(2)};compose.waitForIdle();clickMenu("删除图层")
        compose.waitUntil(10000){vm.layer(2)==null};assertEquals(0L,vm.selected)
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){vm.layer(2)!=null}
        scenario.onActivity{vm.undo()};compose.waitUntil(10000){vm.state.project!!.toString()==before}
        File(root,"layer-management-report.json").writeText(JSONObject().put("renameDuplicateHideDelete",true).put("undoRestoresOriginalProject",true).toString(2))
    }
}
