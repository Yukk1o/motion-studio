package com.motionstudio.editor

import java.nio.ByteBuffer

/** v7 retains word 19's scratch allowance; image passes can override resource 0. */
internal object RenderPlanBudget {
    const val VERSION=7
    fun scratchBytes(plan:ByteBuffer):Long {
        check(plan.capacity()>=128&&plan.getInt(0)==0x46584d53&&plan.getInt(4)==VERSION){"不兼容的帧预算协议"}
        val bytes=plan.getInt(76).toLong()
        check(bytes>0){"效果临时纹理预算失效"}
        return bytes
    }
}
