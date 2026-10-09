package com.motionstudio.editor

import android.opengl.GLES30 as GL
import org.json.JSONObject
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** Plan v5 coverage adapter. Raster geometry and feather/combine shaders come from Rust. */
internal class GlMasks(info:JSONObject) {
    data class Budget(val outputs:Long,val scratch:Long)
    private data class Record(val layer:Int,val width:Int,val height:Int,val offset:Int,val vertices:Int,
        val mode:Int,val inverted:Boolean,val opacity:Float,val featherX:Float,val featherY:Float,val signature:List<Int>)
    private data class Source(val texture:Int,val width:Int,val height:Int,val signature:List<Int>)
    private data class Filter(val id:Int,val samplers:MutableList<Pair<Int,Int>>)
    private val sources=HashMap<Int,Source>()
    private val filters=ArrayList<Filter>()
    private var groups:List<List<Record>> = emptyList()
    private var scratch=IntArray(0)
    private var scratchWidth=0;private var scratchHeight=0
    private var stagedWidth=0;private var stagedHeight=0;private var stagedSlots=0
    private var fbo=0;private var vbo=0;private var vao=0;private var uniform=0;private var raster=0
    private val uniforms=ByteBuffer.allocateDirect(624).order(ByteOrder.nativeOrder())
    init {
        try {
            check(info.getInt("version")==RenderPlanBudget.VERSION&&info.getInt("maskBytes")==64&&info.getInt("maskVertexBytes")==24&&info.getInt("maskSourceToken")==SOURCE_TOKEN){"不兼容的蒙版协议"}
            raster=program(VERTEX,FRAGMENT)
            val programs=info.getJSONArray("maskPrograms")
            check(programs.length()==2){"蒙版滤镜程序缺失"}
            for(i in 0 until programs.length()) {
                val glsl=programs.getJSONObject(i).getJSONObject("glsl")
                val id=program(glsl.getString("vertex"),glsl.getString("fragment"))
                val filter=Filter(id,ArrayList());filters.add(filter)
                val blocks=glsl.getJSONObject("blocks")
                for(name in blocks.keys()) {
                    val block=GL.glGetUniformBlockIndex(id,name)
                    if(block!=GL.GL_INVALID_INDEX) {
                        val bytes=IntArray(1);GL.glGetActiveUniformBlockiv(id,block,GL.GL_UNIFORM_BLOCK_DATA_SIZE,bytes,0)
                        check(bytes[0]==624){"蒙版滤镜参数布局失效"};GL.glUniformBlockBinding(id,block,0)
                    }
                }
                GL.glUseProgram(id)
                val samplers=glsl.getJSONObject("textures")
                for(name in samplers.keys()) {
                    val binding=samplers.getJSONArray(name);val group=binding.getInt(0);val slot=binding.getInt(1)/2
                    check(group==1&&slot in 0..2){"蒙版滤镜纹理布局失效"}
                    val location=GL.glGetUniformLocation(id,name)
                    if(location>=0){GL.glUniform1i(location,slot);filter.samplers.add(slot to slot)}
                }
            }
            val ids=IntArray(1);GL.glGenFramebuffers(1,ids,0);fbo=ids[0]
            GL.glGenBuffers(1,ids,0);uniform=ids[0];GL.glBindBuffer(GL.GL_UNIFORM_BUFFER,uniform)
            GL.glBufferData(GL.GL_UNIFORM_BUFFER,624,null,GL.GL_DYNAMIC_DRAW)
            GL.glGenBuffers(1,ids,0);vbo=ids[0];GL.glBindBuffer(GL.GL_ARRAY_BUFFER,vbo)
            GL.glBufferData(GL.GL_ARRAY_BUFFER,262144*24,null,GL.GL_DYNAMIC_DRAW)
            GL.glGenVertexArrays(1,ids,0);vao=ids[0];GL.glBindVertexArray(vao)
            GL.glEnableVertexAttribArray(0);GL.glVertexAttribPointer(0,2,GL.GL_FLOAT,false,24,0)
            GL.glEnableVertexAttribArray(1);GL.glVertexAttribPointer(1,4,GL.GL_FLOAT,false,24,8)
            GL.glBindVertexArray(0)
            check(GL.glGetError()==GL.GL_NO_ERROR){"蒙版资源初始化失败"}
        }catch(error:Throwable){close();throw error}
    }
    /** Drop obsolete node-local textures before the vector/effect adapters reserve resources. */
    fun stage(plan:ByteBuffer):Budget {
        check(plan.capacity()>=128&&plan.getInt(4)==RenderPlanBudget.VERSION&&plan.getInt(124)==64){"不兼容的蒙版计划"}
        val total=plan.getInt(28);val table=plan.getInt(112);val count=plan.getInt(116);val vertexBase=plan.getInt(120)
        check(total in 128..plan.capacity()&&count in 0..2048&&table>=128&&table.toLong()+count*64<=total&&vertexBase>=table+count*64&&vertexBase<=total){"蒙版记录范围失效"}
        val limit=IntArray(1);GL.glGetIntegerv(GL.GL_MAX_TEXTURE_SIZE,limit,0)
        val records=ArrayList<Record>();var verticesTotal=0L;var previousLayer=-1
        for(i in 0 until count) {
            val p=table+i*64;val layer=plan.getInt(p);val w=plan.getInt(p+12);val h=plan.getInt(p+16)
            val offset=plan.getInt(p+20);val vertices=plan.getInt(p+24)
            val mode=plan.getInt(p+36);val inverted=plan.getInt(p+40);val opacity=plan.getFloat(p+44)
            val fx=plan.getFloat(p+48);val fy=plan.getFloat(p+52)
            check(layer in 0 until plan.getInt(8)&&layer>=previousLayer&&w in 1..limit[0]&&h in 1..limit[0]){"蒙版图层或尺寸失效"}
            check(vertices in 0..262144&&vertices%3==0&&offset>=vertexBase&&offset.toLong()+vertices.toLong()*24<=total){"蒙版顶点范围失效"}
            check(mode in 0..6&&inverted in 0..1&&opacity.isFinite()&&opacity in 0f..1f&&fx.isFinite()&&fy.isFinite()&&fx>=0f&&fy>=0f){"蒙版采样值失效"}
            verticesTotal+=vertices;check(verticesTotal<=262144){"蒙版顶点超过帧预算"};previousLayer=layer
            // Geometry fingerprints exclude its address in the serialized frame.
            val signature=listOf(4,8,12,16,28,32,36,40,44,48,52).map{plan.getInt(p+it)}
            records.add(Record(layer,w,h,offset,vertices,mode,inverted==1,opacity,fx,fy,signature))
        }
        groups=records.groupBy{it.layer}.values.toList()
        for(group in groups)check(group.size<=16&&group.all{it.width==group[0].width&&it.height==group[0].height}){"同层蒙版尺寸或数量失效"}
        val active=groups.associate{it[0].layer to (it[0].width to it[0].height)}
        for(layer in sources.keys.filter{active[it]!=sources.getValue(it).let{s->s.width to s.height}}) {
            GL.glDeleteTextures(1,intArrayOf(sources.remove(layer)!!.texture),0)
        }
        stagedWidth=groups.maxOfOrNull{it[0].width}?:0;stagedHeight=groups.maxOfOrNull{it[0].height}?:0
        stagedSlots=groups.maxOfOrNull{g->1+(if(g.any{it.featherX>0f||it.featherY>0f})1 else 0)+(g.size-1).coerceAtMost(2)}?:0
        if(stagedWidth!=scratchWidth||stagedHeight!=scratchHeight||stagedSlots!=scratch.size) {
            GL.glDeleteTextures(scratch.size,scratch,0);scratch=IntArray(0);scratchWidth=0;scratchHeight=0
        }
        return Budget(groups.sumOf{it[0].width.toLong()*it[0].height},stagedWidth.toLong()*stagedHeight*stagedSlots)
    }
    fun prepare(plan:ByteBuffer,assetBytes:Long,otherScratch:Long) {
        val budget=stage(plan)
        check(assetBytes+budget.outputs<=128L*1024*1024){"蒙版和图层源纹理超过 128 MiB"}
        val scratchLimit=RenderPlanBudget.scratchBytes(plan)
        check(otherScratch+budget.scratch<=scratchLimit){"蒙版与效果临时纹理 ${(otherScratch+budget.scratch)/1048576.0} MiB 超过预算 ${scratchLimit/1048576.0} MiB"}
        if(stagedSlots>0&&scratch.isEmpty()) {
            scratch=IntArray(stagedSlots);scratchWidth=stagedWidth;scratchHeight=stagedHeight
            for(i in scratch.indices)scratch[i]=texture(scratchWidth,scratchHeight)
        }
        for(group in groups) {
            val first=group[0];val signature=group.flatMap{it.signature};val old=sources[first.layer]
            if(old?.signature==signature)continue
            val outputTexture=old?.texture?:texture(first.width,first.height).also{sources[first.layer]=Source(it,first.width,first.height,emptyList())}
            val blur=group.any{it.featherX>0f||it.featherY>0f};val accumulatorStart=1+if(blur)1 else 0
            var previous=-1
            for((i,m) in group.withIndex()) {
                target(scratch[0],m.width,m.height)
                GL.glEnable(GL.GL_BLEND);GL.glBlendFunc(GL.GL_ONE,GL.GL_ONE_MINUS_SRC_ALPHA)
                GL.glUseProgram(raster);GL.glBindVertexArray(vao);GL.glBindBuffer(GL.GL_ARRAY_BUFFER,vbo)
                if(m.vertices>0) {
                    val data=plan.duplicate().order(ByteOrder.nativeOrder()).apply{position(m.offset);limit(m.offset+m.vertices*24)}.slice()
                    GL.glBufferSubData(GL.GL_ARRAY_BUFFER,0,m.vertices*24,data);GL.glDrawArrays(GL.GL_TRIANGLES,0,m.vertices)
                }
                var coverage=0
                for(axis in 0..1) {
                    val feather=if(axis==0)m.featherX else m.featherY;if(feather<=0f)continue
                    val next=1-coverage;val sigma=feather*.25f
                    val params=floatArrayOf(if(axis==0)1f/m.width else 0f,if(axis==1)1f/m.height else 0f,sigma,maxOf(sigma*3f/16f,1f))
                    filter(0,m.width,m.height,params,scratch[coverage],scratch[coverage],scratch[next]);coverage=next
                }
                val slot=accumulatorStart+i%2
                val initial=if(m.mode in listOf(2,3,5))1f else 0f
                val params=floatArrayOf(m.mode.toFloat(),if(m.inverted)1f else 0f,m.opacity,if(i==0)initial else -1f)
                filter(1,m.width,m.height,params,if(previous<0)scratch[coverage]else scratch[previous],scratch[coverage],if(i==group.lastIndex)outputTexture else scratch[slot])
                previous=slot
            }
            sources[first.layer]=Source(outputTexture,first.width,first.height,signature)
        }
        GL.glBindVertexArray(0)
        check(GL.glGetError()==GL.GL_NO_ERROR){"蒙版绘制失败"}
    }
    fun textureFor(layer:Int)=sources[layer]?.texture?:error("蒙版源缺失")
    fun hasLayer(layer:Int)=sources.containsKey(layer)
    fun resourceBytes():Long=sources.values.sumOf{it.width.toLong()*it.height}
    fun scratchBytes():Long=scratchWidth.toLong()*scratchHeight*scratch.size
    private fun target(texture:Int,w:Int,h:Int) {
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,fbo);GL.glFramebufferTexture2D(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_TEXTURE_2D,texture,0)
        check(GL.glCheckFramebufferStatus(GL.GL_FRAMEBUFFER)==GL.GL_FRAMEBUFFER_COMPLETE){"蒙版 R8 目标不可用"}
        GL.glViewport(0,0,w,h);GL.glDisable(GL.GL_BLEND);GL.glDisable(GL.GL_DEPTH_TEST);GL.glDisable(GL.GL_CULL_FACE)
        GL.glClearColor(0f,0f,0f,0f);GL.glClear(GL.GL_COLOR_BUFFER_BIT)
    }
    private fun filter(index:Int,w:Int,h:Int,params:FloatArray,input:Int,source:Int,output:Int) {
        check(input!=output&&source!=output){"蒙版滤镜反馈目标失效"}
        target(output,w,h);GL.glBindVertexArray(0)
        val shader=filters[index];GL.glUseProgram(shader.id)
        val values=uniforms.asFloatBuffer();for(i in 0 until 156)values.put(i,0f)
        for(i in 0..3)values.put(i,if(i%2==0)w.toFloat()else h.toFloat())
        for(base in intArrayOf(4,8,12)){values.put(base+2,w.toFloat());values.put(base+3,h.toFloat())}
        values.put(26,1f);values.put(27,1f);for(i in 0..3)values.put(28+i,params[i])
        uniforms.position(0);GL.glBindBuffer(GL.GL_UNIFORM_BUFFER,uniform)
        GL.glBufferSubData(GL.GL_UNIFORM_BUFFER,0,624,uniforms);GL.glBindBufferBase(GL.GL_UNIFORM_BUFFER,0,uniform)
        for((unit,slot) in shader.samplers){GL.glActiveTexture(GL.GL_TEXTURE0+unit);GL.glBindTexture(GL.GL_TEXTURE_2D,if(slot==1)source else input)}
        GL.glDrawArrays(GL.GL_TRIANGLES,0,3)
    }
    private fun texture(w:Int,h:Int):Int {
        val ids=IntArray(1);GL.glGenTextures(1,ids,0);GL.glBindTexture(GL.GL_TEXTURE_2D,ids[0])
        GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MIN_FILTER,GL.GL_LINEAR);GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MAG_FILTER,GL.GL_LINEAR)
        GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_WRAP_S,GL.GL_CLAMP_TO_EDGE);GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_WRAP_T,GL.GL_CLAMP_TO_EDGE)
        GL.glTexImage2D(GL.GL_TEXTURE_2D,0,GL.GL_R8,w,h,0,GL.GL_RED,GL.GL_UNSIGNED_BYTE,null)
        return ids[0]
    }
    fun close() {
        sources.values.forEach{GL.glDeleteTextures(1,intArrayOf(it.texture),0)};sources.clear()
        GL.glDeleteTextures(scratch.size,scratch,0);scratch=IntArray(0);scratchWidth=0;scratchHeight=0
        filters.forEach{GL.glDeleteProgram(it.id)};filters.clear();GL.glDeleteProgram(raster)
        GL.glDeleteFramebuffers(1,intArrayOf(fbo),0);GL.glDeleteBuffers(2,intArrayOf(vbo,uniform),0);GL.glDeleteVertexArrays(1,intArrayOf(vao),0)
        raster=0;fbo=0;vbo=0;uniform=0;vao=0;groups=emptyList()
    }
    private fun program(v:String,f:String):Int {
        val shaders=ArrayList<Int>();var result=0
        try {
            for((kind,source) in listOf(GL.GL_VERTEX_SHADER to v,GL.GL_FRAGMENT_SHADER to f)) {
                val s=GL.glCreateShader(kind);shaders.add(s);GL.glShaderSource(s,source);GL.glCompileShader(s)
                val ok=IntArray(1);GL.glGetShaderiv(s,GL.GL_COMPILE_STATUS,ok,0);check(ok[0]!=0){GL.glGetShaderInfoLog(s)}
            }
            result=GL.glCreateProgram();shaders.forEach{GL.glAttachShader(result,it)};GL.glLinkProgram(result)
            val ok=IntArray(1);GL.glGetProgramiv(result,GL.GL_LINK_STATUS,ok,0);check(ok[0]!=0){GL.glGetProgramInfoLog(result)};return result
        }catch(error:Throwable){GL.glDeleteProgram(result);throw error}finally{shaders.forEach{GL.glDeleteShader(it)}}
    }
    companion object {
        const val SOURCE_TOKEN=-536870912
        private const val VERTEX="""#version 300 es
        layout(location=0) in vec2 position;layout(location=1) in vec4 color;out vec4 paint;
        void main(){gl_Position=vec4(position.x,-position.y,0.,1.);paint=color;}"""
        private const val FRAGMENT="""#version 300 es
        precision highp float;in vec4 paint;out vec4 result;void main(){result=paint;}"""
    }
}
