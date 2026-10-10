package com.motionstudio.editor

import android.opengl.GLES30 as GL
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Isolated linear MSAA groups. Reuses one target per size and nesting depth. */
internal class GlVectorGroups {
    private data class Target(val renderbuffer:Int,val resolved:Int)
    private val targets=HashMap<Triple<Int,Int,Int>,Target>()
    private var fbo=0
    private var resolveFbo=0
    private fun ensureFramebuffers(){if(fbo==0){val ids=IntArray(2);GL.glGenFramebuffers(2,ids,0);fbo=ids[0];resolveFbo=ids[1]}}
    fun bytes()=targets.keys.sumOf{it.first.toLong()*it.second*20}
    private fun depth(plan:ByteBuffer,p:Int):Int {
        val count=plan.getInt(p+32);val offset=plan.getInt(p+28);val total=plan.getInt(28);val vertices=plan.getInt(p+16)
        check(count in 0..69632&&offset>=RenderPlanBudget.HEADER_BYTES&&offset.toLong()+count*16L<=total){"矢量组指令范围失效"}
        val rootAlpha=plan.getFloat(p+36);check(rootAlpha.isFinite()&&rootAlpha in 0f..1f){"矢量组透明度失效"}
        var depth=0;var maximum=0
        repeat(count){i->val at=offset+i*16;val alpha=plan.getFloat(at+12)
            check(alpha.isFinite()&&alpha in 0f..1f){"矢量组透明度失效"}
            when(plan.getInt(at)) {
                0->{val a=plan.getInt(at+4);val b=plan.getInt(at+8);check(a in 0..b&&b<=vertices&&a%3==0&&b%3==0){"矢量绘制范围失效"}}
                1->{depth++;maximum=maxOf(maximum,depth);check(depth<=8){"矢量组过深"}}
                2->{depth--;check(depth>=0){"矢量组关闭顺序失效"}}
                else->error("未知矢量组指令")
            }
        }
        check(depth==0){"矢量组未关闭"};return maximum
    }
    fun grouped(plan:ByteBuffer,p:Int)=plan.getInt(p+32)>0||plan.getFloat(p+36)!=1f
    fun required(plan:ByteBuffer):Set<Triple<Int,Int,Int>> {
        val table=plan.getInt(84);val count=plan.getInt(88)
        return buildSet{repeat(count){i->val p=table+i*RenderPlanBudget.VECTOR_RECORD_BYTES
            val d=depth(plan,p)
            if(grouped(plan,p))for(n in 0..d)add(Triple(plan.getInt(p+4),plan.getInt(p+8),n))}}
    }
    fun retain(keys:Set<Triple<Int,Int,Int>>){for(k in targets.keys.filter{it !in keys})targets.remove(k)?.let{
        GL.glDeleteRenderbuffers(1,intArrayOf(it.renderbuffer),0);GL.glDeleteTextures(1,intArrayOf(it.resolved),0)}}
    fun draw(plan:ByteBuffer,p:Int,output:Int,vectorProgram:Int,copyProgram:Int,vao:Int,vbo:Int,clearVao:Int,msaaDrawClear:Boolean) {
        ensureFramebuffers()
        val w=plan.getInt(p+4);val h=plan.getInt(p+8);val max=depth(plan,p)
        fun target(d:Int):Target=targets.getOrPut(Triple(w,h,d)) {
            val ids=IntArray(1);GL.glGenRenderbuffers(1,ids,0);val rb=ids[0];GL.glBindRenderbuffer(GL.GL_RENDERBUFFER,rb)
            GL.glRenderbufferStorageMultisample(GL.GL_RENDERBUFFER,4,GL.GL_RGBA8,w,h)
            GL.glGenTextures(1,ids,0);val texture=ids[0];GL.glBindTexture(GL.GL_TEXTURE_2D,texture)
            GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MIN_FILTER,GL.GL_LINEAR);GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MAG_FILTER,GL.GL_LINEAR)
            GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_WRAP_S,GL.GL_CLAMP_TO_EDGE);GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_WRAP_T,GL.GL_CLAMP_TO_EDGE)
            GL.glTexImage2D(GL.GL_TEXTURE_2D,0,GL.GL_RGBA8,w,h,0,GL.GL_RGBA,GL.GL_UNSIGNED_BYTE,null)
            Target(rb,texture)
        }
        for(d in 0..max)target(d)
        fun bind(d:Int){GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,fbo);GL.glFramebufferRenderbuffer(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_RENDERBUFFER,target(d).renderbuffer)
            check(GL.glCheckFramebufferStatus(GL.GL_FRAMEBUFFER)==GL.GL_FRAMEBUFFER_COMPLETE){"矢量组 MSAA 目标失效"};GL.glViewport(0,0,w,h)}
        fun clear(d:Int){bind(d);GL.glDisable(GL.GL_BLEND)
            if(msaaDrawClear){GL.glUseProgram(vectorProgram);GL.glBindVertexArray(clearVao);GL.glVertexAttrib4f(1,0f,0f,0f,0f);GL.glDrawArrays(GL.GL_TRIANGLES,0,3)}
            else{GL.glClearColor(0f,0f,0f,0f);GL.glClear(GL.GL_COLOR_BUFFER_BIT)}}
        fun resolve(d:Int){bind(d);GL.glBindFramebuffer(GL.GL_DRAW_FRAMEBUFFER,resolveFbo);GL.glFramebufferTexture2D(GL.GL_DRAW_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_TEXTURE_2D,target(d).resolved,0)
            check(GL.glCheckFramebufferStatus(GL.GL_DRAW_FRAMEBUFFER)==GL.GL_FRAMEBUFFER_COMPLETE){"矢量组解析目标失效"}
            GL.glBindFramebuffer(GL.GL_READ_FRAMEBUFFER,fbo);GL.glDisable(GL.GL_BLEND);GL.glBlitFramebuffer(0,0,w,h,0,0,w,h,GL.GL_COLOR_BUFFER_BIT,GL.GL_NEAREST)}
        fun copy(texture:Int,alpha:Float){GL.glBindVertexArray(0);GL.glUseProgram(copyProgram);GL.glUniform1i(GL.glGetUniformLocation(copyProgram,"image"),0)
            GL.glUniform1i(GL.glGetUniformLocation(copyProgram,"flipY"),0);GL.glUniform1f(GL.glGetUniformLocation(copyProgram,"opacity"),alpha)
            GL.glActiveTexture(GL.GL_TEXTURE0);GL.glBindTexture(GL.GL_TEXTURE_2D,texture);GL.glDrawArrays(GL.GL_TRIANGLES,0,3)}
        GL.glDisable(GL.GL_DEPTH_TEST);GL.glDisable(GL.GL_CULL_FACE);clear(0)
        val offset=plan.getInt(p+12);val vertices=plan.getInt(p+16)
        GL.glBindBuffer(GL.GL_ARRAY_BUFFER,vbo)
        if(vertices>0){val data=plan.duplicate().order(ByteOrder.nativeOrder()).apply{position(offset);limit(offset+vertices*24)}.slice();GL.glBufferSubData(GL.GL_ARRAY_BUFFER,0,vertices*24,data)}
        var d=0;val base=plan.getInt(p+28)
        repeat(plan.getInt(p+32)){i->val at=base+i*16
            when(plan.getInt(at)) {
                0->{bind(d);GL.glEnable(GL.GL_BLEND);GL.glBlendFunc(GL.GL_ONE,GL.GL_ONE_MINUS_SRC_ALPHA);GL.glUseProgram(vectorProgram);GL.glBindVertexArray(vao)
                    val start=plan.getInt(at+4);GL.glDrawArrays(GL.GL_TRIANGLES,start,plan.getInt(at+8)-start)}
                1->{d++;clear(d)}
                2->{resolve(d);val child=target(d).resolved;d--;bind(d);GL.glEnable(GL.GL_BLEND);GL.glBlendFunc(GL.GL_ONE,GL.GL_ONE_MINUS_SRC_ALPHA);copy(child,plan.getFloat(at+12))}
            }
        }
        resolve(0);GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,resolveFbo);GL.glFramebufferTexture2D(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_TEXTURE_2D,output,0)
        GL.glDisable(GL.GL_BLEND);copy(target(0).resolved,plan.getFloat(p+36))
        check(GL.glGetError()==GL.GL_NO_ERROR){"矢量组绘制失败"}
    }
    fun close(){retain(emptySet());if(fbo!=0)GL.glDeleteFramebuffers(2,intArrayOf(fbo,resolveFbo),0);fbo=0;resolveFbo=0}
}
