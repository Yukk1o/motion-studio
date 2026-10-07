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
import java.util.concurrent.atomic.AtomicLong
import java.util.concurrent.atomic.AtomicReference
import kotlin.math.pow

/** Frozen Rust sampling + an independent EGL compositor into MediaCodec's Surface.
 * Ordered geometry crosses JNI in reused buffers. Raster resources cross once. */
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
        var videoHandle=0L;var audioHandle=0L;var videoUploads=0L
        var complete=false;val times=ArrayList<Long>();var parameterBytes=0L;var vertexBytes=0L;var graphicsCapabilityReadbackBytes=0L
        val stopOutput=AtomicBoolean(false);val codecStopped=AtomicBoolean(false)
        val outputFailure=AtomicReference<Throwable>()
        val endSubmitted=AtomicLong(0);val surfaceSubmission=AtomicLong(0)
        var outputThread:Thread?=null
        val started=System.nanoTime()
        try {
            native=NativeBridge.create(root.absolutePath,projectJson);check(native!=0L){"冻结工程创建失败"}
            val planResponse=JSONObject(NativeBridge.renderPlanInfo(native));check(planResponse.optBoolean("ok")){planResponse.optString("error")}
            val planInfo=planResponse.getJSONObject("data")
            val composition=project.optString("composition_id","comp-main")
            if(planInfo.getBoolean("has_video"))videoHandle=nativeData(MediaBridge.freezeVideo(native)).getLong("handle")
            if(planInfo.getBoolean("has_audio"))audioHandle=nativeData(MediaBridge.freezeAudio(native)).getLong("handle")
            val videoBuffers=HashMap<Long,ByteBuffer>()
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
            var buffer=ByteBuffer.allocateDirect(planInfo.getInt("composition_bundle_buffer_hint")).order(ByteOrder.LITTLE_ENDIAN)
            val outputCodec=codec
            // A Surface submission can wait for the encoder to free its input queue.
            // Consume output independently so that backpressure cannot block both ends.
            outputThread=Thread({
                val outputInfo=MediaCodec.BufferInfo();var track=-1
                var lastOutput=System.nanoTime()
                try {while(!stopOutput.get()) {
                    check(!cancelled.get()){"导出已取消"}
                    val submitting=surfaceSubmission.get()
                    check(submitting==0L||System.nanoTime()-submitting<30_000_000_000L){"编码 Surface 提交超时"}
                    val index=outputCodec.dequeueOutputBuffer(outputInfo,5000L)
                    if(index==MediaCodec.INFO_TRY_AGAIN_LATER) {
                        val end=endSubmitted.get()
                        check(end==0L||System.nanoTime()-maxOf(lastOutput,end)<15_000_000_000L){"编码器未及时结束"}
                    } else if(index==MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                        check(!muxStarted){"编码格式重复变更"}
                        track=outputMuxer.addTrack(outputCodec.outputFormat);outputMuxer.start();muxStarted=true
                    } else if(index>=0) {
                        val output=outputCodec.getOutputBuffer(index)?:error("编码输出为空")
                        try {
                        if(outputInfo.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG!=0)outputInfo.size=0
                        if(outputInfo.size>0) {
                            check(muxStarted){"编码格式尚未就绪"}
                            output.position(outputInfo.offset);output.limit(outputInfo.offset+outputInfo.size)
                            outputMuxer.writeSampleData(track,output,outputInfo)
                            times.add(outputInfo.presentationTimeUs)
                        }
                        }finally{outputCodec.releaseOutputBuffer(index,false)}
                        lastOutput=System.nanoTime()
                        if(outputInfo.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM!=0)break
                    }
                }}catch(error:Throwable) {
                    outputFailure.compareAndSet(null,error)
                    // Release a producer that may already be waiting in eglSwapBuffers.
                    if(codecStopped.compareAndSet(false,true))runCatching{outputCodec.stop()}
                }
            },"motion-encoder-output").apply{start()}
            fun checkOutput() {outputFailure.get()?.let{throw it};check(!cancelled.get()){"导出已取消"}}
            for(frame in 0 until frames) {
                checkOutput()
                var bytes=CompositionBridge.sampleFrameBundleInto(native,composition,frame.toDouble(),buffer)
                if(bytes < -1) {
                    check(-bytes<=32*1024*1024){"合成帧计划超过 32 MiB"}
                    buffer=ByteBuffer.allocateDirect(-bytes).order(ByteOrder.LITTLE_ENDIAN)
                    bytes=CompositionBridge.sampleFrameBundleInto(native,composition,frame.toDouble(),buffer)
                }
                check(bytes>=32){nativeData(NativeBridge.state(native)).optString("renderError","合成帧计划失败")}
                parameterBytes+=bytes
                gpu.prepareBundle(buffer)
                val sequence=frame.toLong()+1
                if(videoHandle!=0L) {
                    val deadline=System.nanoTime()+30_000_000_000L
                    var ready:JSONObject
                    do {
                        checkOutput()
                        ready=nativeData(MediaBridge.requestFrozenCompositionFrame(videoHandle,frame.toDouble(),sequence))
                        if(ready.getString("state")=="pending"){check(System.nanoTime()<deadline){"视频画面读取超时"};Thread.sleep(5)}
                    }while(ready.getString("state")=="pending")
                    check(ready.getString("state")=="ready"){ready.optString("error","视频解码失败")}
                    for(i in 0 until buffer.getInt(8)) {
                        val d=32+i*80;val videoOffset=buffer.getInt(d+48)
                        for(j in 0 until buffer.getInt(d+52)) {
                            val v=videoOffset+j*40;val alias=buffer.getLong(v)
                            val w=buffer.getInt(v+28);val h=buffer.getInt(v+32)
                            val pixels=videoBuffers[alias]?.takeIf{it.capacity()==w*h*4}?:ByteBuffer.allocateDirect(w*h*4).also{videoBuffers[alias]=it}
                            val actual=nativeData(MediaBridge.readFrozenCompositionVideoInto(videoHandle,alias,sequence,pixels))
                            check(actual.getInt("bytes")==w*h*4){"视频像素长度错误"}
                            pixels.clear();gpu.uploadVideo(alias,w,h,pixels);videoUploads++
                        }
                    }
                }
                gpu.drawBundle(buffer)
                val ptsUs=(frame.toLong()*1_000_000L+fps/2)/fps
                checkOutput();surfaceSubmission.set(System.nanoTime())
                gpu.present(ptsUs*1000)
                surfaceSubmission.set(0);checkOutput();onProgress(frame+1,frames)
            }
            endSubmitted.set(System.nanoTime());codec.signalEndOfInputStream()
            outputThread.join(20_000);check(!outputThread.isAlive){"编码器未及时结束"};checkOutput()
            check(times.size==frames){"编码输出帧数不一致: "+times.size+" / "+frames}
            check(times.zipWithNext().all{it.second>it.first}){"输出时间戳顺序不正确"}
            outputMuxer.stop();muxStarted=false
            outputMuxer.release();muxer=null
            if(audioHandle!=0L)AudioMux.addAudio(file,audioHandle,frames.toLong()*48000/fps,cancelled)
            complete=true
        } finally {
            val cleanupErrors=ArrayList<String>()
            fun release(name:String,action:()->Unit) {runCatching(action).onFailure{cleanupErrors.add(name+": "+it.message)}}
            stopOutput.set(true)
            release("output thread") {
                outputThread?.join(2_000)
                if(outputThread?.isAlive==true){if(codecStopped.compareAndSet(false,true))codec?.stop();outputThread?.join(5_000)}
                check(outputThread?.isAlive!=true){"编码输出线程未结束"}
            }
            release("EGL"){graphicsCapabilityReadbackBytes=gpu?.graphicsCapabilityReadbackBytes?:0L;gpu?.close()};release("input Surface"){input?.release()}
            release("codec stop"){if(codecStopped.compareAndSet(false,true))codec?.stop()};release("codec release"){codec?.release()}
            if(muxStarted)release("muxer stop"){muxer?.stop()};release("muxer release"){muxer?.release()}
            if(videoHandle!=0L)release("video snapshot"){nativeData(MediaBridge.releaseFrozenVideo(videoHandle))}
            if(audioHandle!=0L)release("audio snapshot"){nativeData(MediaBridge.releaseFrozenAudio(audioHandle))}
            if(native!=0L)release("native session"){NativeBridge.destroy(native)}
            if(cleanupErrors.isNotEmpty())complete=false
            val elapsed=(System.nanoTime()-started)/1e9
            val report=JSONObject().put("environment","Android system codec")
                .put("codec",info.name).put("hardwareAccelerated",info.isHardwareAccelerated)
                .put("width",width).put("height",height).put("fps",fps).put("expectedFrames",frames)
                .put("encodedFrames",times.size).put("timestampsUs",JSONArray(times))
                .put("elapsedSeconds",elapsed).put("throughputFps",times.size/elapsed)
                .put("applicationFrameReadbacks",0).put("graphicsCapabilityReadbackBytes",graphicsCapabilityReadbackBytes).put("applicationFrameUploads",videoUploads).put("audioMuxed",audioHandle!=0L).put("colorStandard",colorStandard)
                .put("parameterTransferBytes",parameterBytes).put("vertexTransferBytes",vertexBytes)
                .put("geometrySampling",true).put("cancelled",cancelled.get()).put("completed",complete)
                .put("cleanupErrors",JSONArray(cleanupErrors))
                .put("copyScope","Application-level frame transfers only; driver/system internal copies are not measured")
            File(file.parentFile,file.nameWithoutExtension+"-report.json").writeText(report.toString(2))
            if(!complete)file.delete()
        }
        check(complete){"输出封装或资源释放失败，请查看导出记录"}
        return file
    }
}

internal class EglMovieRenderer(surface:Surface?,private val width:Int,private val height:Int,project:JSONObject,native:Long,planInfo:JSONObject) {
    private val display=EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY)
    private var context:EGLContext=EGL14.EGL_NO_CONTEXT
    private var window:EGLSurface=EGL14.EGL_NO_SURFACE
    private val textures=ArrayList<Int>()
    private var frameTexture:Int=0
    private var framebuffer:Int=0
    private var plane:Int=0
    private var presentProgram:Int=0
    private var vertexBuffer:Int=0
    private var vertexArray:Int=0
    private val clear=FloatArray(4)
    private var imageLocation=0;private var mvpLocation=0;private var colorLocation=0
    private var extentLocation=0;private var uvLocation=0;private var presentImageLocation=0
    private val matrix=FloatArray(16);private val color=FloatArray(4)
    private val videoTextures=HashMap<Long,Int>()
    private val compositionTextures=HashMap<Int,Triple<Int,Int,Int>>()
    private val videoSizes=HashMap<Long,Pair<Int,Int>>()
    private var imageBytes=4L
    val skippedSlots=HashSet<Int>()
    private var effects:GlEffects?=null
    private var layerSources:GlLayerSources?=null
    val graphicsCapabilityReadbackBytes:Long get()=layerSources?.graphicsCapabilityReadbackBytes?:0L
    init {
        try {
        val version=IntArray(2);check(EGL14.eglInitialize(display,version,0,version,1)){"EGL 初始化失败"}
        val attributes=intArrayOf(EGL14.EGL_RED_SIZE,8,EGL14.EGL_GREEN_SIZE,8,EGL14.EGL_BLUE_SIZE,8,EGL14.EGL_ALPHA_SIZE,8,
            EGL14.EGL_RENDERABLE_TYPE,0x0040,EGL14.EGL_SURFACE_TYPE,if(surface==null)EGL14.EGL_PBUFFER_BIT else EGL14.EGL_WINDOW_BIT,0x3142,1,EGL14.EGL_NONE)
        val configs=arrayOfNulls<EGLConfig>(1);val count=IntArray(1)
        check(EGL14.eglChooseConfig(display,attributes,0,configs,0,1,count,0)&&count[0]>0){"没有可编码的 EGL 配置"}
        context=EGL14.eglCreateContext(display,configs[0],EGL14.EGL_NO_CONTEXT,intArrayOf(EGL14.EGL_CONTEXT_CLIENT_VERSION,3,EGL14.EGL_NONE),0)
        check(context!=EGL14.EGL_NO_CONTEXT){"EGL 上下文创建失败"}
        window=if(surface==null)EGL14.eglCreatePbufferSurface(display,configs[0],intArrayOf(EGL14.EGL_WIDTH,width,EGL14.EGL_HEIGHT,height,EGL14.EGL_NONE),0)
            else EGL14.eglCreateWindowSurface(display,configs[0],surface,intArrayOf(EGL14.EGL_NONE),0)
        check(window!=EGL14.EGL_NO_SURFACE&&EGL14.eglMakeCurrent(display,window,window,context)){"编码 Surface 创建失败"}
        plane=program(PLANE_VERTEX,PLANE_FRAGMENT);presentProgram=program(PRESENT_VERTEX,PRESENT_FRAGMENT)
        val names=IntArray(1)
        GLES30.glGenVertexArrays(1,names,0);vertexArray=names[0];GLES30.glBindVertexArray(vertexArray)
        GLES30.glGenBuffers(1,names,0);vertexBuffer=names[0];GLES30.glBindBuffer(GLES30.GL_ARRAY_BUFFER,vertexBuffer)
        GLES30.glBufferData(GLES30.GL_ARRAY_BUFFER,65536*20,null,GLES30.GL_STREAM_DRAW)
        GLES30.glEnableVertexAttribArray(0);GLES30.glVertexAttribPointer(0,3,GLES30.GL_FLOAT,false,20,0)
        GLES30.glEnableVertexAttribArray(1);GLES30.glVertexAttribPointer(1,2,GLES30.GL_FLOAT,false,20,12)
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
        imageBytes=bytes
        effects=GlEffects(planInfo,native,textures)
        layerSources=GlLayerSources()
        frameTexture=texture(width,height,null)
        val fbo=IntArray(1);GLES30.glGenFramebuffers(1,fbo,0);framebuffer=fbo[0]
        GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,framebuffer)
        GLES30.glFramebufferTexture2D(GLES30.GL_FRAMEBUFFER,GLES30.GL_COLOR_ATTACHMENT0,GLES30.GL_TEXTURE_2D,frameTexture,0)
        check(GLES30.glCheckFramebufferStatus(GLES30.GL_FRAMEBUFFER)==GLES30.GL_FRAMEBUFFER_COMPLETE){"离屏合成目标不可用"}
        } catch(error:Throwable) {close();throw error}
    }
    fun prepareBundle(bundle:ByteBuffer) {
        check(bundle.getInt(0)==0x4243534d&&bundle.getInt(4)==1&&bundle.getInt(24)==80&&bundle.getInt(28)==40){"不兼容的合成计划"}
        val count=bundle.getInt(8);check(count in 1..64&&bundle.getInt(12) in 32..bundle.capacity()){ "合成计划范围失效" }
        val active=HashSet<Long>()
        for(i in 0 until count){val d=32+i*80;val offset=bundle.getInt(d+48);repeat(bundle.getInt(d+52)){active.add(bundle.getLong(offset+it*40))}}
        for(id in videoTextures.keys.filter{it !in active}) {
            val size=videoSizes.remove(id)!!;imageBytes-=size.first.toLong()*size.second*4
            GLES30.glDeleteTextures(1,intArrayOf(videoTextures.remove(id)!!),0)
        }
        val root=bundle.getInt(16)
        for(i in compositionTextures.keys.filter{it>=count||it==root}){
            val t=compositionTextures.remove(i)!!;imageBytes-=t.second.toLong()*t.third*4;GLES30.glDeleteTextures(1,intArrayOf(t.first),0)
        }
        for(i in 0 until count)if(i!=root) {
            val d=32+i*80;val w=bundle.getInt(d+16);val h=bundle.getInt(d+20);val previous=compositionTextures[i]
            if(previous==null||previous.second!=w||previous.third!=h) {
                val old=previous?.let{it.second.toLong()*it.third*4}?:0
                val cost=w.toLong()*h*4;check(imageBytes-old+cost<=128L*1024*1024){"嵌套合成纹理超过 128 MiB"}
                val next=texture(w,h,null);previous?.let{GLES30.glDeleteTextures(1,intArrayOf(it.first),0)}
                compositionTextures[i]=Triple(next,w,h);imageBytes=imageBytes-old+cost
            }
        }
    }
    fun drawBundle(bundle:ByteBuffer) {
        val count=bundle.getInt(8);val root=bundle.getInt(16)
        for(i in 0 until count) {
            val d=32+i*80;val dynamic=HashMap<Int,Int>()
            for(child in 0 until i){val c=32+child*80;if(bundle.getInt(c+8)==i)dynamic[bundle.getInt(c+12)]=compositionTextures.getValue(child).first}
            val videos=bundle.getInt(d+48)
            repeat(bundle.getInt(d+52)){j->val v=videos+j*40;dynamic[bundle.getInt(v+24)]=videoTextures.getValue(bundle.getLong(v))}
            val alpha=bundle.getFloat(d+36);val bg=FloatArray(4){c->if(c==3)alpha else linear(bundle.getFloat(d+24+c*4))*alpha}
            val offset=bundle.getInt(d+40);val bytes=bundle.getInt(d+44)
            val plan=bundle.duplicate().order(ByteOrder.LITTLE_ENDIAN).apply{position(offset);limit(offset+bytes)}.slice().order(ByteOrder.LITTLE_ENDIAN)
            draw(plan,if(i==root)frameTexture else compositionTextures.getValue(i).first,bundle.getInt(d+16),bundle.getInt(d+20),bg,dynamic,i!=root)
        }
        drawPresentation()
    }
    fun uploadVideo(slot:Long,w:Int,h:Int,pixels:ByteBuffer) {
        val previous=videoSizes[slot]
        if(previous==null) {
            imageBytes+=w.toLong()*h*4;check(imageBytes<=128L*1024*1024){"导出纹理超出预算"}
            videoTextures[slot]=texture(w,h,pixels);videoSizes[slot]=w to h
        }else {
            check(previous==w to h){"视频尺寸在解码中改变"}
            GLES30.glBindTexture(GLES30.GL_TEXTURE_2D,videoTextures.getValue(slot))
            GLES30.glTexSubImage2D(GLES30.GL_TEXTURE_2D,0,0,0,w,h,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,pixels)
        }
    }
    fun draw(buffer:ByteBuffer) {draw(buffer,frameTexture,width,height,clear,videoTextures.mapKeys{it.key.toInt()},false);drawPresentation()}
    private fun draw(buffer:ByteBuffer,target:Int,w:Int,h:Int,background:FloatArray,dynamic:Map<Int,Int>,flip:Boolean) {
        check(buffer.getInt(0)==0x46584d53&&buffer.getInt(4)==4){"不兼容的帧计划"}
        val total=buffer.getInt(28);val vertexOffset=buffer.getInt(60);val bytes=buffer.getInt(80)*20
        check(total in 80..buffer.capacity()&&vertexOffset>=112&&vertexOffset.toLong()+bytes<=total&&bytes>=0&&bytes%20==0&&bytes<=65536*20){"几何计划范围失效"}
        effects?.prepare(buffer)
        val values=buffer.asFloatBuffer()
        val hasAdjustment=(0 until buffer.getInt(8)).any{values.get(buffer.getInt(16)/4+it*32+31)==2f&&values.get(buffer.getInt(16)/4+it*32+28)<values.get(buffer.getInt(16)/4+it*32+29)}
        layerSources!!.prepare(buffer,hasAdjustment,w,h,imageBytes+effects!!.resourceBytes(),effects!!.scratchBytes())
        var accumulator=0
        if(hasAdjustment)layerSources!!.target(layerSources!!.accumulator(0),w,h,true)
        GLES30.glBindVertexArray(vertexArray);GLES30.glBindBuffer(GLES30.GL_ARRAY_BUFFER,vertexBuffer)
        val vertices=buffer.duplicate().apply{position(vertexOffset);limit(vertexOffset+bytes)}.slice()
        if(bytes>0)GLES30.glBufferSubData(GLES30.GL_ARRAY_BUFFER,0,bytes,vertices)
        if(!hasAdjustment) {
            GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,framebuffer);GLES30.glFramebufferTexture2D(GLES30.GL_FRAMEBUFFER,GLES30.GL_COLOR_ATTACHMENT0,GLES30.GL_TEXTURE_2D,target,0);GLES30.glViewport(0,0,w,h)
            GLES30.glClearColor(background[0],background[1],background[2],background[3]);GLES30.glClear(GLES30.GL_COLOR_BUFFER_BIT)
        }
        GLES30.glDisable(GLES30.GL_DEPTH_TEST);GLES30.glDisable(GLES30.GL_CULL_FACE)
        var materialized=-1
        val batchOffset=buffer.getInt(52);val count=buffer.getInt(56)
        check(batchOffset>=80&&count in 0..8192&&batchOffset.toLong()+count*12<=vertexOffset){"几何批次范围失效"}
        for(i in 0 until count) {
            val batch=batchOffset+i*12;val layer=buffer.getInt(batch)
            check(layer in 0 until buffer.getInt(8)){"图层计划索引失效"}
            val base=buffer.getInt(16)/4+layer*32
            val sourceKind=values.get(base+31).toInt()
            val asset=values.get(base+24).toInt();if(sourceKind==0&&asset in skippedSlots)continue
            val passStart=values.get(base+28).toInt();val passEnd=values.get(base+29).toInt()
            if(passStart<passEnd&&materialized!=layer) {
                val video=when(sourceKind){
                    2->layerSources!!.input(accumulator)
                    1->layerSources!!.vector(layer)
                    else->if(asset<0)dynamic[asset]?:error("视频画面未就绪")else null
                }
                effects!!.passes(buffer,passStart,passEnd,video);materialized=layer
            }
            if(sourceKind==2) {
                if(passStart<passEnd&&values.get(base+22)>0f)accumulator=layerSources!!.adjust(accumulator,effects!!.texture(0),buffer,base)
                continue
            }
            if(hasAdjustment)layerSources!!.target(layerSources!!.accumulator(accumulator),w,h)
            else {GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,framebuffer);GLES30.glViewport(0,0,w,h)}
            GLES30.glEnable(GLES30.GL_BLEND);GLES30.glBlendFunc(GLES30.GL_ONE,GLES30.GL_ONE_MINUS_SRC_ALPHA)
            GLES30.glBindVertexArray(vertexArray);GLES30.glUseProgram(plane);GLES30.glUniform1i(imageLocation,0)
            GLES30.glUniform2f(uvLocation,values.get(base+25),values.get(base+26))
            values.position(base);values.get(matrix);values.get(color)
            if(flip&&!hasAdjustment)for(k in intArrayOf(1,5,9,13))matrix[k]=-matrix[k]
            GLES30.glUniformMatrix4fv(mvpLocation,1,false,matrix,0);GLES30.glUniform4fv(colorLocation,1,color,0)
            GLES30.glUniform3f(extentLocation,values.get(base+20),values.get(base+21),values.get(base+22))
            val image=if(values.get(base+27)>=0f)effects!!.texture(values.get(base+27).toInt()) else if(sourceKind==1)layerSources!!.vector(layer) else if(asset<0)dynamic[asset]?:error("视频画面未就绪") else textures.getOrNull(asset)?:error("图片资源失效")
            GLES30.glActiveTexture(GLES30.GL_TEXTURE0);GLES30.glBindTexture(GLES30.GL_TEXTURE_2D,image)
            val first=buffer.getInt(batch+4);val size=buffer.getInt(batch+8)
            check(first>=0&&size>=0&&(first.toLong()+size)*20<=bytes){"几何顶点范围失效"}
            GLES30.glBlendFuncSeparate(GLES30.GL_ONE,if(values.get(base+23)>0.5f)GLES30.GL_ONE else GLES30.GL_ONE_MINUS_SRC_ALPHA,GLES30.GL_ONE,GLES30.GL_ONE_MINUS_SRC_ALPHA)
            GLES30.glDrawArrays(GLES30.GL_TRIANGLES,first,size)
        }
        if(hasAdjustment) {
            GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,framebuffer);GLES30.glFramebufferTexture2D(GLES30.GL_FRAMEBUFFER,GLES30.GL_COLOR_ATTACHMENT0,GLES30.GL_TEXTURE_2D,target,0);GLES30.glViewport(0,0,w,h)
            GLES30.glClearColor(background[0],background[1],background[2],background[3]);GLES30.glClear(GLES30.GL_COLOR_BUFFER_BIT)
            layerSources!!.composite(accumulator,flip)
        }
        check(GLES30.glGetError()==GLES30.GL_NO_ERROR){"GPU 合成节点错误"}
    }
    private fun drawPresentation() {
        GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,0);GLES30.glViewport(0,0,width,height);GLES30.glDisable(GLES30.GL_BLEND)
        GLES30.glUseProgram(presentProgram);GLES30.glActiveTexture(GLES30.GL_TEXTURE0);GLES30.glBindTexture(GLES30.GL_TEXTURE_2D,frameTexture)
        GLES30.glUniform1i(presentImageLocation,0);GLES30.glDrawArrays(GLES30.GL_TRIANGLES,0,3)
        check(GLES30.glGetError()==GLES30.GL_NO_ERROR){"GPU 导出通道错误"}
    }
    /** Optional diagnostic readback of the unencoded composition, bottom row first.
     * RGB is sRGB-encoded premultiplied linear color; alpha is coverage. */
    fun readPixelsInto(pixels:ByteBuffer) {
        check(pixels.isDirect&&pixels.capacity()>=width*height*4){"诊断像素缓冲区不足"}
        pixels.clear()
        GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,framebuffer)
        GLES30.glFramebufferTexture2D(GLES30.GL_FRAMEBUFFER,GLES30.GL_COLOR_ATTACHMENT0,GLES30.GL_TEXTURE_2D,frameTexture,0)
        GLES30.glReadPixels(0,0,width,height,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,pixels)
        check(GLES30.glGetError()==GLES30.GL_NO_ERROR){"诊断像素读取失败"}
        GLES30.glBindFramebuffer(GLES30.GL_FRAMEBUFFER,0)
    }
    fun present(nanos:Long) {
        check(EGLExt.eglPresentationTimeANDROID(display,window,nanos)){"帧时间戳提交失败"}
        check(EGL14.eglSwapBuffers(display,window)){"编码 Surface 提交失败"}
    }
    fun close() {
        if(context!=EGL14.EGL_NO_CONTEXT&&window!=EGL14.EGL_NO_SURFACE) {
            EGL14.eglMakeCurrent(display,window,window,context)
            effects?.close();effects=null
            layerSources?.close();layerSources=null
            GLES30.glDeleteProgram(plane);GLES30.glDeleteProgram(presentProgram)
            GLES30.glDeleteBuffers(1,intArrayOf(vertexBuffer),0);GLES30.glDeleteVertexArrays(1,intArrayOf(vertexArray),0)
            GLES30.glDeleteTextures(videoTextures.size,videoTextures.values.toIntArray(),0);videoTextures.clear()
            GLES30.glDeleteTextures(compositionTextures.size,compositionTextures.values.map{it.first}.toIntArray(),0);compositionTextures.clear()
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
        layout(location=0) in vec3 position; layout(location=1) in vec2 texCoord;
        uniform mat4 mvp; uniform vec2 uvScale; out vec2 uv;
        void main(){gl_Position=mvp*vec4(position,1.);gl_Position.z=gl_Position.z*2.-gl_Position.w;uv=texCoord*uvScale;}"""
        private const val PLANE_FRAGMENT="""#version 300 es
        precision highp float; uniform sampler2D image; uniform vec4 color; uniform vec3 extent; in vec2 uv; out vec4 result;
        void main(){vec4 s=texture(image,uv);float a=color.a*extent.z;result=vec4(s.rgb*color.rgb*a,s.a*a);}"""
        private const val PRESENT_VERTEX="""#version 300 es
        out vec2 uv;
        void main(){vec2 v=vec2(float((gl_VertexID<<1)&2)*2.-1.,float(gl_VertexID&2)*2.-1.);gl_Position=vec4(v,0.,1.);uv=v*.5+.5;}"""
        private const val PRESENT_FRAGMENT="""#version 300 es
        precision highp float;uniform sampler2D image;in vec2 uv;out vec4 result;
        vec3 encode(vec3 v){return mix(1.055*pow(max(v,vec3(0.)),vec3(1./2.4))-.055,12.92*v,lessThanEqual(v,vec3(.0031308)));}
        void main(){vec4 s=texture(image,uv);result=vec4(encode(s.rgb),s.a);}"""
    }
}
