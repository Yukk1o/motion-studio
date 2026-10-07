package com.motionstudio.editor

import android.app.Application
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.*
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*

class ReleaseUpdatesTest {
    @get:Rule val compose=createComposeRule()
    private val app get()=InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as Application
    private lateinit var scope:CoroutineScope
    @Before fun setup(){app.getSharedPreferences("motion-release-updates",0).edit().clear().commit();scope=CoroutineScope(SupervisorJob()+Dispatchers.Main.immediate)}
    @After fun teardown(){scope.cancel();app.getSharedPreferences("motion-release-updates",0).edit().clear().commit()}
    private fun notice(code:Long)=ReleaseNotice("0.1.0-preview.$code",code,"新增图层操作\n修复播放预览", "$RELEASE_PAGE/tag/preview",true)
    private fun create(fetch:suspend(Boolean)->ReleaseNotice?):ReleaseUpdates {lateinit var result:ReleaseUpdates;compose.runOnIdle{result=ReleaseUpdates(app,scope,fetch,automatic=false)};return result}
    @Test fun parsesOnlyMatchingPublishedChannelAndVerifiedApkMetadata() {
        val release=JSONObject().put("tag_name","preview").put("prerelease",true).put("html_url","$RELEASE_PAGE/tag/preview").put("body","更新内容").put("assets",JSONArray().put(JSONObject().put("name","MotionStudio-preview.apk")))
        val info=JSONObject().put("versionCode",123).put("versionName","0.1.0-preview.123").put("kind","preview").put("apk","MotionStudio-preview.apk")
        assertEquals(123L,parseRelease(release,info,true)!!.code)
        assertNull(parseRelease(release,info,false))
        assertNull(parseRelease(JSONObject(release.toString()).put("draft",true),info,true))
        assertNull(parseRelease(JSONObject(release.toString()).put("html_url","https://example.invalid/download"),info,true))
        assertNull(parseRelease(release,JSONObject(info.toString()).put("apk","missing.apk"),true))
        assertNull(parseRelease(release,JSONObject(info.toString()).put("versionCode",0),true))
        val stable=JSONObject(release.toString()).put("tag_name","v1.0.0").put("prerelease",false).put("html_url","$RELEASE_PAGE/tag/v1.0.0")
        assertNotNull(parseRelease(stable,JSONObject(info.toString()).put("kind","release").put("versionName","1.0.0"),false))
    }
    @Test fun slowFetchRunsInBackgroundAndRepeatedChecksDoNotStartAnotherRequest() {
        val ready=CompletableDeferred<Unit>();val finish=CompletableDeferred<Unit>();var calls=0
        val updates=create{calls++;ready.complete(Unit);finish.await();notice(100000)}
        compose.runOnIdle{updates.check(true)}
        runBlocking{withTimeout(5000){ready.await()}}
        var editingStillResponsive=false
        compose.runOnIdle{editingStillResponsive=true;updates.check(true)}
        assertTrue(editingStillResponsive);assertTrue(updates.checking);assertEquals(1,calls)
        finish.complete(Unit);compose.waitUntil(5000){!updates.checking}
        assertTrue(updates.available)
    }
    @Test fun offlineCheckPreservesCachedNotesAndDismissalOnlyHidesThatVersion() {
        var next=notice(100000)
        val updates=create{next};compose.runOnIdle{updates.check(true)};compose.waitUntil(5000){!updates.checking}
        assertTrue(updates.available)
        compose.runOnIdle{updates.dismiss()};assertFalse(updates.available)
        val offline=create{throw java.io.IOException("offline")}
        assertEquals(next.notes,offline.release!!.notes);assertFalse(offline.available)
        compose.runOnIdle{offline.check(true)};compose.waitUntil(5000){!offline.checking}
        assertEquals(next.notes,offline.release!!.notes);assertNotNull(offline.status)
        next=notice(100001);compose.runOnIdle{updates.check(true)};compose.waitUntil(5000){!updates.checking}
        assertTrue(updates.available)
    }
    @Test fun automaticChecksReuseTheSixHourCache() {
        val updates=create{notice(100000)};compose.runOnIdle{updates.check(true)};compose.waitUntil(5000){!updates.checking}
        var calls=0
        compose.runOnIdle{ReleaseUpdates(app,scope,{calls++;notice(100001)})}
        compose.runOnIdle{assertEquals(0,calls)}
    }
    @Test fun bannerOpensNotesAndCanBeDismissedWithoutBlockingThePage() {
        val updates=create{notice(100000)};var opened by androidx.compose.runtime.mutableStateOf(false)
        compose.setContent{MaterialTheme{if(opened)ReleaseNotes(updates){opened=false}else ReleaseUpdateBanner(updates){opened=true}}}
        compose.runOnIdle{updates.check(true)};compose.waitUntil(5000){updates.available}
        compose.onNodeWithText("查看更新").performClick();compose.onNodeWithTag("release-notes").assertIsDisplayed()
        compose.onNodeWithText("关闭").performClick();compose.onNodeWithTag("dismiss-update").performClick()
        compose.onNodeWithTag("update-banner").assertDoesNotExist()
    }
}
