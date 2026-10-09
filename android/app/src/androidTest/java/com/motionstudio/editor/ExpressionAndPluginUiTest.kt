package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
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
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

@RunWith(AndroidJUnit4::class)
class ExpressionAndPluginUiTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(app.filesDir,"acceptance/expression-plugin-${UUID.randomUUID()}").apply{mkdirs()}
        scenario=ActivityScenario.launch(Intent(app,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(30000){vm.state.project!=null&&vm.catalogue!=null}
        scenario.onActivity{vm.select(2,false)}
    }
    @After fun teardown() {
        if(::root.isInitialized)runCatching{photo("final")}
        if(::scenario.isInitialized)scenario.close()
    }
    private fun photo(name:String) {
        compose.waitForIdle()
        InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot().also{bitmap->
            File(root,"$name.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
        }
    }
    private fun visibleWorkspace(tag:String) {
        val panel=compose.onNodeWithTag(tag).fetchSemanticsNode().boundsInRoot
        val timeline=compose.onNodeWithTag("timeline").fetchSemanticsNode().boundsInRoot
        val preview=compose.onNodeWithTag("preview-gesture").fetchSemanticsNode().boundsInRoot
        compose.onNodeWithTag("transport").assertIsDisplayed()
        assertTrue(timeline.height>0&&preview.height>0)
        assertFalse("panel covers timeline",panel.overlaps(timeline));assertFalse("panel covers preview",panel.overlaps(preview))
    }
    private fun applySource(source:String) {
        compose.onNodeWithTag("expression-source").performScrollTo().performTextReplacement(source)
        compose.onNodeWithTag("expression-apply").performScrollTo().performClick()
    }
    @Test fun effectToolbarOpensExpressionForTheSelectedColorParameter() {
        val pkg=vm.catalogue!!.getJSONArray("packages").objects().first{it.getJSONObject("manifest").getJSONArray("effects").objects().any{e->e.getString("id")=="tint"}}
        val manifest=pkg.getJSONObject("manifest")
        val param=manifest.getJSONArray("effects").objects().first{it.getString("id")=="tint"}.getJSONArray("params").objects().first{it.getString("kind")=="color"}.getString("id")
        scenario.onActivity{vm.openEffects();vm.pluginOperation(JSONObject().put("op","add").put("object",2).put("plugin",manifest.getString("id")).put("version",manifest.getString("version")).put("hash",pkg.getString("hash")).put("effect","tint"),true)}
        compose.waitUntil(10000){vm.effectParam(2,1,param)!=null&&vm.state.saved}
        compose.onNodeWithTag("effect-open-1").performScrollTo().performClick()
        compose.onNodeWithTag("effect-select-$param").performScrollTo().performClick()
        photo("effect-color-toolbar")
        compose.onNodeWithTag("effect-expression").performClick()
        compose.onNodeWithTag("expression-workspace").assertIsDisplayed()
        assertEquals("effect",vm.expressionTarget!!.getString("kind"))
        assertEquals(param,vm.expressionTarget!!.getString("param"))
        assertEquals(1L,vm.expressionTarget!!.getLong("effect"))
        visibleWorkspace("expression-workspace")
        scenario.onActivity{vm.closeExpression()}
        assertNull(vm.state.error)
    }
    @Test fun expressionEditsBaseValuePreservesInvalidDraftAndSupportsAxesUndo() {
        scenario.onActivity{vm.openProperty("position")}
        compose.onNodeWithContentDescription("图层操作").performClick()
        compose.onNodeWithTag("open-property-expression").performClick()
        visibleWorkspace("expression-workspace")
        val target=vm.expressionTarget!!
        applySource("value+[time*60,0,0]")
        compose.waitUntil(10000){vm.expressionFor(target)?.optString("source")=="value+[time*60,0,0]"&&vm.state.saved}
        val base=vm.layer(2)!!.getJSONObject("transform").getJSONObject("position").getJSONArray("value").getDouble(0)
        scenario.onActivity{vm.seek(15.0)}
        compose.waitUntil(10000){vm.sampleValueFor(2,"position").let{it is org.json.JSONArray&&kotlin.math.abs(it.getDouble(0)-base-30)<.01}}
        photo("expression-preview-timeline")
        val revision=vm.state.sample!!.getLong("revision")
        applySource("unknown()")
        compose.waitUntil(10000){compose.onAllNodesWithTag("expression-error").fetchSemanticsNodes().isNotEmpty()}
        assertEquals(revision,vm.state.sample!!.getLong("revision"))
        compose.onNodeWithTag("expression-source").assertTextContains("unknown()")
        compose.onNodeWithTag("expression-enabled").performScrollTo().performClick()
        compose.onNodeWithTag("expression-apply").performScrollTo().performClick()
        compose.waitUntil(10000){vm.expressionFor(target)?.optString("source")=="unknown()"&&!vm.expressionFor(target)!!.getBoolean("enabled")}
        compose.onNodeWithTag("expression-scope-x").performClick()
        compose.onNodeWithTag("expression-apply").assertIsNotEnabled()
        compose.onNodeWithTag("expression-remove-conflict").performScrollTo().performClick()
        compose.waitUntil(10000){vm.expressionFor(target)==null}
        applySource("value+10")
        val x=JSONObject(target.toString()).put("axis","x")
        compose.waitUntil(10000){vm.expressionFor(x)!=null}
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.expressionFor(x)==null}
        scenario.onActivity{vm.redo()}
        compose.waitUntil(10000){vm.expressionFor(x)!=null}
        scenario.recreate();compose.waitForIdle()
        compose.onNodeWithTag("expression-workspace").assertIsDisplayed()
        scenario.onActivity{vm.closeExpression()}
        compose.onNodeWithTag("expression-workspace").assertDoesNotExist()
        assertNull(vm.state.error)
    }
    private fun web(root:View):WebView?=if(root is WebView)root else if(root is ViewGroup)(0 until root.childCount).firstNotNullOfOrNull{web(root.getChildAt(it))}else null
    @Test fun lateExpressionFailureKeepsPreviewEditableAndCanBeDisabled() {
        scenario.onActivity{vm.openProperty("position");vm.openExpression(vm.expressionTargetForCurrent()!!)}
        val target=vm.expressionTarget!!
        applySource("time<.1 ? value : unknown()")
        compose.waitUntil(10000){vm.expressionFor(target)!=null&&vm.state.saved}
        scenario.onActivity{vm.seek(15.0)}
        compose.waitUntil(10000){vm.state.sample?.optString("renderError")?.startsWith("expression ")==true}
        compose.onNodeWithTag("expression-render-error").assertIsDisplayed()
        visibleWorkspace("expression-workspace");assertNull(vm.lastGpuFailure)
        photo("expression-error-repair")
        compose.onNodeWithTag("expression-enabled").performScrollTo().performClick()
        compose.onNodeWithTag("expression-apply").performScrollTo().performClick()
        compose.waitUntil(10000){vm.expressionFor(target)?.optBoolean("enabled")==false&&vm.state.sample?.isNull("renderError")==true}
        compose.onNodeWithTag("expression-render-error").assertDoesNotExist();assertNull(vm.state.error)
    }
    private fun js(script:String):String {
        var result:String?=null;val latch=CountDownLatch(1)
        scenario.onActivity{activity->
            val current=web(activity.window.decorView)
            if(current==null){result="0";latch.countDown()}else current.evaluateJavascript(script){result=it;latch.countDown()}
        }
        assertTrue("WebView did not reply",latch.await(10,TimeUnit.SECONDS));return result!!
    }
    private fun plugin(id:String) {
        scenario.onActivity{vm.openEffects()}
        compose.onNodeWithTag("effects-add").performClick()
        compose.onNodeWithTag("effect-add-$id").performScrollTo().performClick()
        compose.waitUntil(10000){vm.layer(2)?.optJSONArray("effects")?.length()==1&&vm.state.saved}
        compose.onNodeWithTag("effect-instance-1").onChildren()[0].performClick()
        compose.onNodeWithTag("plugin-editor-open").performClick()
        compose.waitUntil(20000){vm.pluginEditor.session!=null}
        compose.waitUntil(15000){js("document.querySelectorAll('input[type=number]').length").toInt()>0}
        visibleWorkspace("effects-panel")
    }
    @Test fun actualWebViewLoadsParticlePageEditsAndRollsBackAnUncommittedGesture() {
        plugin("starfield")
        val host=vm.pluginEditor;val session=host.session!!
        assertEquals(403,pluginEditorResource(session,Uri.parse("https://example.com/ui/editor.js")).statusCode)
        assertEquals(403,pluginEditorResource(session,Uri.parse(session.origin+"/project.json")).statusCode)
        assertEquals(403,pluginEditorResource(session,Uri.parse(session.origin+"/ui/../editor.js")).statusCode)
        assertEquals(200,pluginEditorResource(session,Uri.parse(session.origin+"/"+session.entry)).statusCode)
        val initial=vm.effectParam(2,1,"rate")!!.getJSONObject("track").getJSONArray("value").getDouble(0)
        val label=session.definition.getJSONArray("params").objects().first{it.getString("id")=="rate"}.getString("name")
        val script="(()=>{const input=Array.from(document.querySelectorAll('input[type=number]')).find(i=>i.getAttribute('aria-label')===${JSONObject.quote(label)}); input.value='${initial+1}'; input.dispatchEvent(new Event('change',{bubbles:true}));return true})()"
        assertEquals("true",js(script))
        compose.waitUntil(15000){vm.effectParam(2,1,"rate")!!.getJSONObject("track").getJSONArray("value").getDouble(0)==initial+1&&vm.state.saved}
        photo("particle-editor-timeline")
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.effectParam(2,1,"rate")!!.getJSONObject("track").getJSONArray("value").getDouble(0)==initial&&host.state?.optLong("revision")==vm.state.sample?.optLong("revision")}
        scenario.onActivity{vm.seek(15.0)}
        compose.waitUntil(10000){host.state?.optDouble("frame")==15.0}
        fun send(request:JSONObject):JSONObject {
            val latch=CountDownLatch(1);var reply:JSONObject?=null
            scenario.onActivity{host.message(JSONObject().put("protocol",1).put("token",session.token).put("id",UUID.randomUUID().toString()).put("message",request).toString()){reply=it;latch.countDown()}}
            assertTrue(latch.await(10,TimeUnit.SECONDS));assertTrue(reply.toString(),reply!!.getBoolean("ok"));return reply!!.getJSONObject("result")
        }
        val begin=send(JSONObject().put("op","begin").put("revision",host.state!!.getLong("revision")))
        scenario.onActivity{vm.seek(20.0)};assertEquals(15.0,vm.frame,0.0)
        send(JSONObject().put("op","set").put("revision",begin.getLong("revision")).put("param","rate").put("value",org.json.JSONArray(listOf(initial+2,0,0,0))))
        scenario.onActivity{host.close()}
        compose.waitUntil(10000){host.session==null&&vm.effectParam(2,1,"rate")!!.getJSONObject("track").getJSONArray("value").getDouble(0)==initial}
        assertNull(vm.state.error)
    }
    @Test fun lensEditorConnectsAndProjectReplacementClosesItsSession() {
        plugin("lens_flare");photo("lens-editor-timeline")
        val token=vm.pluginEditor.session!!.token
        assertEquals("true",js("typeof window.motionStudioUpdate === 'function'"))
        scenario.onActivity{vm.newProject(256,144)}
        compose.waitUntil(15000){vm.pluginEditor.session==null&&vm.state.project?.optInt("width")==256&&!vm.state.busy}
        assertTrue(token.isNotEmpty());assertNull(vm.expressionTarget);assertNull(vm.state.error)
    }
    @Test fun workspacesKeepPreviewAndTimelineUsableAtActualDisplayAndFontSizes() {
        scenario.onActivity{vm.openProperty("position");vm.openExpression(vm.expressionTargetForCurrent()!!)}
        visibleWorkspace("expression-workspace")
        compose.onNodeWithTag("expression-source").performScrollTo().assertIsDisplayed()
        compose.onNodeWithTag("expression-apply").performScrollTo().assertIsDisplayed()
        photo("workspace-expression")
        scenario.onActivity{vm.closeExpression()}
        plugin("starfield")
        assertTrue("plugin page overflows horizontally",js("document.documentElement.scrollWidth-document.documentElement.clientWidth").toDouble()<=1)
        photo("workspace-particles")
        val config=InstrumentationRegistry.getInstrumentation().targetContext.resources.configuration
        File(root,"layout-profile-report.json").writeText(JSONObject().put("fontScale",config.fontScale).put("densityDpi",config.densityDpi)
            .put("screenWidthDp",config.screenWidthDp).put("screenHeightDp",config.screenHeightDp).put("passed",true).toString(2))
        assertNull(vm.state.error)
    }
}
