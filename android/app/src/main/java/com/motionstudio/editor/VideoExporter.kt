package com.motionstudio.editor

import android.media.*
import android.opengl.*
import android.view.Surface
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.pow

/** Frozen Rust sampling + an independent EGL compositor into MediaCodec's Surface.
 * Only small draw parameters cross JNI per frame. Raster resources cross once. */
class VideoExporter(private val root:File,private val projectJson:String) {
    val cancelled=AtomicBoolean(false)
    fun run(onProgress:(Int,Int)->Unit):File {
        val project=JSONObject(projectJson)
        val width=project.getInt("width");val height=project.getInt("height")
        val fps=project.getInt("fps");val frames=project.getInt("frames")
        val codecs=MediaCodecList(MediaCodecList.ALL_CODECS).codecInfos.filter{info->
            info.isEncoder&&info.supportedTypes.any{it.equals("video/avc",true)}&&
                runCatching{info.getCapabilitiesForType("video/avc").videoCapabilities.areSizeAndRateSupported(width,height,fps.toDouble())}.getOrDefault(false)
        }.sortedByDescending{it.isHardwareAccelerated}
        val info=codecs.firstOrNull()?:error("当前编码器不支持所选尺寸和帧率")
        // Legacy Google OMX's RGB Surface conversion is fixed to BT.601. Labeling
        // its samples BT.709 causes visible hue shifts. Codec2/hardware retain 709.
        val colorStandard=if(info.name.equals("OMX.google.h264.encoder",true))MediaFormat.COLOR_STANDARD_BT601_NTSC else MediaFormat.COLOR_STANDARD_BT709
        val file=File(root,"exports/motion-"+System.currentTimeMillis()+".mp4").apply{parentFile!!.mkdirs()}
        var codec:MediaCodec?=null;var muxer:MediaMuxer?=null;var input:Surface?=null
        var gpu:EglMovieRenderer?=null;var native=0L;var muxStarted=false
        var complete=false;val times=ArrayList<Long>();var parameterBytes=0L
        val started=System.nanoTime()
        try {
            native=NativeBridge.create(root.absolutePath,projectJson);check(native!=0L){"冻结工程创建失败"}
            val planResponse=JSONObject(NativeBridge.renderPlanInfo(native));check(planResponse.optBoolean("ok")){planResponse.optString("error")}
            val planInfo=planResponse.getJSONObject("data")
            codec=MediaCodec.createByCodecName(info.name)
            val format=MediaFormat.createVideoFormat("video/avc",width,height).apply {
                setInteger(MediaFormat.KEY_COLOR_FORMAT,MediaCodecInfo.CodecCapabilities.COLOR_FormatSurface)
                setInteger(MediaFormat.KEY_BIT_RATE,(width.toLong()*height*fps/8).coerceIn(2_000_000L,20_000_000L).toInt())
                setInteger(MediaFormat.KEY_FRAME_RATE,fps);setInteger(MediaFormat.KEY_I_FRAME_INTERVAL,1)
                setInteger(MediaFormat.KEY_MAX_B_FRAMES,0)
                setInteger(MediaFormat.KEY_COLOR_STANDARD,colorStandard)
                setInteger(MediaFormat.KEY_COLOR_RANGE,MediaFormat.COLOR_RANGE_LIMITED)
                setInteger(MediaFormat.KEY_COLOR_TRANSFER,MediaFormat.COLOR_TRANSFER_SDR_VIDEO)
            }
            codec.configure(format,null,null,MediaCodec.CONFIGURE_FLAG_ENCODE)
            input=codec.createInputSurface();codec.start()
            val outputMuxer=MediaMuxer(file.absolutePath,MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
            muxer=outputMuxer
            gpu=EglMovieRenderer(input,width,height,project,native,planInfo)
            val buffer=ByteBuffer.allocateDirect(planInfo.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
            val outputInfo=MediaCodec.BufferInfo();var track=-1
            fun drain(end:Boolean) {
                var lastOutput=System.nanoTime()
                while(true) {
                    check(!cancelled.get()){"导出已取消"}
                    val index=codec.dequeueOutputBuffer(outputInfo,if(end)5000L else 0L)
                    if(index==MediaCodec.INFO_TRY_AGAIN_LATER) {
                        if(!end)return
                        check(System.nanoTime()-lastOutput<15_000_000_000L){"编码器未及时结束"}
                    } else if(index==MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                        check(!muxStarted){"编码格式重复变更"}
                        track=outputMuxer.addTrack(codec.outputFormat);outputMuxer.start();muxStarted=true
                    } else if(index>=0) {
                        val output=codec.getOutputBuffer(index)?:error("编码输出为空")
                        if(outputInfo.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG!=0)outputInfo.size=0
                        if(outputInfo.size>0) {
                            check(muxStarted){"编码格式尚未就绪"}
                            output.position(outputInfo.offset);output.limit(outputInfo.offset+outputInfo.size)
                            outputMuxer.writeSampleData(track,output,outputInfo)
                            times.add(outputInfo.presentationTimeUs)
                        }
                        val eos=outputInfo.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM!=0
                        codec.releaseOutputBuffer(index,false);lastOutput=System.nanoTime()
                        if(eos)return
                    }
                }
            }
            for(frame in 0 until frames) {
                check(!cancelled.get()){"导出已取消"}
                val bytes=NativeBridge.sampleRenderPlanInto(native,frame,buffer);check(bytes>=0){
                    JSONObject(NativeBridge.state(native)).optJSONObject("data")?.optString("renderError")?.takeIf{it.isNotBlank()&&it!="null"}
                        ?:"效果帧计划失败（参数或设备预算超限）"
                }
                parameterBytes+=bytes
                gpu.draw(buffer,buffer.getInt(8))
                val ptsUs=(frame.toLong()*1_000_000L+fps/2)/fps
                gpu.present(ptsUs*1000)
                drain(false);onProgress(frame+1,frames)
            }
            codec.signalEndOfInputStream();drain(true)
            check(times.size==frames){"编码输出帧数不一致: "+times.size+" / "+frames}
            check(times.zipWithNext().all{it.second>it.first}){"输出时间戳顺序不正确"}
            outputMuxer.stop();muxStarted=false
            outputMuxer.release();muxer=null
            complete=true
        } finally {
            val cleanupErrors=ArrayList<String>()
            fun release(name:String,action:()->Unit) {runCatching(action).onFailure{cleanupErrors.add(name+": "+it.message)}}
            release("EGL"){gpu?.close()};release("input Surface"){input?.release()}
            release("codec stop"){codec?.stop()};release("codec release"){codec?.release()}
            if(muxStarted)release("muxer stop"){muxer?.stop()};release("muxer release"){muxer?.release()}
            if(native!=0L)release("native session"){NativeBridge.destroy(native)}
            if(cleanupErrors.isNotEmpty())complete=false
            val elapsed=(System.nanoTime()-started)/1e9
            val report=JSONObject().put("environment","Android system codec")
                .put("codec",info.name).put("hardwareAccelerated",info.isHardwareAccelerated)
                .put("width",width).put("height",height).put("fps",fps).put("expectedFrames",frames)
                .put("encodedFrames",times.size).put("timestampsUs",JSONArray(times))
                .put("elapsedSeconds",elapsed).put("throughputFps",times.size/elapsed)
                .put("applicationFrameReadbacks",0).put("applicationFrameUploads",0)
                .put("parameterTransferBytes",parameterBytes).put("cancelled",cancelled.get()).put("completed",complete)
                .put("cleanupErrors",JSONArray(cleanupErrors))
                .put("copyScope","Application-level frame transfers only; driver/system internal copies are not measured")
            File(file.parentFile,file.nameWithoutExtension+"-report.json").writeText(report.toString(2))
            if(!complete)file.delete()
        }
        check(complete){"输出封装或资源释放失败，请查看导出记录"}
        return file
    }
}

private class EglMovieRenderer(surface:Surface,private val width:Int,private val height:Int,project:JSONObject,native:Long,planInfo:JSONObject) {
    private val display=EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY)
    private var context:EGLContext=EGL14.EGL_NO_CONTEXT
    private var window:EGLSurface=EGL14.EGL_NO_SURFACE
    private val textures=ArrayList<Int>()
    private var frameTexture:Int=0
    private var framebuffer:Int=0
    private var plane:Int=0
    private var presentProgram:Int=0
    private var imageLocation=0;private var mvpLocation=0;private var colorLocation=0
    private var extentLocation=0;private var uvLocation=0;private var presentImageLocation=0
    private val matrix=FloatArray(16);private val color=FloatArray(4)
    private val clear=FloatArray(4)
    private var effects:GlEffects?=null
    init {
        try {
        val version=IntArray(2);check(EGL14.eglInitialize(display,version,0,version,1)){"EGL 初始化失败"}
        val attributes=intArrayOf(EGL14.EGL_RED_SIZE,8,EGL14.EGL_GREEN_SIZE,8,EGL14.EGL_BLUE_SIZE,8,EGL14.EGL_ALPHA_SIZE,8,
            EGL14.EGL_RENDERABLE_TYPE,0x0040,EGL14.EGL_SURFACE_TYPE,EGL14.EGL_WINDOW_BIT,0x3142,1,EGL14.EGL_NONE)
        val configs=arrayOfNulls<EGLConfig>(1);val count=IntArray(1)
        check(EGL14.eglChooseConfig(display,attributes,0,configs,0,1,count,0)&&count[0]>0){"没有可编码的 EGL 配置"}
        context=EGL14.eglCreateContext(display,configs[0],EGL14.EGL_NO_CONTEXT,intArrayOf(EGL14.EGL_CONTEXT_CLIENT_VERSION,3,EGL14.EGL_NONE),0)
        check(context!=EGL14.EGL_NO_CONTEXT){"EGL 上下文创建失败"}
        window=EGL14.eglCreateWindowSurface(display,configs[0],surface,intArrayOf(EGL14.EGL_NONE),0)
        check(window!=EGL14.EGL_NO_SURFACE&&EGL14.eglMakeCurrent(display,window,window,context)){"编码 Surface 创建失败"}
        plane=program(PLANE_VERTEX,PLANE_FRAGMENT);presentProgram=program(PRESENT_VERTEX,PRESENT_FRAGMENT)
        imageLocation=GLES30.glGetUniformLocation(plane,"image");mvpLocation=GLES30.glGetUniformLocation(plane,"mvp")
        colorLocation=GLES30.glGetUniformLocation(plane,"color");extentLocation=GLES30.glGetUniformLocation(plane,"extent")
        uvLocation=GLES30.glGetUniformLocation(plane,"uvScale");presentImageLocation=GLES30.glGetUniformLocation(presentProgram,"image")
        val background=project.getJSONArray("background")
        clear[3]=background.getDouble(3).toFloat()
        for(i in 0..2)clear[i]=linear(background.getDouble(i).toFloat())*clear[3]
        textures.add(texture(1,1,ByteBuffer.allocateDirect(4).put(byteArrayOf(-1,-1,-1,-1)).apply{flip()}))
        val assets=project.getJSONArray("assets");var bytes=4L
        for(i in 0 until assets.length()) {
            val a=assets.getJSONObject(i);val w=a.getInt("width");val h=a.getInt("height")
            bytes+=w.toLong()*h*4;check(bytes<=128L*1024*1024){"导出纹理超出预算"}
            val data=NativeBridge.assetPixels(native,a.getLong("id"))?:error("图片资源读取失败")
            textures.add(texture(w,h,ByteBuffer.allocateDirect(data.size).put(data).apply{flip()}))
        }
        if(planInfo.getJSONArray("programs").length()>2)effects=GlEffects(planInfo,native,textures)
        frameTexture=texture(width,height,null)
        val fbo=IntArray(1);GLES30.glGenFramebuffers(1,fbo,0);framebuffer=fbo[0]
        GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,framebuffer)
        GLES30.glFramebufferTexture2D(GLES30.GL_FRAMEBUFFER,GLES30.GL_COLOR_ATTACHMENT0,GLES30.GL_TEXTURE_2D,frameTexture,0)
        check(GLES30.glCheckFramebufferStatus(GLES30.GL_FRAMEBUFFER)==GLES30.GL_FRAMEBUFFER_COMPLETE){"离屏合成目标不可用"}
        } catch(error:Throwable) {close();throw error}
    }
    fun draw(buffer:ByteBuffer,count:Int) {
        check(buffer.getInt(0)==0x46584d53&&buffer.getInt(4)==2&&buffer.getInt(28) in 64..buffer.capacity()){"不兼容的帧计划"}
        effects?.prepare(buffer)
        GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,framebuffer);GLES30.glViewport(0,0,width,height)
        GLES30.glClearColor(clear[0],clear[1],clear[2],clear[3]);GLES30.glClear(GLES30.GL_COLOR_BUFFER_BIT)
        GLES30.glDisable(GLES30.GL_DEPTH_TEST);GLES30.glDisable(GLES30.GL_CULL_FACE)
        GLES30.glEnable(GLES30.GL_BLEND);GLES30.glBlendFunc(GLES30.GL_ONE,GLES30.GL_ONE_MINUS_SRC_ALPHA)
        GLES30.glUseProgram(plane);GLES30.glUniform1i(imageLocation,0)
        val values=buffer.asFloatBuffer();var uvX=Float.NaN;var uvY=Float.NaN
        for(i in 0 until count) {
            val base=buffer.getInt(16)/4+i*32;
            val passStart=values.get(base+28).toInt();val passEnd=values.get(base+29).toInt()
            if(passStart<passEnd) {
                effects!!.passes(buffer,passStart,passEnd)
                GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,framebuffer);GLES30.glViewport(0,0,width,height)
                GLES30.glEnable(GLES30.GL_BLEND);GLES30.glBlendFunc(GLES30.GL_ONE,GLES30.GL_ONE_MINUS_SRC_ALPHA);GLES30.glUseProgram(plane)
                GLES30.glUniform1i(imageLocation,0)
            }
            val x=values.get(base+25);val y=values.get(base+26)
            if(x!=uvX||y!=uvY){GLES30.glUniform2f(uvLocation,x,y);uvX=x;uvY=y}
            values.position(base);values.get(matrix);values.get(color)
            GLES30.glUniformMatrix4fv(mvpLocation,1,false,matrix,0)
            GLES30.glUniform4fv(colorLocation,1,color,0)
            GLES30.glUniform3f(extentLocation,values.get(base+20),values.get(base+21),values.get(base+22))
            val asset=values.get(base+24).toInt();check(asset in textures.indices){"纹理索引失效"}
            GLES30.glActiveTexture(GLES30.GL_TEXTURE0);GLES30.glBindTexture(GLES30.GL_TEXTURE_2D,if(values.get(base+27)>=0f)effects!!.texture(values.get(base+27).toInt()) else textures[asset])
            GLES30.glBlendFuncSeparate(GLES30.GL_ONE,if(values.get(base+23)>0.5f)GLES30.GL_ONE else GLES30.GL_ONE_MINUS_SRC_ALPHA,GLES30.GL_ONE,GLES30.GL_ONE_MINUS_SRC_ALPHA)
            GLES30.glDrawArrays(GLES30.GL_TRIANGLES,0,6)
        }
        GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,0);GLES30.glDisable(GLES30.GL_BLEND)
        GLES30.glUseProgram(presentProgram);GLES30.glActiveTexture(GLES30.GL_TEXTURE0)
        GLES30.glBindTexture(GLES30.GL_TEXTURE_2D,frameTexture)
        GLES30.glUniform1i(presentImageLocation,0)
        GLES30.glDrawArrays(GLES30.GL_TRIANGLES,0,3)
        check(GLES30.glGetError()==GLES30.GL_NO_ERROR){"GPU 导出通道错误"}
    }
    fun present(nanos:Long) {
        check(EGLExt.eglPresentationTimeANDROID(display,window,nanos)){"帧时间戳提交失败"}
        check(EGL14.eglSwapBuffers(display,window)){"编码 Surface 提交失败"}
    }
    fun close() {
        if(context!=EGL14.EGL_NO_CONTEXT&&window!=EGL14.EGL_NO_SURFACE) {
            EGL14.eglMakeCurrent(display,window,window,context)
            effects?.close();effects=null
            GLES30.glDeleteProgram(plane);GLES30.glDeleteProgram(presentProgram)
            GLES30.glDeleteTextures(textures.size,textures.toIntArray(),0)
            GLES30.glDeleteTextures(1,intArrayOf(frameTexture),0);GLES30.glDeleteFramebuffers(1,intArrayOf(framebuffer),0)
        }
        EGL14.eglMakeCurrent(display,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_CONTEXT)
        EGL14.eglDestroySurface(display,window);EGL14.eglDestroyContext(display,context)
        EGL14.eglReleaseThread();EGL14.eglTerminate(display)
    }
    private fun texture(w:Int,h:Int,data:ByteBuffer?):Int {
        val limit=IntArray(1);GLES30.glGetIntegerv(GLES30.GL_MAX_TEXTURE_SIZE,limit,0)
        check(w<=limit[0]&&h<=limit[0]){"图片超过设备纹理尺寸"}
        val id=IntArray(1);GLES30.glGenTextures(1,id,0);GLES30.glBindTexture(GLES30.GL_TEXTURE_2D,id[0])
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_MIN_FILTER,GLES30.GL_LINEAR)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_MAG_FILTER,GLES30.GL_LINEAR)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_WRAP_S,GLES30.GL_CLAMP_TO_EDGE)
        GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_WRAP_T,GLES30.GL_CLAMP_TO_EDGE)
        GLES30.glTexImage2D(GLES30.GL_TEXTURE_2D,0,GLES30.GL_SRGB8_ALPHA8,w,h,0,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,data)
        return id[0]
    }
    private fun program(vertex:String,fragment:String):Int {
        fun compile(type:Int,code:String):Int {
            val shader=GLES30.glCreateShader(type);GLES30.glShaderSource(shader,code);GLES30.glCompileShader(shader)
            val ok=IntArray(1);GLES30.glGetShaderiv(shader,GLES30.GL_COMPILE_STATUS,ok,0)
            check(ok[0]!=0){GLES30.glGetShaderInfoLog(shader)};return shader
        }
        val v=compile(GLES30.GL_VERTEX_SHADER,vertex);val f=compile(GLES30.GL_FRAGMENT_SHADER,fragment)
        val p=GLES30.glCreateProgram();GLES30.glAttachShader(p,v);GLES30.glAttachShader(p,f);GLES30.glLinkProgram(p)
        val ok=IntArray(1);GLES30.glGetProgramiv(p,GLES30.GL_LINK_STATUS,ok,0)
        GLES30.glDeleteShader(v);GLES30.glDeleteShader(f);check(ok[0]!=0){GLES30.glGetProgramInfoLog(p)};return p
    }
    private fun linear(v:Float)=if(v<=.04045f)v/12.92f else ((v+.055f)/1.055f).pow(2.4f)
    companion object {
        private const val PLANE_VERTEX="""#version 300 es
        uniform mat4 mvp; uniform vec3 extent; uniform vec2 uvScale; out vec2 uv;
        const vec2 p[6]=vec2[6](vec2(-.5,.5),vec2(-.5,-.5),vec2(.5,.5),vec2(.5,.5),vec2(-.5,-.5),vec2(.5,-.5));
        void main(){vec2 v=p[gl_VertexID];gl_Position=mvp*vec4(v*extent.xy,0.,1.);gl_Position.z=gl_Position.z*2.-gl_Position.w;uv=vec2(v.x+.5,.5-v.y)*uvScale;}"""
        private const val PLANE_FRAGMENT="""#version 300 es
        precision highp float; uniform sampler2D image; uniform vec4 color; uniform vec3 extent; in vec2 uv; out vec4 result;
        void main(){vec4 s=texture(image,uv);float a=color.a*extent.z;result=vec4(s.rgb*color.rgb*a,s.a*a);}"""
        private const val PRESENT_VERTEX="""#version 300 es
        out vec2 uv;const vec2 p[3]=vec2[3](vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));
        void main(){vec2 v=p[gl_VertexID];gl_Position=vec4(v,0.,1.);uv=v*.5+.5;}"""
        private const val PRESENT_FRAGMENT="""#version 300 es
        precision highp float;uniform sampler2D image;in vec2 uv;out vec4 result;
        vec3 encode(vec3 v){return mix(1.055*pow(max(v,vec3(0.)),vec3(1./2.4))-.055,12.92*v,lessThanEqual(v,vec3(.0031308)));}
        void main(){vec4 s=texture(image,uv);result=vec4(encode(s.rgb),s.a);}"""
    }
}
