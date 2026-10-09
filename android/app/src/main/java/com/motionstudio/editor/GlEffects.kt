package com.motionstudio.editor

import android.opengl.GLES30 as GL
import org.json.JSONObject
import java.nio.ByteBuffer
import java.nio.ByteOrder

/** SDK 1–3 adapter. Shader sources and pass/parameter layouts come from Rust. */
internal class GlEffects(info:JSONObject,native:Long,private val assets:List<Int>) {
    private data class Shader(val id:Int,val samplers:List<Triple<Int,Int,Int>>,val resources:IntArray,val sprite:Boolean,val additive:Boolean)
    private val shaders=ArrayList<Shader>()
    private val ownedTextures=ArrayList<Int>()
    private val pool=IntArray(8)
    private val luts=HashMap<Int,Pair<Int,ByteArray>>()
    private var packageBytes=0L
    private var poolWidth=0;private var poolHeight=0;private var poolMask=0
    private var poolSizes=IntArray(16)
    private var framebuffer=0;private var uniform=0;private var sprites=0;private var spriteVao=0
    init {
        try {
            check(info.getInt("version")==5&&info.getInt("headerBytes")==128&&info.getInt("uniformBytes")==624){"不兼容的效果渲染协议"}
            val ids=IntArray(1);GL.glGenFramebuffers(1,ids,0);framebuffer=ids[0]
            GL.glGenBuffers(1,ids,0);uniform=ids[0];GL.glBindBuffer(GL.GL_UNIFORM_BUFFER,uniform)
            GL.glBufferData(GL.GL_UNIFORM_BUFFER,624,null,GL.GL_DYNAMIC_DRAW)
            GL.glGenBuffers(1,ids,0);sprites=ids[0];GL.glBindBuffer(GL.GL_ARRAY_BUFFER,sprites)
            GL.glBufferData(GL.GL_ARRAY_BUFFER,65536*48,null,GL.GL_DYNAMIC_DRAW)
            GL.glGenVertexArrays(1,ids,0);spriteVao=ids[0]
            val white=texture(1,1,false,ByteBuffer.allocateDirect(4).put(byteArrayOf(-1,-1,-1,-1)).apply{flip()})
            ownedTextures.add(white)
            val identity=ByteBuffer.allocateDirect(1024).apply {for(i in 0..255)repeat(4){put(i.toByte())};flip()}
            val lut=texture(256,1,false,identity);ownedTextures.add(lut);luts[-1]=lut to ByteArray(0)
            val programs=info.getJSONArray("programs");var resourceBytes=info.getLong("assetBytes")
            val resourceCache=HashMap<String,Int>()
            for(index in 0 until programs.length()) {
                val program=programs.getJSONObject(index);val glsl=program.getJSONObject("glsl")
                val id=try{link(glsl.getString("vertex"),glsl.getString("fragment"))}catch(failure:Throwable){
                    error("效果程序 "+program.getString("key")+": "+failure.message)
                }
                val resources=IntArray(4){white}
                // Register immediately so failed reflection/resource loading still releases the program.
                val samplers=ArrayList<Triple<Int,Int,Int>>();shaders.add(Shader(id,samplers,resources,program.optBoolean("sprite"),program.optBoolean("additive")))
                val blocks=glsl.getJSONObject("blocks")
                for(name in blocks.keys()) {
                    val block=GL.glGetUniformBlockIndex(id,name)
                    if(block!=GL.GL_INVALID_INDEX) {
                        val size=IntArray(1);GL.glGetActiveUniformBlockiv(id,block,GL.GL_UNIFORM_BLOCK_DATA_SIZE,size,0)
                        check(size[0]==624){"效果参数布局不一致: "+program.getString("key")}
                        GL.glUniformBlockBinding(id,block,0)
                    }
                }
                GL.glUseProgram(id)
                val mappings=glsl.getJSONObject("textures")
                for(name in mappings.keys()) {
                    val binding=mappings.getJSONArray(name);val group=binding.getInt(0);val slot=binding.getInt(1)/2
                    val unit=if(group==1)slot else 3+slot
                    val location=GL.glGetUniformLocation(id,name)
                    if(location>=0){GL.glUniform1i(location,unit);samplers.add(Triple(unit,group,slot))}
                }
                val images=program.getJSONArray("resources")
                for(i in 0 until images.length()) {
                    val image=images.getJSONObject(i);val w=image.getInt("width");val h=image.getInt("height")
                    val key=program.getString("key").substringBefore(':')+":"+image.getString("path")
                    resources[i]=resourceCache[key]?:run {
                        packageBytes+=w.toLong()*h*4;resourceBytes+=w.toLong()*h*4;check(resourceBytes<=128L*1024*1024){"素材和效果资源超过 128 MiB"}
                        val pixels=NativeBridge.pluginPixels(native,index,i)?:error("效果资源读取失败")
                        val texture=texture(w,h,false,ByteBuffer.allocateDirect(pixels.size).put(pixels).apply{flip()})
                        ownedTextures.add(texture);resourceCache[key]=texture;texture
                    }
                }
            }
            check(GL.glGetError()==GL.GL_NO_ERROR){"效果程序初始化失败"}
        }catch(error:Throwable){close();throw error}
    }
    fun prepare(plan:ByteBuffer) {
        check(plan.getInt(0)==0x46584d53&&plan.getInt(4)==5){"不兼容的帧计划"}
        check(plan.getInt(28) in 128..plan.capacity()){"帧计划长度错误"}
        val spriteOffset=plan.getInt(64);val spriteCount=plan.getInt(68)
        check(plan.getInt(72)==48&&spriteCount in 0..65536&&spriteOffset>=128&&spriteOffset.toLong()+spriteCount.toLong()*48<=plan.getInt(28)){"粒子实例范围错误"}
        if(spriteCount>0) {
            val data=plan.duplicate().order(ByteOrder.nativeOrder()).apply{position(spriteOffset);limit(spriteOffset+spriteCount*48)}.slice()
            GL.glBindBuffer(GL.GL_ARRAY_BUFFER,sprites);GL.glBufferSubData(GL.GL_ARRAY_BUFFER,0,spriteCount*48,data)
        }
        val w=plan.getInt(32);val h=plan.getInt(36);val mask=plan.getInt(40)
        check(mask and 255==mask&&w>=0&&h>=0){"效果纹理描述错误"}
        // Capacities are derived from the existing v2 pass table, identically to Rust.
        val sizes=IntArray(16);val passBase=plan.getInt(20);val passCount=plan.getInt(12)
        check(passCount>=0&&passBase>=128&&passBase.toLong()+passCount.toLong()*40<=plan.getInt(28)){"效果 pass 表错误"}
        var actualMask=0
        for(i in 0 until passCount) {
            val p=passBase+i*40;val slot=plan.getInt(p+12);val pw=plan.getInt(p+16);val ph=plan.getInt(p+20)
            check(slot in 0..7&&pw in 1..w&&ph in 1..h){"效果纹理尺寸错误"}
            actualMask=actualMask or (1 shl slot)
            sizes[slot*2]=maxOf(sizes[slot*2],pw);sizes[slot*2+1]=maxOf(sizes[slot*2+1],ph)
        }
        check(mask==actualMask){"效果纹理槽位不一致"}
        val bytes=(0..7).sumOf{i->sizes[i*2].toLong()*sizes[i*2+1]*(if(i==7)8 else 4)}
        check(bytes<=64L*1024*1024){"效果临时纹理需要 ${bytes/1048576.0} MiB，超过 64 MiB"}
        if(w!=poolWidth||h!=poolHeight||mask!=poolMask||!sizes.contentEquals(poolSizes)) {
            GL.glDeleteTextures(8,pool,0);pool.fill(0);poolWidth=w;poolHeight=h;poolMask=mask
            poolSizes=sizes
            for(i in pool.indices)if(mask and (1 shl i)!=0)pool[i]=texture(sizes[i*2],sizes[i*2+1],i !in 1..3&&i!=7,null,i==7)
        }
        val lutBase=plan.getInt(44);val count=plan.getInt(48)
        for(i in 0 until count) {
            val offset=lutBase+i*1024;check(offset>=0&&offset+1024<=plan.getInt(28)){"LUT 范围错误"}
            val bytes=ByteArray(1024);plan.duplicate().apply{position(offset);get(bytes)}
            val previous=luts[i]
            if(previous==null){val data=ByteBuffer.allocateDirect(1024).put(bytes).apply{flip()};luts[i]=texture(256,1,false,data) to bytes}
            else if(!previous.second.contentEquals(bytes)) {
                GL.glBindTexture(GL.GL_TEXTURE_2D,previous.first)
                GL.glTexSubImage2D(GL.GL_TEXTURE_2D,0,0,0,256,1,GL.GL_RGBA,GL.GL_UNSIGNED_BYTE,ByteBuffer.allocateDirect(1024).put(bytes).apply{flip()})
                luts[i]=previous.first to bytes
            }
        }
    }
    fun resourceBytes():Long=packageBytes
    /** Sprite PNGs are effect inputs even when the emitting layer has no image source. */
    fun spriteAssetSlots(plan:ByteBuffer):Set<Int> {
        val count=plan.getInt(12);val base=plan.getInt(20);val result=HashSet<Int>()
        check(count>=0&&base>=128&&base.toLong()+count.toLong()*40<=plan.getInt(28)){"粒子图片 pass 表错误"}
        for(i in 0 until count) {
            val p=base+i*40;val shaderIndex=plan.getInt(p)
            check(shaderIndex in shaders.indices){"粒子图片程序索引错误"}
            if(shaders[shaderIndex].sprite)for(offset in intArrayOf(4,8)) {
                val index=plan.getInt(p+offset)
                if(index<0&&index>GlMasks.SOURCE_TOKEN) {
                    val slot=-index-1;check(slot in assets.indices){"粒子图片索引错误"}
                    if(slot>0)result.add(slot)
                }
            }
        }
        return result
    }
    fun scratchBytes():Long=(0..7).sumOf{i->poolSizes[i*2].toLong()*poolSizes[i*2+1]*(if(i==7)8 else 4)}
    fun texture(slot:Int):Int {check(slot in pool.indices&&pool[slot]!=0){"效果纹理索引错误"};return pool[slot]}
    fun passes(plan:ByteBuffer,start:Int,end:Int,videoTexture:Int?=null,maskTexture:((Int)->Int)?=null) {
        val count=plan.getInt(12);check(start>=0&&end in start..count){"效果 pass 范围错误"}
        val base=plan.getInt(20)
        // Materialize the current decoded video texture into the layer's effect chain.
        fun input(index:Int,sprite:Boolean)=when {
            index<=GlMasks.SOURCE_TOKEN->maskTexture?.invoke((GlMasks.SOURCE_TOKEN.toLong()-index).toInt())?:error("效果蒙版源缺失")
            index<0->(if(sprite)null else videoTexture)?:run{check(-index-1 in assets.indices){"效果图片索引错误"};assets[-index-1]}
            else->texture(index)
        }
        for(i in start until end) {
            val p=base+i*40;check(p>=128&&p+40<=plan.getInt(28)){"效果 pass 地址错误"}
            val shaderIndex=plan.getInt(p);check(shaderIndex in shaders.indices){"效果程序索引错误"}
            val shader=shaders[shaderIndex];val source=input(plan.getInt(p+8),shader.sprite);val previous=input(plan.getInt(p+4),shader.sprite)
            val targetSlot=plan.getInt(p+12);val target=texture(targetSlot);val w=plan.getInt(p+16);val h=plan.getInt(p+20)
            check(w in 1..poolSizes[targetSlot*2]&&h in 1..poolSizes[targetSlot*2+1]&&target!=source&&target!=previous){"效果输出目标错误"}
            val offset=plan.getInt(p+24);check(offset>=128&&offset+624<=plan.getInt(28)){"效果参数地址错误"}
            GL.glBindFramebuffer(GL.GL_FRAMEBUFFER,framebuffer)
            GL.glFramebufferTexture2D(GL.GL_FRAMEBUFFER,GL.GL_COLOR_ATTACHMENT0,GL.GL_TEXTURE_2D,target,0)
            check(GL.glCheckFramebufferStatus(GL.GL_FRAMEBUFFER)==GL.GL_FRAMEBUFFER_COMPLETE){"效果离屏目标不可用"}
            GL.glDisable(GL.GL_BLEND);GL.glViewport(0,0,w,h);GL.glClearColor(0f,0f,0f,0f);GL.glClear(GL.GL_COLOR_BUFFER_BIT)
            GL.glUseProgram(shader.id);GL.glBindBuffer(GL.GL_UNIFORM_BUFFER,uniform)
            val data=plan.duplicate().order(ByteOrder.nativeOrder()).apply{position(offset);limit(offset+624)}.slice()
            GL.glBufferSubData(GL.GL_UNIFORM_BUFFER,0,624,data);GL.glBindBufferBase(GL.GL_UNIFORM_BUFFER,0,uniform)
            val lutOffset=plan.getInt(p+28);val lutBase=plan.getInt(44)
            val lutIndex=if(lutOffset==-1)-1 else {
                check(lutOffset>=lutBase&&(lutOffset-lutBase)%1024==0){"LUT 地址错误"}
                ((lutOffset-lutBase)/1024).also{check(it in 0 until plan.getInt(48)){"LUT 索引错误"}}
            }
            val lut=luts[lutIndex]?.first?:luts[-1]!!.first
            for((unit,group,slot) in shader.samplers) {
                val image=if(group==1)when(slot){0->previous;1->source;else->lut}else shader.resources[slot]
                GL.glActiveTexture(GL.GL_TEXTURE0+unit);GL.glBindTexture(GL.GL_TEXTURE_2D,image)
            }
            if(shader.sprite) {
                val start=plan.getInt(p+32);val count=plan.getInt(p+36)
                check(start>=0&&count>=0&&start.toLong()+count<=plan.getInt(68)){"粒子绘制范围错误"}
                GL.glEnable(GL.GL_BLEND)
                GL.glBlendFuncSeparate(GL.GL_ONE,if(shader.additive)GL.GL_ONE else GL.GL_ONE_MINUS_SRC_ALPHA,GL.GL_ONE,GL.GL_ONE_MINUS_SRC_ALPHA)
                GL.glBindVertexArray(spriteVao);GL.glBindBuffer(GL.GL_ARRAY_BUFFER,sprites)
                for(attribute in 0..2) {
                    GL.glEnableVertexAttribArray(attribute);GL.glVertexAttribPointer(attribute,4,GL.GL_FLOAT,false,48,start*48+attribute*16)
                    GL.glVertexAttribDivisor(attribute,1)
                }
                GL.glDrawArraysInstanced(GL.GL_TRIANGLES,0,6,count)
                GL.glBindVertexArray(0);GL.glDisable(GL.GL_BLEND)
            } else GL.glDrawArrays(GL.GL_TRIANGLES,0,3)
            check(GL.glGetError()==GL.GL_NO_ERROR){"效果 pass $i 执行失败"}
        }
    }
    fun close() {
        shaders.forEach{GL.glDeleteProgram(it.id)};shaders.clear()
        GL.glDeleteTextures(8,pool,0);pool.fill(0)
        luts.filterKeys{it>=0}.values.forEach{GL.glDeleteTextures(1,intArrayOf(it.first),0)};luts.clear()
        GL.glDeleteTextures(ownedTextures.size,ownedTextures.toIntArray(),0);ownedTextures.clear()
        GL.glDeleteBuffers(1,intArrayOf(uniform),0);GL.glDeleteFramebuffers(1,intArrayOf(framebuffer),0)
        GL.glDeleteBuffers(1,intArrayOf(sprites),0);GL.glDeleteVertexArrays(1,intArrayOf(spriteVao),0)
        sprites=0;spriteVao=0;uniform=0;framebuffer=0
    }
    private fun texture(w:Int,h:Int,srgb:Boolean,data:ByteBuffer?,floating:Boolean=false):Int {
        val limit=IntArray(1);GL.glGetIntegerv(GL.GL_MAX_TEXTURE_SIZE,limit,0)
        check(w in 1..limit[0]&&h in 1..limit[0]){"效果纹理超过设备能力"}
        val id=IntArray(1);GL.glGenTextures(1,id,0);GL.glBindTexture(GL.GL_TEXTURE_2D,id[0])
        GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MIN_FILTER,GL.GL_LINEAR);GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MAG_FILTER,GL.GL_LINEAR)
        GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_WRAP_S,GL.GL_CLAMP_TO_EDGE);GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_WRAP_T,GL.GL_CLAMP_TO_EDGE)
        GL.glTexImage2D(GL.GL_TEXTURE_2D,0,if(floating)GL.GL_RGBA16F else if(srgb)GL.GL_SRGB8_ALPHA8 else GL.GL_RGBA8,w,h,0,GL.GL_RGBA,if(floating)GL.GL_HALF_FLOAT else GL.GL_UNSIGNED_BYTE,data)
        return id[0]
    }
    private fun link(vertex:String,fragment:String):Int {
        val shaderIds=ArrayList<Int>();var program=0
        try {
            for((type,source) in listOf(GL.GL_VERTEX_SHADER to vertex,GL.GL_FRAGMENT_SHADER to fragment)) {
                val id=GL.glCreateShader(type);shaderIds.add(id);GL.glShaderSource(id,source);GL.glCompileShader(id)
                val ok=IntArray(1);GL.glGetShaderiv(id,GL.GL_COMPILE_STATUS,ok,0);check(ok[0]!=0){GL.glGetShaderInfoLog(id)}
            }
            program=GL.glCreateProgram();shaderIds.forEach{GL.glAttachShader(program,it)};GL.glLinkProgram(program)
            val ok=IntArray(1);GL.glGetProgramiv(program,GL.GL_LINK_STATUS,ok,0);check(ok[0]!=0){GL.glGetProgramInfoLog(program)}
            return program
        }catch(error:Throwable){GL.glDeleteProgram(program);throw error}finally{shaderIds.forEach{GL.glDeleteShader(it)}}
    }
}
