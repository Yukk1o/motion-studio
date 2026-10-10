package com.motionstudio.editor

import android.opengl.GLES30 as GL
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Backend adapter for plan v5. No editor UI or project mutation lives here. */
internal class GlLayerSources {
    private data class Source(val texture:Int,val width:Int,val height:Int,val fingerprint:Long)
    private val vectors=HashMap<Int,Source>()
    private val effectInputs=HashMap<Int,Source>()
    private val accumulators=IntArray(2)
    private val rasterScratch=HashMap<Pair<Int,Int>,Int>()
    private val rasterResolve=HashMap<Pair<Int,Int>,Int>()
    private val groups=GlVectorGroups()
    private var width=0;private var height=0
    private var fbo=0;private var resolveFbo=0;private var vbo=0;private var vao=0
    private var vectorProgram=0;private var mixProgram=0;private var copyProgram=0
    private var clearVbo=0;private var clearVao=0;private var msaaDrawClear=false;private var msaaProbed=false
    var graphicsCapabilityReadbackBytes=0L;private set
    private val matrix=FloatArray(16)
    private val colorAttachment=intArrayOf(GL.GL_COLOR_ATTACHMENT0)
    init {
        try {
            vectorProgram=program(VECTOR_VERTEX,VECTOR_FRAGMENT)
            mixProgram=program(FULLSCREEN,MIX_FRAGMENT);copyProgram=program(FULLSCREEN,COPY_FRAGMENT)
            val ids=IntArray(1);GL.glGenFramebuffers(1,ids,0);fbo=ids[0]
            GL.glGenFramebuffers(1,ids,0);resolveFbo=ids[0]
            GL.glGenBuffers(1,ids,0);vbo=ids[0];GL.glBindBuffer(GL.GL_ARRAY_BUFFER,vbo)
            GL.glBufferData(GL.GL_ARRAY_BUFFER,262144*24,null,GL.GL_DYNAMIC_DRAW)
            GL.glGenVertexArrays(1,ids,0);vao=ids[0];GL.glBindVertexArray(vao)
            GL.glEnableVertexAttribArray(0);GL.glVertexAttribPointer(0,2,GL.GL_FLOAT,false,24,0)
            GL.glEnableVertexAttribArray(1);GL.glVertexAttribPointer(1,4,GL.GL_FLOAT,false,24,8)
            GL.glGenBuffers(1,ids,0);clearVbo=ids[0];GL.glBindBuffer(GL.GL_ARRAY_BUFFER,clearVbo)
            val clearVertices=ByteBuffer.allocateDirect(24).order(ByteOrder.nativeOrder())
            clearVertices.asFloatBuffer().put(floatArrayOf(-1f,1f,3f,1f,-1f,-3f))
            GL.glBufferData(GL.GL_ARRAY_BUFFER,24,clearVertices,GL.GL_STATIC_DRAW)
            GL.glGenVertexArrays(1,ids,0);clearVao=ids[0];GL.glBindVertexArray(clearVao)
            GL.glEnableVertexAttribArray(0);GL.glVertexAttribPointer(0,2,GL.GL_FLOAT,false,8,0)
            GL.glBindVertexArray(0)
        }catch(error:Throwable){close();throw error}
    }
    fun prepare(plan:ByteBuffer,hasAdjustment:Boolean,w:Int,h:Int,assetBytes:Long,scratchBytes:Long) {
        check(plan.getInt(4)==RenderPlanBudget.VERSION&&plan.getInt(96)==24&&plan.getInt(108)==RenderPlanBudget.VECTOR_RECORD_BYTES){"不兼容的矢量计划"}
        val table=plan.getInt(84);val count=plan.getInt(88);val total=plan.getInt(28)
        check(count in 0..128&&table>=128&&table.toLong()+count*RenderPlanBudget.VECTOR_RECORD_BYTES<=total){"矢量资源表失效"}
        if(count>0&&!msaaProbed) {
            val samples=IntArray(1);GL.glGetIntegerv(GL.GL_MAX_SAMPLES,samples,0);check(samples[0]>=4){"矢量描边需要 4x MSAA"}
            msaaDrawClear=!probeMsaaClear();graphicsCapabilityReadbackBytes=4;msaaProbed=true
        }
        val active=HashSet<Int>()
        for(i in 0 until count)active.add(plan.getInt(table+i*RenderPlanBudget.VECTOR_RECORD_BYTES))
        val stale=vectors.keys.filter{it !in active}
        for(key in stale){GL.glDeleteTextures(1,intArrayOf(vectors.remove(key)!!.texture),0)}
        val groupKeys=groups.required(plan);groups.retain(groupKeys)
        val groupBytes=groupKeys.sumOf{it.first.toLong()*it.second*20}
        val sizes=(0 until count).filter{!groups.grouped(plan,table+it*RenderPlanBudget.VECTOR_RECORD_BYTES)}.map{val p=table+it*RenderPlanBudget.VECTOR_RECORD_BYTES;plan.getInt(p+4) to plan.getInt(p+8)}.toSet()
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,fbo);GL.glFramebufferRenderbuffer(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_RENDERBUFFER,0)
        val unused=rasterScratch.keys.filter{it !in sizes}
        for(size in unused){GL.glDeleteRenderbuffers(1,intArrayOf(rasterScratch.remove(size)!!),0);rasterResolve.remove(size)?.let{GL.glDeleteTextures(1,intArrayOf(it),0)}}
        var resident=vectors.values.sumOf{it.width.toLong()*it.height*4}
        for(i in 0 until count) {
            val p=table+i*RenderPlanBudget.VECTOR_RECORD_BYTES;val layer=plan.getInt(p);val sw=plan.getInt(p+4);val sh=plan.getInt(p+8)
            val offset=plan.getInt(p+12);val vertices=plan.getInt(p+16)
            val fingerprint=(plan.getInt(p+20).toLong() and 0xffffffffL) or (plan.getInt(p+24).toLong() shl 32)
            check(layer in 0 until plan.getInt(8)&&vertices in 0..262144&&offset>=plan.getInt(92)&&offset.toLong()+vertices*24<=total){"矢量顶点范围失效"}
            val old=vectors[layer]
            if(old?.fingerprint==fingerprint&&old.width==sw&&old.height==sh)continue
            val cost=sw.toLong()*sh*4;val resize=old==null||old.width!=sw||old.height!=sh
            val previous=old?.let{it.width.toLong()*it.height*4}?:0
            val scratchCost=rasterScratch.keys.sumOf{it.first.toLong()*it.second*20}+groupBytes
            val newScratch=if(groups.grouped(plan,p)||sw to sh in rasterScratch)0L else cost*5
            check(assetBytes+resident+(if(resize)cost else 0L)+scratchCost+newScratch<=128L*1024*1024){"矢量纹理与 MSAA 资源超过 128 MiB"}
            val target=if(resize)texture(sw,sh,true)else old!!.texture
            try {
                if(groups.grouped(plan,p)){
                    groups.draw(plan,p,target,vectorProgram,copyProgram,vao,vbo,clearVao,msaaDrawClear)
                    if(resize)old?.let{GL.glDeleteTextures(1,intArrayOf(it.texture),0)}
                    vectors[layer]=Source(target,sw,sh,fingerprint);if(resize)resident=resident-previous+cost
                    continue
                }
                val samples=IntArray(1);GL.glGetIntegerv(GL.GL_MAX_SAMPLES,samples,0);check(samples[0]>=4){"矢量描边需要 4x MSAA"}
                val renderbuffer=rasterScratch[sw to sh]?:run {
                    val ids=IntArray(1);GL.glGenRenderbuffers(1,ids,0);GL.glBindRenderbuffer(GL.GL_RENDERBUFFER,ids[0])
                    GL.glRenderbufferStorageMultisample(GL.GL_RENDERBUFFER,4,GL.GL_RGBA8,sw,sh)
                    rasterScratch[sw to sh]=ids[0];ids[0]
                }
                GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,fbo);GL.glFramebufferRenderbuffer(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_RENDERBUFFER,renderbuffer)
                check(GL.glCheckFramebufferStatus(GL.GL_FRAMEBUFFER)==GL.GL_FRAMEBUFFER_COMPLETE){"矢量 MSAA 目标不可用"}
                GL.glViewport(0,0,sw,sh)
                GL.glDisable(GL.GL_DEPTH_TEST);GL.glDisable(GL.GL_CULL_FACE)
                if(msaaDrawClear) {
                    // Some GLES implementations clear only one MSAA sample.
                    // Replace all samples with a triangle when the startup probe detects it.
                    GL.glDisable(GL.GL_BLEND);GL.glBindVertexArray(clearVao);GL.glUseProgram(vectorProgram)
                    GL.glVertexAttrib4f(1,0f,0f,0f,0f);GL.glDrawArrays(GL.GL_TRIANGLES,0,3)
                }else {GL.glClearColor(0f,0f,0f,0f);GL.glClear(GL.GL_COLOR_BUFFER_BIT)}
                GL.glDisable(GL.GL_DEPTH_TEST);GL.glDisable(GL.GL_CULL_FACE);GL.glEnable(GL.GL_BLEND);GL.glBlendFunc(GL.GL_ONE,GL.GL_ONE_MINUS_SRC_ALPHA)
                GL.glUseProgram(vectorProgram);GL.glBindVertexArray(vao);GL.glBindBuffer(GL.GL_ARRAY_BUFFER,vbo)
                if(vertices>0){val data=plan.duplicate().order(ByteOrder.nativeOrder()).apply{position(offset);limit(offset+vertices*24)}.slice();GL.glBufferSubData(GL.GL_ARRAY_BUFFER,0,vertices*24,data);GL.glDrawArrays(GL.GL_TRIANGLES,0,vertices)}
                // Resolve coverage and premultiplied paint in linear space. Some
                // GLES drivers average encoded samples when resolving sRGB MSAA.
                val linearTarget=rasterResolve[sw to sh]?:texture(sw,sh,false).also{rasterResolve[sw to sh]=it}
                GL.glBindFramebuffer(GL.GL_DRAW_FRAMEBUFFER,resolveFbo);GL.glFramebufferTexture2D(GL.GL_DRAW_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_TEXTURE_2D,linearTarget,0)
                check(GL.glCheckFramebufferStatus(GL.GL_DRAW_FRAMEBUFFER)==GL.GL_FRAMEBUFFER_COMPLETE){"矢量解析目标不可用"}
                GL.glDisable(GL.GL_BLEND)
                GL.glBindFramebuffer(GL.GL_READ_FRAMEBUFFER,fbo);GL.glBlitFramebuffer(0,0,sw,sh,0,0,sw,sh,GL.GL_COLOR_BUFFER_BIT,GL.GL_NEAREST)
                GL.glInvalidateFramebuffer(GL.GL_READ_FRAMEBUFFER,1,colorAttachment,0)
                GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,resolveFbo);GL.glFramebufferTexture2D(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_TEXTURE_2D,target,0)
                GL.glBindVertexArray(0);GL.glUseProgram(copyProgram);GL.glUniform1f(GL.glGetUniformLocation(copyProgram,"opacity"),1f)
                GL.glUniform1i(GL.glGetUniformLocation(copyProgram,"image"),0);GL.glUniform1i(GL.glGetUniformLocation(copyProgram,"flipY"),0)
                GL.glActiveTexture(GL.GL_TEXTURE0);GL.glBindTexture(GL.GL_TEXTURE_2D,linearTarget);GL.glDrawArrays(GL.GL_TRIANGLES,0,3)
                check(GL.glGetError()==GL.GL_NO_ERROR){"矢量绘制失败"}
                if(resize)old?.let{GL.glDeleteTextures(1,intArrayOf(it.texture),0)}
                vectors[layer]=Source(target,sw,sh,fingerprint);if(resize)resident=resident-previous+cost
            }catch(error:Throwable){if(resize)GL.glDeleteTextures(1,intArrayOf(target),0);throw error}finally{GL.glBindVertexArray(0)}
        }
        val accumulatorBytes=if(hasAdjustment)w.toLong()*h*8 else 0
        check(accumulatorBytes<=128L*1024*1024){"调整图层累加器 ${accumulatorBytes/1048576.0} MiB 超过合成预算 128 MiB"}
        val budget=RenderPlanBudget.scratchBytes(plan)
        check(scratchBytes<=budget){"效果临时纹理 ${scratchBytes/1048576.0} MiB 超过临时纹理预算 ${budget/1048576.0} MiB"}
        if(!hasAdjustment||width!=w||height!=h) {
            GL.glDeleteTextures(2,accumulators,0);accumulators.fill(0);width=0;height=0
            if(hasAdjustment){width=w;height=h;for(i in 0..1)accumulators[i]=texture(w,h,false)}
        }
    }
    fun vector(layer:Int)=vectors[layer]?.texture?:error("矢量源缺失")
    fun resourceBytes():Long=groups.bytes()+vectors.values.sumOf{it.width.toLong()*it.height*4}+rasterScratch.keys.sumOf{it.first.toLong()*it.second*20}+effectInputs.values.sumOf{it.width.toLong()*it.height*4}
    fun prepareEffectInputs(plan:ByteBuffer,order:List<Int>,assetBytes:Long) {
        effectInputs.keys.filter{it !in order}.forEach{GL.glDeleteTextures(1,intArrayOf(effectInputs.remove(it)!!.texture),0)}
        for(layer in order) {
            val draw=plan.getInt(16)+layer*128;val last=plan.getFloat(draw+116).toInt()-1
            check(last>=0){"效果来源没有可保存的像素"};val pass=plan.getInt(20)+last*40;val w=plan.getInt(pass+16);val h=plan.getInt(pass+20)
            val old=effectInputs[layer];if(old?.width==w&&old.height==h)continue
            check(assetBytes+resourceBytes()+w.toLong()*h*4<=128L*1024*1024){"效果来源快照超过纹理预算"}
            val image=texture(w,h,true);old?.let{GL.glDeleteTextures(1,intArrayOf(it.texture),0)};effectInputs[layer]=Source(image,w,h,0)
        }
    }
    fun effectInput(layer:Int)=effectInputs[layer]?.texture?:error("效果来源快照缺失")
    fun hasEffectInput(layer:Int)=effectInputs.containsKey(layer)
    fun copyEffectInput(layer:Int,source:Int) {
        val dest=effectInputs.getValue(layer);target(source,dest.width,dest.height)
        GL.glBindTexture(GL.GL_TEXTURE_2D,dest.texture);GL.glCopyTexSubImage2D(GL.GL_TEXTURE_2D,0,0,0,0,0,dest.width,dest.height)
        check(GL.glGetError()==GL.GL_NO_ERROR){"效果来源 GPU 复制失败"}
    }
    fun restoreEffectInput(layer:Int,output:Int) {
        val source=effectInputs.getValue(layer);target(source.texture,source.width,source.height)
        GL.glBindTexture(GL.GL_TEXTURE_2D,output);GL.glCopyTexSubImage2D(GL.GL_TEXTURE_2D,0,0,0,0,0,source.width,source.height)
        check(GL.glGetError()==GL.GL_NO_ERROR){"效果来源复用失败"}
    }
    fun accumulatorBytes():Long=if(accumulators[0]!=0)width.toLong()*height*8 else 0L
    /** One synthetic pixel on first vector use; never reads animation frames back. */
    private fun probeMsaaClear():Boolean {
        val ids=IntArray(1);var renderbuffer=0;var image=0
        try {
            GL.glGenRenderbuffers(1,ids,0);renderbuffer=ids[0];GL.glBindRenderbuffer(GL.GL_RENDERBUFFER,renderbuffer)
            GL.glRenderbufferStorageMultisample(GL.GL_RENDERBUFFER,4,GL.GL_SRGB8_ALPHA8,4,4)
            GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,fbo);GL.glFramebufferRenderbuffer(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_RENDERBUFFER,renderbuffer)
            check(GL.glCheckFramebufferStatus(GL.GL_FRAMEBUFFER)==GL.GL_FRAMEBUFFER_COMPLETE){"MSAA 清除能力检测目标不可用"}
            image=texture(4,4,true)
            GL.glViewport(0,0,4,4);GL.glDisable(GL.GL_BLEND);GL.glDisable(GL.GL_DEPTH_TEST);GL.glDisable(GL.GL_CULL_FACE)
            GL.glBindVertexArray(clearVao);GL.glUseProgram(vectorProgram);GL.glVertexAttrib4f(1,1f,1f,1f,1f)
            GL.glDrawArrays(GL.GL_TRIANGLES,0,3)
            GL.glClearColor(0f,0f,0f,0f);GL.glClear(GL.GL_COLOR_BUFFER_BIT)
            GL.glBindFramebuffer(GL.GL_DRAW_FRAMEBUFFER,resolveFbo);GL.glFramebufferTexture2D(GL.GL_DRAW_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_TEXTURE_2D,image,0)
            check(GL.glCheckFramebufferStatus(GL.GL_DRAW_FRAMEBUFFER)==GL.GL_FRAMEBUFFER_COMPLETE){"MSAA 清除能力检测解析目标不可用"}
            GL.glBindFramebuffer(GL.GL_READ_FRAMEBUFFER,fbo);GL.glBlitFramebuffer(0,0,4,4,0,0,4,4,GL.GL_COLOR_BUFFER_BIT,GL.GL_NEAREST)
            GL.glBindFramebuffer(GL.GL_READ_FRAMEBUFFER,resolveFbo)
            val pixel=ByteBuffer.allocateDirect(4);GL.glReadPixels(1,1,1,1,GL.GL_RGBA,GL.GL_UNSIGNED_BYTE,pixel)
            check(GL.glGetError()==GL.GL_NO_ERROR){"MSAA 清除能力检测失败"}
            return (0..3).all{pixel.get(it).toInt()==0}
        }finally {
            GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,fbo);GL.glFramebufferRenderbuffer(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_RENDERBUFFER,0)
            GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,resolveFbo);GL.glFramebufferTexture2D(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_TEXTURE_2D,0,0)
            if(renderbuffer!=0)GL.glDeleteRenderbuffers(1,intArrayOf(renderbuffer),0)
            if(image!=0)GL.glDeleteTextures(1,intArrayOf(image),0)
            GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,0)
        }
    }
    fun accumulator(index:Int)=accumulators[index].also{check(it!=0)}
    fun target(texture:Int,w:Int,h:Int,clear:Boolean=false) {
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,resolveFbo);GL.glFramebufferTexture2D(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_TEXTURE_2D,texture,0)
        check(GL.glCheckFramebufferStatus(GL.GL_FRAMEBUFFER)==GL.GL_FRAMEBUFFER_COMPLETE){"图层合成目标不可用"}
        GL.glViewport(0,0,w,h)
        if(clear){GL.glClearColor(0f,0f,0f,0f);GL.glClear(GL.GL_COLOR_BUFFER_BIT)}
    }
    /** Turn the composition FBO into the SDK's top-row-first input in the other accumulator. */
    fun input(current:Int):Int {
        val dest=1-current;target(accumulator(dest),width,height)
        GL.glDisable(GL.GL_BLEND);GL.glBindVertexArray(0);GL.glUseProgram(copyProgram);GL.glUniform1f(GL.glGetUniformLocation(copyProgram,"opacity"),1f)
        GL.glUniform1i(GL.glGetUniformLocation(copyProgram,"image"),0);GL.glUniform1i(GL.glGetUniformLocation(copyProgram,"flipY"),1)
        GL.glActiveTexture(GL.GL_TEXTURE0);GL.glBindTexture(GL.GL_TEXTURE_2D,accumulator(current));GL.glDrawArrays(GL.GL_TRIANGLES,0,3)
        return accumulator(dest)
    }
    fun adjust(current:Int,filtered:Int,plan:ByteBuffer,base:Int):Int {
        val values=plan.asFloatBuffer();val next=1-current;target(accumulator(next),width,height)
        GL.glDisable(GL.GL_BLEND);GL.glBindVertexArray(0);GL.glUseProgram(mixProgram)
        values.position(base);values.get(matrix);GL.glUniformMatrix4fv(GL.glGetUniformLocation(mixProgram,"inverseModel"),1,false,matrix,0)
        GL.glUniform4f(GL.glGetUniformLocation(mixProgram,"region"),values.get(base+16),values.get(base+17),values.get(base+18),values.get(base+19))
        GL.glUniform3f(GL.glGetUniformLocation(mixProgram,"maskOpacity"),values.get(base+20),values.get(base+21),values.get(base+22))
        GL.glUniform2f(GL.glGetUniformLocation(mixProgram,"uvScale"),values.get(base+25),values.get(base+26))
        GL.glUniform2f(GL.glGetUniformLocation(mixProgram,"compositionSize"),plan.getInt(100).toFloat(),plan.getInt(104).toFloat())
        GL.glUniform1i(GL.glGetUniformLocation(mixProgram,"original"),0);GL.glUniform1i(GL.glGetUniformLocation(mixProgram,"filtered"),1)
        GL.glActiveTexture(GL.GL_TEXTURE0);GL.glBindTexture(GL.GL_TEXTURE_2D,accumulator(current))
        GL.glActiveTexture(GL.GL_TEXTURE1);GL.glBindTexture(GL.GL_TEXTURE_2D,filtered);GL.glDrawArrays(GL.GL_TRIANGLES,0,3)
        return next
    }
    fun composite(current:Int,flip:Boolean=false) {
        GL.glBindVertexArray(0);GL.glUseProgram(copyProgram);GL.glUniform1f(GL.glGetUniformLocation(copyProgram,"opacity"),1f)
        GL.glEnable(GL.GL_BLEND);GL.glBlendFunc(GL.GL_ONE,GL.GL_ONE_MINUS_SRC_ALPHA)
        GL.glUniform1i(GL.glGetUniformLocation(copyProgram,"image"),0);GL.glUniform1i(GL.glGetUniformLocation(copyProgram,"flipY"),if(flip)1 else 0)
        GL.glActiveTexture(GL.GL_TEXTURE0);GL.glBindTexture(GL.GL_TEXTURE_2D,accumulator(current));GL.glDrawArrays(GL.GL_TRIANGLES,0,3)
    }
    fun close() {
        groups.close()
        effectInputs.values.forEach{GL.glDeleteTextures(1,intArrayOf(it.texture),0)};effectInputs.clear()
        vectors.values.forEach{GL.glDeleteTextures(1,intArrayOf(it.texture),0)};vectors.clear();GL.glDeleteTextures(2,accumulators,0);accumulators.fill(0)
        GL.glDeleteRenderbuffers(rasterScratch.size,rasterScratch.values.toIntArray(),0);rasterScratch.clear()
        GL.glDeleteTextures(rasterResolve.size,rasterResolve.values.toIntArray(),0);rasterResolve.clear()
        GL.glDeleteFramebuffers(2,intArrayOf(fbo,resolveFbo),0);GL.glDeleteBuffers(2,intArrayOf(vbo,clearVbo),0);GL.glDeleteVertexArrays(2,intArrayOf(vao,clearVao),0)
        for(p in listOf(vectorProgram,mixProgram,copyProgram))GL.glDeleteProgram(p)
        fbo=0;resolveFbo=0;vbo=0;vao=0;clearVbo=0;clearVao=0;vectorProgram=0;mixProgram=0;copyProgram=0
    }
    private fun texture(w:Int,h:Int,srgb:Boolean):Int {
        val limit=IntArray(1);GL.glGetIntegerv(GL.GL_MAX_TEXTURE_SIZE,limit,0);check(w in 1..limit[0]&&h in 1..limit[0]){"图层源纹理超过设备尺寸"}
        val ids=IntArray(1);GL.glGenTextures(1,ids,0);GL.glBindTexture(GL.GL_TEXTURE_2D,ids[0])
        GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MIN_FILTER,GL.GL_LINEAR);GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MAG_FILTER,GL.GL_LINEAR)
        GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_WRAP_S,GL.GL_CLAMP_TO_EDGE);GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_WRAP_T,GL.GL_CLAMP_TO_EDGE)
        GL.glTexImage2D(GL.GL_TEXTURE_2D,0,if(srgb)GL.GL_SRGB8_ALPHA8 else GL.GL_RGBA8,w,h,0,GL.GL_RGBA,GL.GL_UNSIGNED_BYTE,null)
        return ids[0]
    }
    private fun program(v:String,f:String):Int {
        val shaders=ArrayList<Int>();var result=0
        try {for((kind,source)in listOf(GL.GL_VERTEX_SHADER to v,GL.GL_FRAGMENT_SHADER to f)){val s=GL.glCreateShader(kind);shaders.add(s);GL.glShaderSource(s,source);GL.glCompileShader(s);val ok=IntArray(1);GL.glGetShaderiv(s,GL.GL_COMPILE_STATUS,ok,0);check(ok[0]!=0){GL.glGetShaderInfoLog(s)}}
            result=GL.glCreateProgram();shaders.forEach{GL.glAttachShader(result,it)};GL.glLinkProgram(result);val ok=IntArray(1);GL.glGetProgramiv(result,GL.GL_LINK_STATUS,ok,0);check(ok[0]!=0){GL.glGetProgramInfoLog(result)};return result
        }catch(error:Throwable){GL.glDeleteProgram(result);throw error}finally{shaders.forEach{GL.glDeleteShader(it)}}
    }
    companion object {
        private const val VECTOR_VERTEX="""#version 300 es
        layout(location=0) in vec2 position;layout(location=1) in vec4 color;out vec4 paint;
        void main(){gl_Position=vec4(position.x,-position.y,0.,1.);paint=color;}"""
        private const val VECTOR_FRAGMENT="""#version 300 es
        precision highp float;in vec4 paint;out vec4 result;void main(){result=paint;}"""
        private const val FULLSCREEN="""#version 300 es
        out vec2 uv;
        void main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2))*2.-1.;gl_Position=vec4(p,0.,1.);uv=p*.5+.5;}"""
        private const val COPY_FRAGMENT="""#version 300 es
        precision highp float;uniform sampler2D image;uniform int flipY;uniform float opacity;in vec2 uv;out vec4 result;
        void main(){result=texture(image,vec2(uv.x,flipY==1?1.-uv.y:uv.y))*opacity;}"""
        private const val MIX_FRAGMENT="""#version 300 es
        precision highp float;uniform sampler2D original;uniform sampler2D filtered;uniform mat4 inverseModel;
        uniform vec4 region;uniform vec3 maskOpacity;uniform vec2 uvScale;uniform vec2 compositionSize;in vec2 uv;out vec4 result;
        void main(){vec2 pixel=vec2(uv.x,1.-uv.y)*compositionSize;
        vec2 local=(inverseModel*vec4(pixel.x-compositionSize.x*.5,compositionSize.y*.5-pixel.y,0.,1.)).xy;
        vec2 d=maskOpacity.xy*.5-abs(local);vec2 coverage=clamp(d/max(fwidth(local),vec2(.0001))+.5,0.,1.);
        vec2 q=(pixel-region.xy)/region.zw;vec4 b=texture(original,uv);vec4 f=texture(filtered,q*uvScale);
        if(any(lessThan(q,vec2(0.)))||any(greaterThan(q,vec2(1.))))f=vec4(0.);
        result=mix(b,f,coverage.x*coverage.y*maskOpacity.z);}"""
    }
}
