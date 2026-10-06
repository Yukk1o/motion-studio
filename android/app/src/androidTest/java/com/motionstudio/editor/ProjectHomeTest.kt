package com.motionstudio.editor

import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.UiDevice
import org.junit.*
import org.junit.Assert.*
import java.io.File
import java.util.UUID

class ProjectHomeTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private lateinit var root:File
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(context.filesDir,"acceptance/home-${UUID.randomUUID()}/default").apply{mkdirs()}
        val project=LayerClipTimelineTest().fixture().put("name","当前工程")
        File(root,"project.json").writeText(project.toString())
        listOf("project-1" to "横屏练习","project-2" to "竖屏练习").forEach{(directory,name)->
            val folder=File(root.parentFile,directory).apply{mkdirs()}
            File(folder,"project.json").writeText(project.put("name",name).toString())
        }
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null}
        compose.onNodeWithContentDescription("工程列表").performClick()
        compose.waitUntil(10000){!vm.projectsLoading&&vm.projects.size==3}
        compose.onNodeWithTag("project-home").assertIsDisplayed()
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    private fun photo(name:String) {
        compose.waitForIdle();InstrumentationRegistry.getInstrumentation().waitForIdleSync()
        android.os.SystemClock.sleep(150)
        val bitmap=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
        File(root,"$name.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
    }
    @Test fun searchFiltersProjectsAndResumePreservesTheCurrentProject() {
        val before=vm.state.project!!.toString();val active=vm.root
        photo("project-home")
        compose.onNodeWithTag("home-project-search").performTextInput("横屏")
        compose.onNodeWithTag("home-project-project-1").assertIsDisplayed()
        compose.onNodeWithTag("home-project-default").assertDoesNotExist()
        compose.onNodeWithContentDescription("清空工程搜索").performClick()
        compose.onNodeWithTag("home-project-grid").performScrollToNode(hasTestTag("home-project-default"))
        compose.onNodeWithTag("home-project-default").performClick()
        compose.onNodeWithTag("timeline").assertIsDisplayed()
        assertEquals(active,vm.root);assertEquals(before,vm.state.project!!.toString())
    }
    @Test fun openingARecentProjectUsesTheRealProjectDirectory() {
        compose.onNodeWithTag("home-project-project-1").performClick()
        compose.waitUntil(20000){vm.root.name=="project-1"&&vm.state.project?.optString("name")=="横屏练习"&&!vm.state.busy}
        compose.onNodeWithTag("timeline").assertIsDisplayed();assertNull(vm.state.error)
        compose.onNodeWithContentDescription("工程列表").performClick()
        compose.onNodeWithTag("home-project-project-1").assertIsDisplayed()
    }
    @Test fun newProjectValidatesDimensionsAndCreatesWithoutOverwritingTheOldOne() {
        val previous=vm.state.project!!.toString()
        compose.onNodeWithTag("home-new-project").performClick()
        compose.onNodeWithTag("new-project-width").performTextReplacement("0")
        compose.onNodeWithTag("create-project").assertIsNotEnabled()
        compose.onNodeWithTag("new-project-preset-16:9").performClick()
        compose.onNodeWithTag("new-project-name").performTextInput("测试工程")
        compose.onNodeWithTag("new-project-name").performImeAction()
        compose.onNodeWithTag("new-project-fps-60").performScrollTo().performClick()
        compose.onNodeWithTag("new-project-duration").performScrollTo().performTextReplacement("0")
        compose.onNodeWithTag("create-project").assertIsNotEnabled()
        compose.onNodeWithTag("new-project-duration").performTextReplacement("1.25")
        compose.onNodeWithTag("new-project-duration").performImeAction();photo("new-project")
        compose.onNodeWithTag("create-project").performClick()
        compose.waitUntil(20000){vm.root.canonicalFile!=root.canonicalFile&&vm.state.saved&&vm.state.project?.optString("name")=="测试工程"}
        assertEquals(1920,vm.state.project!!.getInt("width"));assertEquals(1080,vm.state.project!!.getInt("height"));assertEquals(60,vm.state.project!!.getInt("fps"))
        assertEquals(75,vm.state.project!!.getInt("frames"))
        scenario.onActivity{vm.openProject(root.name)}
        compose.waitUntil(20000){vm.root.canonicalFile==root.canonicalFile&&vm.state.saved&&!vm.state.busy}
        assertEquals(previous,vm.state.project!!.toString());assertNull(vm.state.error)
    }
    @Test fun editorSettingsOpenTheSameCustomCompositionForm() {
        compose.onNodeWithTag("home-project-default").performClick()
        compose.onNodeWithContentDescription("合成设置").performClick()
        compose.onNodeWithTag("open-plugins").assertDoesNotExist()
        compose.onNodeWithTag("adjust-layout").assertDoesNotExist()
        compose.onNodeWithTag("reset-layout").assertDoesNotExist()
        compose.onNodeWithTag("open-new-composition").performScrollTo().performClick()
        compose.onNodeWithTag("new-project-duration").performScrollTo().performTextReplacement("0.5")
        compose.onNodeWithTag("new-project-duration").performImeAction()
        compose.onNodeWithTag("create-project").performClick()
        compose.waitUntil(20000){vm.root.canonicalFile!=root.canonicalFile&&vm.state.saved&&!vm.state.busy}
        assertEquals(30,vm.state.project!!.getInt("fps"));assertEquals(15,vm.state.project!!.getInt("frames"));assertNull(vm.state.error)
    }
    @Test fun settingsReturnToTheFilteredHomeAndSurviveRecreation() {
        val before=vm.state.project!!.toString();val current=vm.root
        compose.onNodeWithTag("home-project-search").performTextInput("横屏")
        compose.onNodeWithTag("home-project-search").performImeAction()
        compose.onNodeWithTag("home-settings").performClick()
        compose.onNodeWithTag("home-settings-page").assertIsDisplayed()
        compose.onNodeWithTag("reset-layout").assertIsNotEnabled()
        photo("home-settings")
        scenario.recreate();scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.onNodeWithTag("home-settings-page").assertIsDisplayed()
        compose.onNodeWithContentDescription("返回主页").performClick()
        compose.onNodeWithTag("home-project-search").assertTextEquals("横屏")
        compose.onNodeWithTag("home-project-project-1").assertIsDisplayed()
        compose.onNodeWithTag("home-project-default").assertDoesNotExist()
        assertEquals(current,vm.root);assertEquals(before,vm.state.project!!.toString())
    }
    @Test fun pluginManagementEnablesRealPackagesAndReturnsToSettings() {
        val before=vm.state.project!!.toString()
        compose.onNodeWithTag("home-settings").performClick()
        compose.onNodeWithTag("open-plugins").performScrollTo().performClick()
        compose.waitUntil(15000){vm.catalogue!=null}
        val pkg=effectPackages(vm).first{it.getJSONObject("manifest").getString("id")=="com.motionstudio.effects.ae2021"}
        val manifest=pkg.getJSONObject("manifest");val id=manifest.getString("id");val version=manifest.getString("version")
        val tag="plugin-enable-$id-$version"
        val initiallyEnabled=pkg.getBoolean("enabled")
        photo("home-plugins")
        compose.onNodeWithTag(tag).performScrollTo().performClick()
        compose.waitUntil(15000){effectPackages(vm).first{it.getJSONObject("manifest").getString("id")==id}.getBoolean("enabled")!=initiallyEnabled}
        compose.onNodeWithTag(tag).performClick()
        compose.waitUntil(15000){effectPackages(vm).first{it.getJSONObject("manifest").getString("id")==id}.getBoolean("enabled")==initiallyEnabled}
        compose.onNodeWithTag("plugin-uninstall-$id-$version").assertDoesNotExist()
        compose.onNodeWithContentDescription("返回设置").performClick()
        compose.onNodeWithTag("home-settings-page").assertIsDisplayed()
        compose.onNodeWithTag("adjust-layout").assertIsDisplayed()
        assertEquals(before,vm.state.project!!.toString());assertNull(vm.state.error)
    }
    @Test fun cancelledAndInvalidPackageInstallationStayInHomeSettings() {
        val before=vm.state.project!!.toString();val current=vm.root
        compose.onNodeWithTag("home-settings").performClick()
        compose.onNodeWithTag("open-plugins").performScrollTo().performClick()
        compose.onNodeWithTag("plugin-install").performClick()
        val device=UiDevice.getInstance(InstrumentationRegistry.getInstrumentation())
        compose.waitUntil(15000){device.currentPackageName?.endsWith(".documentsui")==true}
        device.pressBack()
        compose.waitUntil(15000){device.currentPackageName==InstrumentationRegistry.getInstrumentation().targetContext.packageName}
        compose.onNodeWithTag("plugins-page").assertIsDisplayed()
        assertEquals(current,vm.root);assertEquals(before,vm.state.project!!.toString())
        scenario.onActivity{vm.installPlugin(Uri.parse("content://com.motionstudio.editor.test.audio-fixtures/tone-stereo-48000.wav"))}
        compose.waitUntil(20000){!vm.state.busy&&vm.state.error!=null}
        compose.onNodeWithText("操作未完成").assertIsDisplayed();photo("home-plugin-install-error")
        compose.onNodeWithText("知道了").performClick()
        compose.onNodeWithTag("plugins-page").assertIsDisplayed()
        assertEquals(current,vm.root);assertEquals(before,vm.state.project!!.toString());assertNull(vm.state.error)
        device.pressBack();compose.onNodeWithTag("home-settings-page").assertIsDisplayed()
    }
}
