package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
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

class TransportLayerActionsTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(context.filesDir,"acceptance/transport-actions-${UUID.randomUUID()}").apply{mkdirs()}
        File(root,"project.json").writeText(LayerClipTimelineTest().fixture().put("name","剪辑练习").toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null&&vm.catalogue!=null}
        scenario.onActivity{vm.select(2,false);vm.seek(40.0)}
        compose.waitUntil(10000){vm.state.sample?.optDouble("frame")==40.0}
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun menu() {
        compose.mainClock.advanceTimeBy(500);compose.waitForIdle()
        compose.onNodeWithContentDescription("更多编辑操作").performClick()
    }
    private fun copy() {
        menu();compose.onNodeWithTag("copy-layers").assertIsEnabled().performClick()
        compose.waitUntil(10000){vm.canPasteLayers()}
    }
    private fun paste(count:Int) {
        val previous=vm.selected
        menu();compose.onNodeWithTag("paste-layers").assertIsEnabled().performClick()
        compose.waitUntil(10000){vm.state.saved&&vm.state.project!!.getJSONArray("layers").length()==count&&vm.selected!=0L&&vm.selected!=previous}
    }
    private fun undo(before:String) {
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.state.saved&&vm.state.project!!.toString()==before}
    }
    private fun photo(name:String) {
        compose.mainClock.advanceTimeBy(500);compose.waitForIdle()
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"$name.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
    }
    @Test fun cuttingFromTransportSelectsTheRightClipAndUndoRestoresTheWholeLayer() {
        val before=vm.state.project!!.toString()
        compose.onNodeWithContentDescription("切割图层").assertIsEnabled().performClick()
        compose.waitUntil(10000){vm.state.saved&&vm.selected==3L&&vm.timelineLayer(3)?.optInt("in_frame")==40}
        assertEquals(40,vm.timelineLayer(2)!!.getInt("out_frame"))
        assertEquals(100,vm.timelineLayer(3)!!.getInt("out_frame"))
        assertEquals(40.0,vm.frame,0.0)
        compose.onNodeWithContentDescription("切割图层").assertIsNotEnabled();photo("transport-split-layer")
        undo(before)
        scenario.onActivity{vm.redo()}
        compose.waitUntil(10000){vm.state.saved&&vm.state.project!!.getJSONArray("layers").length()==2}
        assertNull(vm.state.error)
    }
    @Test fun copyCapturesAnimationEffectsAndExpressionsAndPasteIsOneUndo() {
        scenario.onActivity{vm.openEffects()}
        compose.onNodeWithTag("effects-add").performClick()
        compose.onNodeWithTag("effect-add-brightness_contrast").performScrollTo().performClick()
        compose.waitUntil(10000){vm.state.saved&&vm.layer(2)!!.getJSONArray("effects").length()==1}
        val target=JSONObject().put("kind","property").put("object",2).put("property","position")
        var expressionDone=false
        scenario.onActivity{vm.saveExpression(target,"value",true,7L){error->assertNull(error);expressionDone=true};vm.closeWorkspace()}
        compose.waitUntil(10000){expressionDone&&vm.state.saved}
        val captured=JSONObject(vm.layer(2)!!.toString()).apply{remove("id")}
        val beforeCopy=vm.state.project!!.toString();val canUndo=vm.state.canUndo
        menu();compose.onNodeWithTag("paste-layers").assertIsNotEnabled();photo("transport-copy-paste-menu")
        compose.onNodeWithTag("copy-layers").performClick();compose.waitUntil(10000){vm.canPasteLayers()}
        assertEquals(beforeCopy,vm.state.project!!.toString());assertEquals(canUndo,vm.state.canUndo)
        scenario.onActivity{
            vm.rename("复制后改名")
            vm.edit(JSONObject().put("op","set_vector").put("object",2).put("property","rotation").put("frame",40).put("value",JSONArray(listOf(0,0,15))))
        }
        compose.waitUntil(10000){vm.state.saved&&vm.layer(2)!!.getString("name")=="复制后改名"&&vm.layer(2)!!.getJSONObject("transform").getJSONObject("rotation").getJSONArray("keys").length()==3}
        val beforePaste=vm.state.project!!.toString();paste(2)
        assertEquals(3L,vm.selected)
        assertEquals(captured.toString(),JSONObject(vm.layer(3)!!.toString()).apply{remove("id")}.toString())
        val expressions=vm.state.project!!.getJSONArray("expressions").objects()
        assertEquals(2,expressions.size)
        assertEquals("value",expressions.single{it.getJSONObject("target").getLong("object")==3L}.getString("source"))
        assertEquals(40.0,vm.frame,0.0);photo("transport-pasted-layer")
        undo(beforePaste);assertNull(vm.state.error)
    }
    @Test fun copiedLayerCanBePastedAfterTheSourceIsDeleted() {
        val captured=JSONObject(vm.layer(2)!!.toString()).apply{remove("id")}
        copy();scenario.onActivity{vm.deleteLayer()}
        compose.waitUntil(10000){vm.state.saved&&vm.layer(2)==null}
        assertTrue(vm.canPasteLayers());val beforePaste=vm.state.project!!.toString()
        paste(1)
        assertEquals(captured.toString(),JSONObject(vm.layer(vm.selected)!!.toString()).apply{remove("id")}.toString())
        undo(beforePaste);assertNull(vm.state.error)
    }
    @Test fun multipleCopiedLayersRemapParentsAndKeepStackingOrder() {
        scenario.onActivity{vm.addNull()}
        compose.waitUntil(10000){vm.state.saved&&vm.layer(3)!=null}
        scenario.onActivity{vm.edit(JSONObject().put("op","parent").put("object",2).put("parent",3).put("frame",40));vm.closeWorkspace()}
        compose.waitUntil(10000){vm.state.saved&&vm.layer(2)!!.optJSONObject("parent")?.optLong("object")==3L}
        scenario.onActivity{vm.startLayerSelection();vm.selectAllLayers()}
        copy();val beforePaste=vm.state.project!!.toString();paste(4)
        assertEquals(listOf(2L,3L,4L,5L),vm.state.project!!.getJSONArray("layers").objects().map{it.getLong("id")})
        assertEquals(5L,vm.layer(4)!!.getJSONObject("parent").getLong("object"))
        compose.waitUntil(10000){vm.selectedLayerIds==setOf(4L,5L)}
        assertTrue(vm.layerSelectionMode);photo("transport-pasted-group")
        undo(beforePaste);assertNull(vm.state.error)
    }
    @Test fun copiedTextAndAudioReuseAssetsAndLockedSourcesRemainCopyable() {
        scenario.onActivity{vm.addText("标题")}
        compose.waitUntil(10000){vm.state.saved&&vm.contentKind()=="text"}
        val textId=vm.selected
        scenario.onActivity{vm.flags(textId,true,true);vm.closeWorkspace()}
        compose.waitUntil(10000){vm.state.saved&&vm.layer(textId)!!.getBoolean("locked")}
        val text=vm.layer(textId)!!.getJSONObject("content").toString();copy()
        val beforePaste=vm.state.project!!.toString();paste(3)
        assertEquals(text,vm.layer(vm.selected)!!.getJSONObject("content").toString())
        assertFalse(vm.layer(vm.selected)!!.getBoolean("locked"));assertEquals(1,vm.state.project!!.getJSONArray("assets").length())
        undo(beforePaste)
        scenario.onActivity{vm.importMedia(Uri.parse("content://com.motionstudio.editor.test.audio-fixtures/tone-stereo-48000.wav"),"audio")}
        compose.waitUntil(45000){vm.state.error!=null||(vm.importTask==null&&vm.contentKind()=="audio"&&vm.state.saved)}
        assertNull(vm.state.error);scenario.onActivity{vm.clearMediaNotice();vm.closeWorkspace()}
        val audio=vm.layer(vm.selected)!!.getJSONObject("content").toString();copy();paste(4)
        assertEquals(audio,vm.layer(vm.selected)!!.getJSONObject("content").toString())
        assertEquals(1,vm.state.project!!.getJSONArray("audio_assets").length());assertNull(vm.state.error)
    }
    @Test fun transportButtonsDoNotOverlapAndSplitAndPasteRespectTheirContext() {
        val labels=mutableListOf("撤销","上一帧","播放/暂停","下一帧","切割图层","更多编辑操作")
        if(compose.onAllNodesWithContentDescription("重做").fetchSemanticsNodes().isNotEmpty())labels+="重做"
        val density=InstrumentationRegistry.getInstrumentation().targetContext.resources.displayMetrics.density
        val bounds=labels.map {label->compose.onNodeWithContentDescription(label).fetchSemanticsNode().boundsInRoot.also{
            assertTrue("$label has a small touch target",it.width/density>=44&&it.height/density>=44)
        }}
        bounds.forEachIndexed{i,a->bounds.drop(i+1).forEach{b->assertFalse("transport buttons overlap",a.overlaps(b))}}
        photo("transport-layer-actions")
        for(frame in listOf(20.0,100.0)) {
            scenario.onActivity{vm.seek(frame)}
            compose.onNodeWithContentDescription("切割图层").assertIsNotEnabled()
        }
        scenario.onActivity{vm.seek(40.0);vm.flags(2,true,true)}
        compose.waitUntil(10000){vm.state.saved&&vm.layer(2)!!.getBoolean("locked")}
        compose.onNodeWithContentDescription("切割图层").assertIsNotEnabled();copy()
        val originalRoot=vm.root
        scenario.onActivity{vm.newProject(256,256,30,"其他工程",180)}
        compose.waitUntil(10000){vm.root!=originalRoot&&vm.state.saved}
        menu();compose.onNodeWithTag("paste-layers").assertIsNotEnabled()
        compose.onNodeWithText("在原工程内粘贴").assertIsDisplayed();assertNull(vm.state.error)
    }
}
