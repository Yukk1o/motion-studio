package com.motionstudio.editor

import android.content.Intent
import android.content.Context
import android.content.pm.ActivityInfo
import android.graphics.Bitmap
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.*
import androidx.compose.ui.semantics.SemanticsActions
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
import kotlin.math.abs

class ColorEditingTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    private var curveParam=""
    private var paletteBackup:Map<String,String?> = emptyMap()
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val prefs=context.getSharedPreferences("motion-color-palette",Context.MODE_PRIVATE)
        paletteBackup=listOf("favorites","common").associateWith{prefs.getString(it,null)}
        assertTrue(prefs.edit().clear().commit())
        root=File(context.filesDir,"acceptance/color-ui-${UUID.randomUUID()}").apply{mkdirs()}
        val p=nativeData(NativeBridge.projectTemplate(0)).put("width",1080).put("height",1920).put("frames",60)
        p.getJSONObject("camera").put("created",false)
        val first=p.getJSONArray("layers").getJSONObject(0).put("id",1).put("parent",JSONObject.NULL)
        first.put("size",JSONArray(listOf(1080,1920))).put("three_d",false)
        first.put("content",JSONObject().put("kind","solid").put("color",JSONArray(listOf(.31,.61,.81,1))))
        first.getJSONObject("transform").getJSONObject("position").put("value",JSONArray(listOf(540,960,0))).put("keys",JSONArray())
        p.put("layers",JSONArray().put(first))
        val native=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),native>0)
        try {
            val packages=nativeData(NativeBridge.plugin(native,"{\"op\":\"catalogue\"}")).getJSONArray("packages").objects()
            val pkg=packages.first{it.getJSONObject("manifest").getJSONArray("effects").objects().any{e->e.getString("id")=="curves"}}
            val manifest=pkg.getJSONObject("manifest")
            nativeData(NativeBridge.plugin(native,JSONObject().put("op","add").put("object",1).put("plugin",manifest.getString("id"))
                .put("version",manifest.getString("version")).put("hash",pkg.getString("hash")).put("effect","curves").toString()))
            val effect=nativeData(NativeBridge.state(native)).getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONArray("effects").getJSONObject(0)
            curveParam=effect.getJSONObject("params").keys().asSequence().first{effect.getJSONObject("params").getJSONObject(it).getString("kind")=="curve"}
            nativeData(NativeBridge.save(native))
        } finally {NativeBridge.destroy(native)}
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        settled{vm.state.project!=null};scenario.onActivity{vm.select(1)};settled{vm.selected==1L}
    }
    @After fun teardown(){
        if(::scenario.isInitialized)scenario.close()
        val prefs=InstrumentationRegistry.getInstrumentation().targetContext.getSharedPreferences("motion-color-palette",Context.MODE_PRIVATE)
        val edit=prefs.edit().clear();paletteBackup.forEach{(key,value)->if(value!=null)edit.putString(key,value)};assertTrue(edit.commit())
    }
    private fun settled(ready:()->Boolean){
        try {compose.waitUntil(20000){vm.state.error!=null||ready()};assertNull(vm.state.error);assertTrue(ready())}
        catch(e:Throwable){
            File(root,"ui-failure-state.json").writeText(JSONObject().put("project",vm.state.project).put("sample",vm.state.sample)
                .put("error",vm.state.error).put("eyedropper",vm.eyedropperActive).put("colorEditor",vm.colorEditor?.value?.array())
                .put("colorStarted",vm.colorEditor?.started).toString())
            runCatching{photo("ui-failure")};throw e
        }
    }
    private fun color()=vm.layer(1)!!.getJSONObject("content").getJSONArray("color")
    private fun photo(name:String){compose.waitForIdle();val b=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot();File(root,"$name.png").outputStream().use{b.compress(Bitmap.CompressFormat.PNG,100,it)};b.recycle()}
    private fun chooseChannel(index:Int) {
        val button=compose.onNodeWithTag("color-curve-channel-$index")
        // There are independent horizontal tabs and a vertical parameter dock.
        // Scroll each viewport explicitly so a tap cannot hit the fixed header.
        repeat(3) {
            val dock=compose.onNodeWithTag("effect-parameters")
            val bounds=dock.fetchSemanticsNode().boundsInRoot
            val target=button.fetchSemanticsNode().boundsInRoot
            dock.performSemanticsAction(SemanticsActions.ScrollBy){it(0f,target.top-bounds.top)}
            val tabs=compose.onNodeWithTag("color-curve-tabs")
            val row=tabs.fetchSemanticsNode().boundsInRoot
            val item=button.fetchSemanticsNode().boundsInRoot
            val dx=if(item.right>row.right)item.right-row.right else if(item.left<row.left)item.left-row.left else 0f
            tabs.performSemanticsAction(SemanticsActions.ScrollBy){it(dx,0f)}
            compose.waitForIdle()
        }
        button.assertIsDisplayed().performClick();compose.waitForIdle();button.assertIsSelected()
    }

    @Test fun unifiedColorPreviewsCancelRestoresAndConfirmHasOneUndo() {
        val original=color().toString()
        val preview=compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot
        val timeline=compose.onNodeWithTag("timeline").fetchSemanticsNode().boundsInRoot
        compose.onNodeWithTag("source-color-palette").performScrollTo().performClick()
        compose.onNodeWithTag("color-palette").assertIsDisplayed()
        compose.onNodeWithTag("preview-gesture").assertIsDisplayed()
        assertEquals(preview,compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot)
        assertEquals(timeline,compose.onNodeWithTag("timeline").fetchSemanticsNode().boundsInRoot)
        compose.onNodeWithTag("color-hue").fetchSemanticsNode().boundsInRoot.let{assertTrue(it.width>it.height)}
        compose.onNodeWithTag("color-code").performClick()
        compose.onNodeWithTag("color-hex").performScrollTo().performTextReplacement("#FFD600")
        settled{abs(color().getDouble(1)-214.0/255)<.001}
        compose.onNodeWithTag("color-component-3").performScrollTo().performTextReplacement("50")
        settled{abs(color().getDouble(3)-.5)<.001}
        compose.onNodeWithTag("color-values-back").performClick()
        compose.onNodeWithTag("color-sv").assertIsDisplayed();photo("color-palette-portrait")
        compose.onNodeWithTag("color-cancel").performClick();settled{color().toString()==original}
        compose.onNodeWithTag("source-color-palette").performScrollTo().performClick()
        compose.onNodeWithTag("color-code").performClick()
        compose.onNodeWithTag("color-hex").performScrollTo().performTextReplacement("#00AABB80")
        compose.onNodeWithTag("color-confirm").performClick();settled{vm.state.saved&&abs(color().getDouble(3)-128.0/255)<.001}
        scenario.onActivity{vm.undo()};settled{color().toString()==original}
        scenario.onActivity{vm.redo()};settled{abs(color().getDouble(2)-187.0/255)<.001}
    }
    @Test fun playbackAndAnotherGestureCommitColorWithoutNestingHistory() {
        val original=color().toString()
        compose.onNodeWithTag("source-color-palette").performScrollTo().performClick()
        scenario.onActivity{vm.previewColor(Rgba(.8,.2,.1,1.0))}
        settled{abs(color().getDouble(0)-.8)<.001}
        scenario.onActivity{vm.togglePlay();vm.pause()}
        settled{vm.colorEditor==null&&vm.state.saved}
        scenario.onActivity{vm.undo()};settled{color().toString()==original}
        compose.onNodeWithTag("source-color-palette").performScrollTo().performClick()
        scenario.onActivity{vm.previewColor(Rgba(.7,.3,.2,1.0))}
        settled{abs(color().getDouble(0)-.7)<.001}
        scenario.onActivity{vm.beginGesture();vm.setPropertyValue(1,"opacity",0,.75,false);vm.endGesture()}
        settled{vm.colorEditor==null&&vm.state.saved&&vm.layer(1)!!.getJSONObject("transform").getJSONObject("opacity").getDouble("value")==.75}
        scenario.onActivity{vm.undo()};settled{vm.layer(1)!!.getJSONObject("transform").getJSONObject("opacity").getDouble("value")==1.0}
        assertEquals(.7,color().getDouble(0),.001)
        scenario.onActivity{vm.undo()};settled{color().toString()==original}
    }
    @Test fun backgroundPaletteRestoresClosedWorkspaceAndAddLayerButton() {
        scenario.onActivity{vm.select(0,false);vm.closeWorkspace()}
        compose.onNodeWithTag("add-layer").assertIsDisplayed()
        compose.onNodeWithContentDescription("合成设置").performClick()
        compose.onNodeWithTag("background-color-palette").performScrollTo().performClick()
        compose.onNodeWithTag("color-palette").assertIsDisplayed()
        compose.onNodeWithTag("color-cancel").performClick()
        settled{vm.colorEditor==null&&!vm.panelOpen}
        compose.onNodeWithTag("add-layer").assertIsDisplayed()
    }
    @Test fun paletteCanSaveFavoriteAndConfigurePersistentCommonSwatches() {
        val store=ColorBookmarks(InstrumentationRegistry.getInstrumentation().targetContext)
        compose.onNodeWithTag("source-color-palette").performScrollTo().performClick()
        compose.onNodeWithTag("color-palette-favorite").performClick()
        compose.onNode(hasSetTextAction() and hasAnyAncestor(isDialog())).performTextReplacement("轨迹蓝")
        compose.onNodeWithText("确定").performClick()
        compose.waitUntil(5000){store.list().size==1}
        val favorite=store.list().single();val pin="favorite:${favorite.id}"
        assertEquals(Rgba.from(color()),favorite.color)
        compose.onNodeWithTag("color-quick-back").performClick()
        compose.onNodeWithTag("color-common-manage").performClick()
        compose.onNodeWithTag("color-bookmark-${favorite.id}").performScrollTo().performClick()
        compose.onNodeWithTag("color-preset-FFD600").performScrollTo().performClick()
        compose.onNodeWithTag("color-preset-9C7CF4").performScrollTo().performClick()
        val before=store.commonIds().indexOf(pin)
        compose.onNodeWithTag("color-common-up-$pin").performScrollTo().performClick()
        assertEquals(before-1,store.commonIds().indexOf(pin))
        compose.onNodeWithTag("color-common-remove-builtin:#FFFFFF").performScrollTo().performClick()
        photo("color-common-settings")
        compose.onNodeWithTag("color-confirm").performClick()
        compose.onNodeWithTag("source-color").performScrollTo().performClick()
        compose.onNodeWithTag("source-color-favorites").assertDoesNotExist()
        compose.onNodeWithTag("color-selection-panel").assertIsDisplayed()
        compose.onNodeWithTag("color-common-colors").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("color-bookmark-${favorite.id}").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("color-palette").assertDoesNotExist()
        compose.onNodeWithTag("color-confirm").performClick()
        val expected=store.commonIds()
        assertFalse("builtin:#FFD600" in expected);assertTrue("builtin:#9C7CF4" in expected)
        assertTrue(InstrumentationRegistry.getInstrumentation().targetContext.getSharedPreferences("motion-color-palette",Context.MODE_PRIVATE).edit().commit())
        scenario.recreate();compose.waitForIdle()
        assertEquals(expected,ColorBookmarks(InstrumentationRegistry.getInstrumentation().targetContext).commonIds())
        compose.onNodeWithTag("source-color").performScrollTo().performClick()
        compose.onNodeWithTag("color-selection-panel").assertIsDisplayed()
        compose.onNodeWithTag("color-palette").assertDoesNotExist()
        compose.onNodeWithTag("color-common-favorite:${favorite.id}").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("color-common-builtin:#FFD600").assertDoesNotExist()
        photo("color-common-row")
    }
    @Test fun eyedropperSamplesCompositionWhileKeepingAlphaAndWheelWorksInLandscape() {
        // Make a translucent target and a distinct opaque background to sample.
        scenario.onActivity{
            vm.edit(JSONObject().put("op","set_color").put("object",1).put("value",JSONArray(listOf(.3,.6,.8,.37))))
            vm.edit(JSONObject().put("op","set_color").put("object",0).put("value",JSONArray(listOf(.9,.2,.1,1))))
        }
        settled{abs(color().getDouble(3)-.37)<.001&&vm.state.saved}
        compose.onNodeWithTag("source-color-eyedropper").performScrollTo().performClick()
        compose.onNodeWithTag("color-selection-panel").assertDoesNotExist()
        compose.onNodeWithTag("color-palette").assertDoesNotExist()
        // Wait for the displayed translucent composition, including the dock's
        // Surface resize. Match the real displayed pixel rather than a formula.
        var displayed=0
        compose.waitUntil(10000) {
            val bounds=compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot
            val shot=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
            displayed=shot.getPixel(bounds.center.x.toInt(),bounds.center.y.toInt());shot.recycle()
            (displayed shr 16 and 255)>140
        }
        compose.onNodeWithTag("preview-gesture").performTouchInput{click(center)}
        settled{!vm.eyedropperActive&&vm.colorEditor==null&&(0..2).all{i->
            abs(color().getDouble(i)-(displayed shr (16-i*8) and 255)/255.0)<=3.0/255
        }}
        assertEquals(.37,color().getDouble(3),.0001)
        for(i in 0..2)assertEquals((displayed shr (16-i*8) and 255)/255.0,color().getDouble(i),3.0/255)
        scenario.onActivity{vm.undo()};settled{abs(color().getDouble(0)-.3)<.001}
        scenario.onActivity{it.requestedOrientation=ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE}
        compose.waitUntil(10000){InstrumentationRegistry.getInstrumentation().targetContext.resources.configuration.orientation==android.content.res.Configuration.ORIENTATION_LANDSCAPE}
        compose.onNodeWithTag("source-color-palette").performScrollTo().performClick()
        compose.onNodeWithTag("color-mode-wheel").performClick()
        compose.onNodeWithTag("color-hue-wheel").performScrollTo().performTouchInput{click(Offset(width*.8f,height*.5f))}
        compose.onNodeWithTag("preview-gesture").assertIsDisplayed()
        compose.onNodeWithTag("timeline").assertIsDisplayed()
        photo("color-palette-landscape")
        compose.onNodeWithTag("color-cancel").performClick()
        scenario.onActivity{it.requestedOrientation=ActivityInfo.SCREEN_ORIENTATION_PORTRAIT}
    }
    @Test fun rgbOverviewContainsAlphaAndDraggingAlphaLeavesRgbAlone() {
        scenario.onActivity{vm.openEffects()};settled{vm.effectsOpen&&vm.catalogue!=null}
        compose.onNodeWithTag("effect-instance-1").onChildAt(0).performClick()
        compose.onNodeWithTag("effect-color-curve").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("effect-color-curve").assertContentDescriptionContains("RGB 总览",substring=true)
        chooseChannel(4)
        compose.onNodeWithTag("effect-color-curve").assertContentDescriptionContains("Alpha 曲线")
        val original=vm.effectParam(1,1,curveParam)!!.getJSONObject("curve").getJSONObject("value")
        val rgb=(0..3).map{original.getJSONArray("channels").getJSONArray(it).toString()}
        compose.onNodeWithTag("effect-color-curve").performScrollTo().performTouchInput {
            down(Offset(width-9f,9f));moveTo(Offset(width-9f,height*.55f),100);up()
        }
        settled{vm.effectParam(1,1,curveParam)!!.getJSONObject("curve").getJSONObject("value").getJSONArray("channels").getJSONArray(4).getJSONArray(1).getDouble(1)<.6}
        val after=vm.effectParam(1,1,curveParam)!!.getJSONObject("curve").getJSONObject("value")
        for(i in 0..3)assertEquals(rgb[i],after.getJSONArray("channels").getJSONArray(i).toString())
        photo("alpha-curve")
        scenario.onActivity{vm.undo()};settled{vm.effectParam(1,1,curveParam)!!.getJSONObject("curve").getJSONObject("value").getJSONArray("channels").getJSONArray(4).getJSONArray(1).getDouble(1)==1.0}
        chooseChannel(0)
        compose.onNodeWithTag("effect-color-curve").performScrollTo();photo("rgb-overview")
    }
}
