package com.motionstudio.editor

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test
import java.nio.ByteBuffer
import java.nio.ByteOrder

class RenderPlanBudgetTest {
    private fun plan(bytes:Int,version:Int=RenderPlanBudget.VERSION)=ByteBuffer.allocate(128).order(ByteOrder.nativeOrder()).apply {
        putInt(0,0x46584d53);putInt(4,version);putInt(76,bytes)
    }
    @Test fun readsAllNativeTiersWithoutAnIndependentScratchConstant() {
        for(mib in listOf(64,96,192,384))assertEquals(mib.toLong()*1024*1024,RenderPlanBudget.scratchBytes(plan(mib*1024*1024)))
    }
    @Test fun rejectsMissingBudgetOldProtocolAndShortHeader() {
        assertThrows(IllegalStateException::class.java){RenderPlanBudget.scratchBytes(plan(0))}
        assertThrows(IllegalStateException::class.java){RenderPlanBudget.scratchBytes(plan(64*1024*1024,5))}
        assertThrows(IllegalStateException::class.java){RenderPlanBudget.scratchBytes(ByteBuffer.allocate(76))}
    }
}
