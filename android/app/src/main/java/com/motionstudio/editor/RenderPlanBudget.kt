package com.motionstudio.editor

import java.nio.ByteBuffer

/** v9 adds ordered vector paint commands; the header and scratch allowance remain stable. */
internal object RenderPlanBudget {
    const val VERSION=9
    const val VECTOR_RECORD_BYTES=40
    const val HEADER_BYTES=144
    fun compositingOffset(plan:ByteBuffer):Int {
        scratchBytes(plan)
        val count=plan.getInt(132);val offset=plan.getInt(128);val total=plan.getInt(28)
        check(count==plan.getInt(8)&&count in 0..128&&plan.getInt(136)==20&&offset>=HEADER_BYTES&&offset.toLong()+count*20L<=total&&total<=plan.capacity()){"图层混合记录失效"}
        repeat(count){i->val p=offset+i*20;check(plan.getInt(p) in 0..14&&plan.getInt(p+4) in 0..1&&plan.getInt(p+8) in -1 until count&&plan.getInt(p+12) in 0..4&&plan.getInt(p+16) in 0..1){"图层混合值失效"}}
        return offset
    }
    fun scratchBytes(plan:ByteBuffer):Long {
        check(plan.capacity()>=HEADER_BYTES&&plan.getInt(0)==0x46584d53&&plan.getInt(4)==VERSION){"不兼容的帧预算协议"}
        val bytes=plan.getInt(76).toLong()
        check(bytes>0){"效果临时纹理预算失效"}
        return bytes
    }
}
