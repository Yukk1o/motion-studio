package com.motionstudio.editor

import org.junit.Assert.*
import org.junit.Test

class CompositionDurationTest {
    @Test fun fractionalSecondsUseTheSelectedFrameRate() {
        assertEquals(75,compositionFrames("1.25",60))
        assertEquals(38,compositionFrames("1.25",30))
        assertEquals(360,compositionFrames("6",60))
    }
    @Test fun invalidAndUnrepresentableDurationsAreRejected() {
        listOf("", "0", "-1", "NaN", "Infinity", "0.001", "600.1").forEach{assertNull(compositionFrames(it,60))}
        assertEquals(36000,compositionFrames("600",60))
        assertEquals(36000,compositionFrames("1200",30))
    }
}
