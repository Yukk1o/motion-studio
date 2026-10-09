package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
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

class CompositionFrontendTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(context.filesDir,"acceptance/composition-ui-${UUID.randomUUID()}").apply{mkdirs()}
        val project=nativeData(NativeBridge.projectTemplate(0)).put("width",512).put("height",512).put("frames",120).put("layers",JSONArray())
        project.getJSONObject("camera").put("created",false)
        File(root,"project.json").writeText(project.toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]};settled{vm.state.project!=null}
        scenario.onActivity{vm.addRectangle()};settled{vm.state.saved&&vm.layer(1)!=null}
        scenario.onActivity{vm.addRectangle();vm.closeWorkspace()};settled{vm.state.saved&&vm.layer(2)!=null}
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun settled(predicate:()->Boolean){compose.waitUntil(20000){vm.state.error!=null||predicate()};assertNull(vm.state.error);assertTrue(predicate())}
    private fun photo(name:String,preview:Boolean=true) {
        compose.waitForIdle();val before=vm.state.sample?.optLong("presented")?:0
        if(preview){
            scenario.onActivity{vm.refreshDiagnostics(repaint=true)}
            compose.waitUntil(15000){vm.refreshDiagnostics();(vm.state.sample?.optLong("presented")?:0)>before&&vm.state.sample?.optLong("lastPresentedRevision")==vm.state.sample?.optLong("revision")&&vm.state.sample?.optDouble("lastPresentedFrame")==vm.frame}
        }
        compose.waitForIdle();android.os.SystemClock.sleep(150)
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"$name.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
    }
    private fun group():String {
        scenario.onActivity{vm.startLayerSelection();vm.selectAllLayers()}
        compose.onNodeWithContentDescription("批量图层操作").performClick();compose.onNodeWithTag("selected-precompose").performClick()
        compose.onNode(hasSetTextAction()).performTextReplacement("片头")
        compose.onNodeWithText("确定").performClick()
        settled{vm.state.saved&&vm.contentKind()=="composition"}
        return vm.layer(vm.selected)!!.getJSONObject("content").getJSONObject("clip").getString("composition")
    }
    @Test fun precomposeEntryAndBreadcrumbRestoreTimelineSelectionAndScopedUndo() {
        val child=group();assertEquals(1,vm.state.project!!.getJSONArray("layers").length());val reference=vm.selected;photo("precompose-parent")
        scenario.onActivity{vm.seek(37.0);vm.timelineScale=3f}
        settled{vm.state.sample?.optDouble("frame")==37.0}
        compose.onNodeWithTag("footer-composition").performScrollTo().performClick();settled{vm.compositionId==child}
        assertEquals(2,vm.state.project!!.getJSONArray("layers").length());assertEquals(listOf("comp-main",child),vm.compositionPath)
        scenario.onActivity{vm.select(1,false);vm.openProperty("position");vm.setValue(JSONArray(listOf(100,180,0)))}
        settled{vm.state.saved&&vm.layer(1)?.getJSONObject("transform")?.getJSONObject("position")?.getJSONArray("value")?.optInt(0)==100}
        scenario.onActivity{vm.undo()};settled{vm.layer(1)?.getJSONObject("transform")?.getJSONObject("position")?.getJSONArray("value")?.optInt(0)==256}
        scenario.onActivity{vm.closeWorkspace();vm.seek(15.0);vm.timelineScale=2f};settled{vm.state.sample?.optDouble("frame")==15.0};photo("child-editor")
        compose.onNodeWithTag("composition-breadcrumb-0").performClick();settled{vm.compositionId=="comp-main"}
        assertEquals(reference,vm.selected);assertEquals(37.0,vm.frame,0.0);assertEquals(3f,vm.timelineScale)
        compose.onNodeWithTag("footer-composition").performScrollTo().performClick();settled{vm.compositionId==child}
        assertEquals(1L,vm.selected);assertEquals(15.0,vm.frame,0.0);assertEquals(2f,vm.timelineScale)
    }
    @Test fun childVectorEditingAndClipboardCannotAffectAnotherComposition() {
        val child=group();scenario.onActivity{vm.openSelectedComposition()};settled{vm.compositionId==child}
        scenario.onActivity{vm.addVectorShape("star","星形")};settled{vm.state.saved&&vm.contentKind()=="vector"}
        scenario.onActivity{vm.openVector();vm.selectVectorTrack("vector:parameter:inner_ratio");vm.setValue(.3)}
        settled{vm.state.saved&&kotlin.math.abs(((vm.vectorValue(vm.selected,vm.property) as? Number)?.toDouble()?:0.0)-.3)<1e-6}
        compose.onNodeWithTag("timeline").assertIsDisplayed();photo("child-vector-properties")
        scenario.onActivity{vm.copyLayers()};settled{vm.canPasteLayers()}
        compose.onNodeWithTag("composition-breadcrumb-0").performClick();settled{vm.compositionId=="comp-main"}
        assertFalse(vm.canPasteLayers());assertEquals(1,vm.state.project!!.getJSONArray("layers").length())
        scenario.onActivity{vm.openSelectedComposition()};settled{vm.compositionId==child};assertTrue(vm.canPasteLayers())
        scenario.onActivity{vm.pasteLayers()};settled{vm.state.saved&&vm.state.project!!.getJSONArray("layers").length()==4}
    }
    @Test fun referenceClipControlsKeepTimelineVisibleAndCommitAnUndoableChange() {
        group();compose.onNodeWithTag("footer-composition-clip").performScrollTo().performClick()
        compose.onNodeWithTag("timeline").assertIsDisplayed();photo("composition-clip")
        compose.onNodeWithTag("composition-clip-value-source_start_frame").performClick()
        compose.onNode(hasSetTextAction()).performTextReplacement("12");compose.onNodeWithText("确定").performClick()
        settled{vm.state.saved&&vm.layer(vm.selected)!!.getJSONObject("content").getJSONObject("clip").optInt("source_start_frame")==12}
        scenario.onActivity{vm.undo()};settled{vm.layer(vm.selected)!!.getJSONObject("content").getJSONObject("clip").optInt("source_start_frame")==0}
    }
    @Test fun settingsRequireImpactPreviewAndExposeHighFpsThroughTheUi() {
        group();scenario.onActivity{vm.openSelectedComposition()};settled{vm.compositionId!="comp-main"}
        compose.onNodeWithContentDescription("合成设置").performClick();compose.onNodeWithTag("edit-composition-settings").performClick()
        compose.onNodeWithTag("composition-settings-fps").performScrollTo().performTextReplacement("144")
        compose.onNodeWithTag("preview-composition-settings").performClick()
        compose.waitUntil(10000){compose.onAllNodesWithTag("apply-composition-settings").fetchSemanticsNodes().isNotEmpty()};photo("composition-settings")
        compose.onNodeWithTag("apply-composition-settings").performClick();settled{vm.state.saved&&vm.state.project!!.getInt("fps")==144}
        assertEquals(576,vm.state.project!!.getInt("frames"))
    }
    @Test fun libraryCreatesHighFpsChildReferencesItAndDeletesOnlyAfterUnlinking() {
        compose.onNodeWithTag("add-layer").performClick();compose.onNodeWithTag("add-composition").performScrollTo().performClick()
        compose.onNodeWithTag("create-child-composition").performClick()
        compose.onNodeWithTag("new-project-name").performTextReplacement("素材合成")
        compose.onNodeWithTag("new-project-custom-fps").performScrollTo().performTextReplacement("59")
        compose.onNodeWithTag("create-project").performClick();settled{vm.state.saved&&vm.state.sample!!.getJSONArray("compositions").length()==2}
        val child=vm.state.sample!!.getJSONArray("compositions").objects().first{!it.optBoolean("main")};val id=child.getString("id")
        assertEquals(59,child.getInt("fps"));photo("composition-library")
        compose.waitUntil(10000){!compose.onNodeWithTag("reference-composition-$id").fetchSemanticsNode().config.contains(androidx.compose.ui.semantics.SemanticsProperties.Disabled)}
        compose.onNodeWithTag("reference-composition-$id").performClick();settled{vm.state.saved&&vm.contentKind()=="composition"}
        scenario.onActivity{vm.deleteLayer();vm.closeWorkspace()};settled{vm.state.saved&&vm.state.project!!.getJSONArray("layers").length()==2}
        compose.onNodeWithTag("add-layer").performClick();compose.onNodeWithTag("add-composition").performScrollTo().performClick()
        compose.onNodeWithTag("delete-composition-$id").performClick()
        compose.waitUntil(10000){compose.onAllNodesWithText("删除空闲合成，可撤销恢复。").fetchSemanticsNodes().isNotEmpty()}
        compose.onNodeWithTag("delete-composition-confirm").performClick();settled{vm.state.saved&&vm.state.sample!!.getJSONArray("compositions").length()==1}
    }
    @Test fun libraryBeyondThirtyTwoReadsHostLimitsAndCreatesAnUndoableChild() {
        val commands=JSONArray()
        repeat(40){n->commands.put(JSONObject().put("op","composition").put("action",
            JSONObject().put("kind","create").put("settings",JSONObject().put("name","Library $n")
                .put("width",512).put("height",512).put("fps",20).put("frames",120))))}
        scenario.onActivity{vm.editBatch(commands)}
        settled{vm.state.saved&&vm.state.sample?.getJSONArray("compositions")?.length()==41}
        val limits=vm.state.sample!!.getJSONObject("capabilities").getJSONObject("composition_api")
        assertEquals(256,limits.getInt("max_compositions"));assertEquals(64,limits.getInt("max_render_instances"))
        compose.onNodeWithTag("add-layer").performClick();compose.onNodeWithTag("add-composition").performScrollTo().performClick()
        compose.onNodeWithTag("create-child-composition").assertIsEnabled().performClick()
        compose.onNodeWithTag("new-project-name").performTextReplacement("Another Library")
        compose.onNodeWithTag("create-project").performClick()
        settled{vm.state.saved&&vm.state.sample?.getJSONArray("compositions")?.length()==42}
        scenario.onActivity{vm.undo()};settled{vm.state.sample?.getJSONArray("compositions")?.length()==41}
        scenario.onActivity{vm.redo()};settled{vm.state.sample?.getJSONArray("compositions")?.length()==42}
    }
    @Test fun homeShowsCachedUpdatePromptAndSettingsExposeReleaseNotes() {
        scenario.close()
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val prefs=context.getSharedPreferences("motion-release-updates",0)
        val notice=ReleaseNotice("0.1.0-preview.100000",100000,"新增子合成编辑\n修复播放预览", "$RELEASE_PAGE/tag/preview",true)
        prefs.edit().putString("cached-preview",notice.json().toString()).putLong("checked-preview",System.currentTimeMillis()).remove("dismissed").commit()
        try {
            scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
            scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]};settled{vm.state.project!=null}
            compose.onNodeWithContentDescription("工程列表").performClick();compose.onNodeWithTag("update-banner").assertIsDisplayed();photo("update-banner",false)
            compose.onNodeWithText("查看更新").performClick();compose.onNodeWithTag("release-notes").assertIsDisplayed();photo("release-notes",false)
            compose.onNodeWithText("关闭").performClick();compose.onNodeWithTag("dismiss-update").performClick();compose.onNodeWithTag("update-banner").assertDoesNotExist()
            compose.onNodeWithTag("home-settings").performClick();compose.onNodeWithTag("settings-updates").performScrollTo().performClick()
            compose.onNodeWithTag("release-notes").assertIsDisplayed()
        }finally{prefs.edit().clear().commit()}
    }
}
