package com.motionstudio.editor

import androidx.compose.foundation.layout.width
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.unit.dp
import org.junit.Assert.*
import org.junit.Before
import org.junit.Rule
import org.junit.Test

class NumericWheelInertiaTest {
    @get:Rule val compose=createComposeRule()
    private var value by mutableDoubleStateOf(0.0)
    private var minimum by mutableDoubleStateOf(-1000.0)
    private var maximum by mutableDoubleStateOf(1000.0)
    private var visible by mutableStateOf(true)
    private var commits=0
    private var cancellations=0
    @Before fun setup() {
        compose.setContent {StudioTheme {
            if(visible)NumericWheel(value,minimum,maximum,"数值",true,Modifier.width(300.dp).testTag("wheel"),
                onValueChange={value=it},onFinished={commits++},onCancelled={cancellations++;value=0.0})
        }}
        compose.mainClock.autoAdvance=false
    }
    private fun flick()=compose.onNodeWithTag("wheel").performTouchInput {
        down(center);moveBy(Offset(-30f,0f),40);moveBy(Offset(-30f,0f),40);up()
    }
    @Test fun releaseContinuesThenSlowsAndCommitsOnce() {
        flick();var released=0.0
        compose.runOnIdle{released=value;assertEquals(0,commits)}
        compose.mainClock.advanceTimeBy(80)
        var early=0.0;compose.runOnIdle{early=value;assertTrue(early>released)}
        compose.mainClock.advanceTimeBy(80)
        compose.runOnIdle{assertTrue(value>early);assertTrue(value-early<early-released)}
        compose.mainClock.advanceTimeBy(2000)
        compose.runOnIdle{assertEquals(1,commits);assertEquals(0,cancellations)}
    }
    @Test fun touchingAgainStopsBeforeAnotherDragStarts() {
        flick();compose.mainClock.advanceTimeBy(80)
        compose.onNodeWithTag("wheel").performTouchInput{down(center)}
        var stopped=0.0;compose.runOnIdle{stopped=value;assertEquals(1,commits)}
        compose.mainClock.advanceTimeBy(500)
        compose.runOnIdle{assertEquals(stopped,value,0.0)}
        compose.onNodeWithTag("wheel").performTouchInput{up()}
        compose.runOnIdle{assertEquals(1,commits)}
    }
    @Test fun holdingStillBeforeReleaseDoesNotFling() {
        compose.onNodeWithTag("wheel").performTouchInput {
            down(center);moveBy(Offset(-40f,0f),40);advanceEventTime(150);up()
        }
        var released=0.0;compose.runOnIdle{released=value;assertEquals(1,commits)}
        compose.mainClock.advanceTimeBy(1500)
        compose.runOnIdle{assertEquals(released,value,0.0)}
    }
    @Test fun cancelledDragRestoresValueAndNeverCommits() {
        compose.onNodeWithTag("wheel").performTouchInput{down(center);moveBy(Offset(-40f,0f),40);cancel()}
        compose.mainClock.advanceTimeBy(1500)
        compose.runOnIdle{assertEquals(0.0,value,0.0);assertEquals(1,cancellations);assertEquals(0,commits)}
    }
    @Test fun rangeBoundaryStopsAndCannotOvershoot() {
        compose.runOnIdle{value=995.0}
        compose.mainClock.advanceTimeByFrame()
        flick();compose.mainClock.advanceTimeBy(1500)
        compose.runOnIdle{assertEquals(1000.0,value,0.0);assertEquals(1,commits)}
    }
    @Test fun removingTheControlSettlesItsLastValueOnce() {
        flick();compose.mainClock.advanceTimeBy(80)
        compose.runOnIdle{visible=false}
        compose.mainClock.advanceTimeByFrame()
        var stopped=0.0;compose.runOnIdle{stopped=value;assertEquals(1,commits)}
        compose.mainClock.advanceTimeBy(1500)
        compose.runOnIdle{assertEquals(stopped,value,0.0);assertEquals(1,commits);assertEquals(0,cancellations)}
    }
    @Test fun wideCoordinateRangeStaysPreciseAndKeepsTheFullInputRange() {
        compose.runOnIdle{minimum=-100000.0;maximum=100000.0;value=1000.25}
        compose.mainClock.advanceTimeByFrame()
        flick()
        compose.runOnIdle{assertTrue(value>1000.25);assertTrue(value-1000.25<100)}
        compose.mainClock.advanceTimeBy(2000)
        compose.runOnIdle{assertTrue(value-1000.25<250);assertEquals(1,commits)}
        compose.onNodeWithTag("wheel").performSemanticsAction(androidx.compose.ui.semantics.SemanticsActions.SetProgress){it(100000f)}
        compose.runOnIdle{assertEquals(100000.0,value,0.0);assertEquals(2,commits)}
    }
}
