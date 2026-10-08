package com.motionstudio.editor
import org.junit.Assert.*
import org.junit.Test

class ColorModelTest {
    @Test fun referenceHexAndRgbaOrderAreUnambiguous() {
        val yellow=Rgba.hex("#FFD600",1.0,true)!!
        assertEquals(1.0,yellow.r,0.0);assertEquals(214.0/255,yellow.g,0.0);assertEquals(0.0,yellow.b,0.0)
        assertEquals("#FFD600",yellow.hex())
        val rgba=Rgba.hex("#4020F080",1.0,true)!!
        assertEquals(64.0/255,rgba.r,0.0);assertEquals(32.0/255,rgba.g,0.0);assertEquals(240.0/255,rgba.b,0.0);assertEquals(128.0/255,rgba.a,0.0)
        assertEquals("#4020F080",rgba.hex())
    }
    @Test fun sixDigitHexPreservesFullPrecisionAlphaAndRejectsInvalidInput() {
        val alpha=.3725191234;assertEquals(alpha,Rgba.hex("#FFFFFF",alpha,true)!!.a,0.0)
        for(raw in listOf("#FFFFF","#FFFFFG","#FFFFFFFFF","NaN","##123456"))assertNull(raw,Rgba.hex(raw,1.0,true))
        assertNull(Rgba.hex("#FFFFFF80",1.0,false))
    }
    @Test fun hsvRoundTripsAndGrayRetainsTheChosenHue() {
        for(v in listOf(Rgba(.71,.29,.94,.371),Rgba(1.0,0.0,0.0,1.0),Rgba(.5,.5,.5,.2),Rgba(0.0,0.0,0.0,.8))) {
            val h=v.hsv(218.0);val restored=Rgba.hsv(h[0],h[1],h[2],v.a)
            assertEquals(v.r,restored.r,1e-12);assertEquals(v.g,restored.g,1e-12);assertEquals(v.b,restored.b,1e-12);assertEquals(v.a,restored.a,0.0)
        }
        assertEquals(218.0,Rgba(.5,.5,.5,1.0).hsv(218.0)[0],0.0)
        assertEquals(Rgba(1.0,0.0,0.0,1.0),Rgba.hsv(360.0,1.0,1.0,1.0))
    }
    @Test fun changingAlphaDoesNotQuantizeRgb() {
        val original=Rgba(.700123456,.513273,.884114,1.0)
        val changed=original.with(3,.12501)
        assertEquals(original.r,changed.r,0.0);assertEquals(original.g,changed.g,0.0);assertEquals(original.b,changed.b,0.0)
    }
}
