package com.motionstudio.editor

import org.junit.Assert.*
import org.junit.Test

class GestureVelocityTest {
    private val limits=FlingLimits(50f,8000f)
    @Test fun stationaryReleaseSuppressesOldVelocity() {
        assertEquals(0f,releaseVelocity(1200f,250,100,limits),0f)
    }
    @Test fun releaseKeepsDirectionWithinDeviceLimit() {
        assertEquals(-8000f,releaseVelocity(-12000f,200,190,limits),0f)
        assertEquals(400f,releaseVelocity(400f,200,190,limits),0f)
    }
    @Test fun invalidSamplesCannotStartMotion() {
        assertEquals(0f,releaseVelocity(Float.NaN,100,100,limits),0f)
        assertEquals(0f,releaseVelocity(Float.POSITIVE_INFINITY,100,100,limits),0f)
    }
}
