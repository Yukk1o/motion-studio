package com.motionstudio.editor

import org.junit.Assert.assertEquals
import org.junit.Test

class NumericWheelSensitivityTest {
    @Test fun broadCoordinateAndDistanceBoundsDoNotIncreaseDragSpeed() {
        assertEquals(10.0,numericWheelStep(-100000.0,100000.0),0.0)
        assertEquals(10.0,numericWheelStep(1.0,100000.0),0.0)
        assertEquals(10.0,numericWheelStep(-1000.0,1000.0),0.0)
    }
    @Test fun fractionalParametersKeepTheirFineSteps() {
        assertEquals(.01,numericWheelStep(0.0,1.0),1e-9)
        assertEquals(.02,numericWheelStep(-1.0,1.0),1e-9)
        assertEquals(.1,numericWheelStep(0.0,16.0),1e-9)
    }
}
