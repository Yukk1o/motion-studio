package com.motionstudio.editor

import org.junit.Assert.*
import org.junit.Test

class PlaybackClockTest {
    @Test fun callbackBeforePlayNeverProducesNegativeTime() {
        assertEquals(0.0,playbackFrame(0.0,1_000_000_000,990_000_000,30,180),0.0)
        assertEquals(72.0,playbackFrame(72.0,1_000_000_000,990_000_000,30,180),0.0)
    }
    @Test fun monotonicClockKeepsDurationFractionsAndLoopBoundary() {
        assertEquals(15.0,playbackFrame(0.0,1_000_000_000,1_500_000_000,30,180),.000001)
        assertEquals(0.0,playbackFrame(0.0,1_000_000_000,7_000_000_000,30,180),.000001)
        val lastHalf=playbackFrame(0.0,0,5_983_333_333,30,180)
        assertEquals(179.5,lastHalf,.000001)
        assertTrue(lastHalf>=0&&lastHalf<180)
    }
    @Test fun staleSeedFromLongerCompositionIsNormalized() {
        assertEquals(150.0,playbackFrame(330.0,0,0,30,180),.000001)
        assertEquals(0.0,playbackFrame(Double.NaN,0,0,30,180),0.0)
    }
}
