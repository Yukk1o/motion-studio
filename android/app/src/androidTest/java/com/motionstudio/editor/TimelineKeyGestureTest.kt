package com.motionstudio.editor

import android.content.Intent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.*
import org.junit.Assert.*
import java.io.File
import java.util.UUID

class TimelineKeyGestureTest {
    @get:Rule val compose=createEmptyComposeRule()
    private lateinit var scenario:ActivityScenario<AcceptanceActivity>
    private lateinit var vm:EditorViewModel
    private var density=1f
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        density=context.resources.displayMetrics.density
        val root=File(context.filesDir,"acceptance/key-gesture-${UUID.randomUUID()}").apply{mkdirs()}
        File(root,"project.json").writeText(LayerClipTimelineTest().fixture().toString())
        scenario=ActivityScenario.launch(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",root.absolutePath))
        scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
        compose.waitUntil(20000){vm.state.project!=null}
        scenario.onActivity{vm.select(2,false);vm.property="rotation";vm.timelineScale=3f;vm.seek(40.0);vm.addKey()}
        compose.waitUntil(10000){vm.keys().size==3&&vm.state.saved}
    }
    @After fun teardown(){if(::scenario.isInitialized)scenario.close()}
    @Test fun ordinarySwipeFromAKeyScrubsWithoutMovingIt() {
        val before=vm.state.project!!.toString();val history=vm.state.canUndo
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x,9*density));moveBy(Offset(60*density,0f),100);up()
        }
        scenario.onActivity{vm.gestureInertia.stop()}
        compose.waitForIdle()
        assertEquals(before,vm.state.project!!.toString());assertEquals(history,vm.state.canUndo)
        assertTrue("ordinary key-area swipe should scrub",vm.frame<40.0)
        assertTrue(vm.keys().any{it.getInt("frame")==40});assertNull(vm.state.error)
    }
    @Test fun longPressDragMovesAKeyAndOneUndoRestoresIt() {
        val before=vm.state.project!!.toString()
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x,9*density));advanceEventTime(600);moveBy(Offset(60*density,0f),100);up()
        }
        compose.waitUntil(10000){vm.state.saved&&vm.keys().any{it.getInt("frame")==60}}
        assertEquals(60.0,vm.frame,0.0)
        assertFalse(vm.keys().any{it.getInt("frame")==40})
        scenario.onActivity{vm.undo()}
        compose.waitUntil(10000){vm.state.saved&&vm.state.project!!.toString()==before}
        assertNull(vm.state.error)
    }
    @Test fun cancelledLongPressDragKeepsTheProjectAndPlayhead() {
        val before=vm.state.project!!.toString();val history=vm.state.canUndo
        compose.onNodeWithTag("timeline").performTouchInput {
            down(Offset(center.x,9*density));advanceEventTime(600);moveBy(Offset(60*density,0f),100);cancel()
        }
        compose.waitForIdle()
        assertEquals(before,vm.state.project!!.toString());assertEquals(history,vm.state.canUndo)
        assertEquals(40.0,vm.frame,0.0);assertNull(vm.state.error)
    }
    @Test fun tappingAKeyKeepsItsPositionAndDoesNotEditHistory() {
        val before=vm.state.project!!.toString();val history=vm.state.canUndo
        compose.onNodeWithTag("timeline").performTouchInput{down(Offset(center.x,9*density));advanceEventTime(40);up()}
        compose.waitForIdle()
        assertEquals(before,vm.state.project!!.toString());assertEquals(history,vm.state.canUndo)
        assertEquals(2L,vm.selected);assertEquals("rotation",vm.property);assertEquals(40.0,vm.frame,0.0)
        assertNull(vm.state.error)
    }
}
