package com.motionstudio.editor

import android.opengl.GLES30 as GL
import org.json.JSONObject
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** The plane and transfer programs are generated from the native WGSL. */
internal class GlCompositing(info:JSONObject) {
    private data class Program(val id:Int,val samplers:List<Pair<Int,Int>>)
    private data class Target(val id:Int,val width:Int,val height:Int)
    private val planes=ArrayList<Program>()
    private var blend:Program?=null
    private val mattes=HashMap<Pair<Int,Boolean>,Target>()
    private var source:Target?=null
    private var zero=0
    private var framebuffer=0
    private val buffers=IntArray(2)
    private val planeValues=ByteBuffer.allocateDirect(112).order(ByteOrder.nativeOrder())
    private val blendValues=ByteBuffer.allocateDirect(624).order(ByteOrder.nativeOrder())
    init {
        try {
            val ids=IntArray(1);GL.glGenFramebuffers(1,ids,0);framebuffer=ids[0]
            GL.glGenBuffers(2,buffers,0)
            for((i,bytes) in listOf(112,624).withIndex()){GL.glBindBuffer(GL.GL_UNIFORM_BUFFER,buffers[i]);GL.glBufferData(GL.GL_UNIFORM_BUFFER,bytes,null,GL.GL_DYNAMIC_DRAW)}
            val native=info.getJSONObject("compositing");val data=native.getJSONArray("planes")
            repeat(3){planes.add(program(data.getJSONObject(it),112,false))}
            blend=program(native.getJSONObject("blend"),624,true)
            zero=texture(1,1,true)
            GL.glBindTexture(GL.GL_TEXTURE_2D,zero)
            GL.glTexSubImage2D(GL.GL_TEXTURE_2D,0,0,0,1,1,GL.GL_RED,GL.GL_UNSIGNED_BYTE,ByteBuffer.allocateDirect(1).put(0).apply{flip()})
        }catch(error:Throwable){close();throw error}
    }
    fun coverageBytes()=1L+mattes.values.sumOf{it.width.toLong()*it.height}
    fun sourceBytes()=source?.let{it.width.toLong()*it.height*4}?:0L
    private fun base(plan:ByteBuffer,layer:Int)=plan.getInt(128)+layer*20
    fun visible(plan:ByteBuffer,layer:Int)=plan.getInt(base(plan,layer)+16)!=0
    fun transfer(plan:ByteBuffer,layer:Int)=base(plan,layer).let{plan.getInt(it)!=0||plan.getInt(it+4)!=0}
    fun active(plan:ByteBuffer)=(0 until plan.getInt(8)).any{visible(plan,it)&&transfer(plan,it)||plan.getInt(base(plan,it)+12)!=0}
    fun hasTransfer(plan:ByteBuffer)=(0 until plan.getInt(8)).any{visible(plan,it)&&transfer(plan,it)}
    fun prepare(plan:ByteBuffer,w:Int,h:Int,otherSourceBytes:Long) {
        val needed=HashSet<Pair<Int,Boolean>>()
        repeat(plan.getInt(8)){i->val p=base(plan,i);val source=plan.getInt(p+8);if(source>=0)needed.add(source to (plan.getInt(p+12)>=3))}
        val stale=mattes.keys.filter{it !in needed||mattes.getValue(it).width!=w||mattes.getValue(it).height!=h}
        for(key in stale)GL.glDeleteTextures(1,intArrayOf(mattes.remove(key)!!.id),0)
        check(otherSourceBytes+1+needed.size*w.toLong()*h<=128L*1024*1024){"轨道遮罩与图层源纹理超过 128 MiB"}
        for(key in needed)if(key !in mattes)mattes[key]=Target(texture(w,h,true),w,h)
        val useSource=hasTransfer(plan)
        if(source?.let{!useSource||it.width!=w||it.height!=h}==true){GL.glDeleteTextures(1,intArrayOf(source!!.id),0);source=null}
        if(useSource) {
            check(w.toLong()*h*12<=128L*1024*1024){"图层混合累加器超过 128 MiB"}
            if(source==null)source=Target(texture(w,h,false),w,h)
        }
    }
    private fun parent(plan:ByteBuffer,layer:Int):Int {
        val p=base(plan,layer);val owner=plan.getInt(p+8)
        return if(owner<0)zero else mattes.getValue(owner to (plan.getInt(p+12)>=3)).id
    }
    private fun flags(plan:ByteBuffer,layer:Int,masked:Boolean,flip:Boolean):Int {
        val mode=plan.getInt(base(plan,layer)+12)
        return (if(masked)1 else 0)+(if(mode!=0)2 else 0)+(if(mode==2||mode==4)4 else 0)+(if(flip)8 else 0)
    }
    fun extract(plan:ByteBuffer,w:Int,h:Int,vertexArray:Int,source:(Int)->Int) {
        val done=HashSet<Pair<Int,Boolean>>();val active=HashSet<Int>()
        fun visit(layer:Int,luma:Boolean) {
            val key=layer to luma;if(key in done)return
            check(active.add(layer)){"轨道遮罩形成循环"}
            val p=base(plan,layer);val owner=plan.getInt(p+8);if(owner>=0)visit(owner,plan.getInt(p+12)>=3)
            target(mattes.getValue(key).id,w,h,true);GL.glDisable(GL.GL_BLEND)
            plane(plan,layer,source(layer),0,w,h,vertexArray,false,true,if(luma)2 else 1)
            val table=plan.getInt(52)
            repeat(plan.getInt(56)){i->val b=table+i*12;if(plan.getInt(b)==layer)GL.glDrawArrays(GL.GL_TRIANGLES,plan.getInt(b+4),plan.getInt(b+8))}
            active.remove(layer);done.add(key)
        }
        repeat(plan.getInt(8)){layer->val p=base(plan,layer);val owner=plan.getInt(p+8);if(owner>=0)visit(owner,plan.getInt(p+12)>=3)}
    }
    fun sourceTarget(w:Int,h:Int){target(source!!.id,w,h,true)}
    fun plane(plan:ByteBuffer,layer:Int,image:Int,mask:Int,w:Int,h:Int,vertexArray:Int,flip:Boolean,snapshot:Boolean=false,entry:Int=0) {
        val data=plan.getInt(16)+layer*128
        planeValues.clear();repeat(28){planeValues.putFloat(plan.getFloat(data+it*4))};planeValues.flip()
        // Naga converts native Y to GL Y. Compensate to match the existing
        // composition FBO orientation; nested textures can explicitly flip.
        for(k in intArrayOf(1,5,9,13))planeValues.putFloat(k*4,-planeValues.getFloat(k*4)*(if(flip)-1f else 1f))
        if(snapshot)repeat(4){planeValues.putFloat(64+it*4,1f)}
        planeValues.putFloat(80,0f);planeValues.putFloat(84,0f)
        planeValues.putFloat(92,flags(plan,layer,mask!=0,flip).toFloat())
        planeValues.putFloat(96,if(snapshot)1f else plan.getFloat(data+100));planeValues.putFloat(100,if(snapshot)1f else plan.getFloat(data+104))
        planeValues.putFloat(104,w.toFloat());planeValues.putFloat(108,h.toFloat())
        use(planes[entry],buffers[0],planeValues,intArrayOf(image,if(mask==0)zero else mask,parent(plan,layer)))
        GL.glBindVertexArray(vertexArray)
    }
    fun blend(plan:ByteBuffer,layer:Int,current:Int,next:Int,w:Int,h:Int) {
        target(next,w,h,true);GL.glDisable(GL.GL_BLEND);GL.glBindVertexArray(0)
        blendValues.clear();repeat(156){blendValues.putFloat(0f)};blendValues.flip()
        for(offset in listOf(0,16,32,48)) {
            blendValues.putFloat(offset+8,w.toFloat());blendValues.putFloat(offset+12,h.toFloat())
        }
        blendValues.putFloat(0,w.toFloat());blendValues.putFloat(4,h.toFloat())
        blendValues.putFloat(104,1f);blendValues.putFloat(108,1f)
        val record=base(plan,layer);blendValues.putFloat(112,plan.getInt(record).toFloat());blendValues.putFloat(116,plan.getInt(record+4).toFloat())
        use(blend!!,buffers[1],blendValues,intArrayOf(current,source!!.id,source!!.id));GL.glDrawArrays(GL.GL_TRIANGLES,0,3)
    }
    private fun use(p:Program,buffer:Int,data:ByteBuffer,textures:IntArray) {
        GL.glUseProgram(p.id);GL.glBindBuffer(GL.GL_UNIFORM_BUFFER,buffer);data.position(0)
        GL.glBufferSubData(GL.GL_UNIFORM_BUFFER,0,data.limit(),data);GL.glBindBufferBase(GL.GL_UNIFORM_BUFFER,0,buffer)
        for((unit,slot) in p.samplers){GL.glActiveTexture(GL.GL_TEXTURE0+unit);GL.glBindTexture(GL.GL_TEXTURE_2D,textures[slot])}
    }
    private fun target(id:Int,w:Int,h:Int,clear:Boolean) {
        GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,framebuffer);GL.glFramebufferTexture2D(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_TEXTURE_2D,id,0)
        check(GL.glCheckFramebufferStatus(GL.GL_FRAMEBUFFER)==GL.GL_FRAMEBUFFER_COMPLETE){"轨道遮罩目标不可用"};GL.glViewport(0,0,w,h)
        if(clear){GL.glClearColor(0f,0f,0f,0f);GL.glClear(GL.GL_COLOR_BUFFER_BIT)}
    }
    private fun texture(w:Int,h:Int,red:Boolean):Int {
        val limit=IntArray(1);GL.glGetIntegerv(GL.GL_MAX_TEXTURE_SIZE,limit,0);check(w in 1..limit[0]&&h in 1..limit[0]){"合成资源超过设备纹理尺寸"}
        val ids=IntArray(1);GL.glGenTextures(1,ids,0);GL.glBindTexture(GL.GL_TEXTURE_2D,ids[0])
        GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MIN_FILTER,GL.GL_LINEAR);GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MAG_FILTER,GL.GL_LINEAR)
        GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_WRAP_S,GL.GL_CLAMP_TO_EDGE);GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_WRAP_T,GL.GL_CLAMP_TO_EDGE)
        GL.glTexImage2D(GL.GL_TEXTURE_2D,0,if(red)GL.GL_R8 else GL.GL_RGBA8,w,h,0,if(red)GL.GL_RED else GL.GL_RGBA,GL.GL_UNSIGNED_BYTE,null)
        return ids[0]
    }
    private fun program(glsl:JSONObject,bytes:Int,transfer:Boolean):Program {
        fun compile(type:Int,text:String):Int {val id=GL.glCreateShader(type);GL.glShaderSource(id,text);GL.glCompileShader(id);val ok=IntArray(1);GL.glGetShaderiv(id,GL.GL_COMPILE_STATUS,ok,0);if(ok[0]==0){val message=GL.glGetShaderInfoLog(id);GL.glDeleteShader(id);error(message)};return id}
        val v=compile(GL.GL_VERTEX_SHADER,glsl.getString("vertex"));val f=try{compile(GL.GL_FRAGMENT_SHADER,glsl.getString("fragment"))}catch(e:Throwable){GL.glDeleteShader(v);throw e}
        val id=GL.glCreateProgram();GL.glAttachShader(id,v);GL.glAttachShader(id,f);GL.glLinkProgram(id);GL.glDeleteShader(v);GL.glDeleteShader(f)
        val ok=IntArray(1);GL.glGetProgramiv(id,GL.GL_LINK_STATUS,ok,0)
        try {
            check(ok[0]!=0){GL.glGetProgramInfoLog(id)}
            for(name in glsl.getJSONObject("blocks").keys()){val index=GL.glGetUniformBlockIndex(id,name);if(index!=GL.GL_INVALID_INDEX){val size=IntArray(1);GL.glGetActiveUniformBlockiv(id,index,GL.GL_UNIFORM_BLOCK_DATA_SIZE,size,0);check(size[0]==bytes){"合成参数布局不一致"};GL.glUniformBlockBinding(id,index,0)}}
            GL.glUseProgram(id);val samplers=ArrayList<Pair<Int,Int>>();val mapping=glsl.getJSONObject("textures")
            for(name in mapping.keys()){val b=mapping.getJSONArray(name);val slot=if(transfer)b.getInt(1)/2 else b.getInt(0)-1;val unit=samplers.size;GL.glUniform1i(GL.glGetUniformLocation(id,name),unit);samplers.add(unit to slot)}
            return Program(id,samplers)
        }catch(e:Throwable){GL.glDeleteProgram(id);throw e}
    }
    fun close() {
        for(p in planes)GL.glDeleteProgram(p.id);planes.clear();blend?.let{GL.glDeleteProgram(it.id)};blend=null
        mattes.values.forEach{GL.glDeleteTextures(1,intArrayOf(it.id),0)};mattes.clear();source?.let{GL.glDeleteTextures(1,intArrayOf(it.id),0)};source=null
        GL.glDeleteTextures(1,intArrayOf(zero),0);zero=0;GL.glDeleteFramebuffers(1,intArrayOf(framebuffer),0);framebuffer=0
        GL.glDeleteBuffers(2,buffers,0);buffers.fill(0)
    }
}
