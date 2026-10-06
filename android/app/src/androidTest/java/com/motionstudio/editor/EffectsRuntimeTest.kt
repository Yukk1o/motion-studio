package com.motionstudio.editor

import android.graphics.BitmapFactory
import android.graphics.Color
import android.media.MediaMetadataRetriever
import android.opengl.EGL14
import android.opengl.GLES30
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID
import kotlin.math.abs
import kotlin.math.pow

@RunWith(AndroidJUnit4::class)
class EffectsRuntimeTest {
    private fun data(raw:String):JSONObject=JSONObject(raw).let{assertTrue(it.optString("error"),it.optBoolean("ok"));it.getJSONObject("data")}
    private fun root()=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"effects-test/"+UUID.randomUUID()+"/project").apply{mkdirs()}
    private fun corePackage(session:Long,version:String="1.1.0"):JSONObject {
        val packages=data(NativeBridge.plugin(session,"{\"op\":\"catalogue\"}")).getJSONArray("packages")
        return (0 until packages.length()).map{packages.getJSONObject(it)}.first {
            val manifest=it.getJSONObject("manifest")
            manifest.getString("id")=="com.motionstudio.effects.ae2021"&&manifest.getString("version")==version
        }
    }
    private fun add(session:Long,effect:String,version:String="1.1.0"):JSONObject {
        val pkg=corePackage(session,version)
        val m=pkg.getJSONObject("manifest")
        return data(NativeBridge.plugin(session,JSONObject().put("op","add").put("object",2).put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",pkg.getString("hash")).put("effect",effect).toString()))
    }
    @Test fun runtimeDependenciesBlockBothExportsAndKeepProjectData() {
        val root=root();val p=data(NativeBridge.projectTemplate(0));val session=NativeBridge.create(root.absolutePath,p.toString())
        assertTrue(NativeBridge.creationError(),session>0)
        try {
            val s=add(session,"tint");assertEquals(5,s.getJSONObject("project").getInt("version"))
            assertEquals(1,s.getJSONObject("project").getJSONArray("plugin_dependencies").length())
            val capture=data(NativeBridge.capture(session));assertTrue(File(capture.getString("path")).length()>64)
            val pkg=corePackage(session);val m=pkg.getJSONObject("manifest")
            val request=JSONObject().put("op","enable").put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",pkg.getString("hash")).put("enabled",false)
            data(NativeBridge.plugin(session,request.toString()))
            assertFalse(JSONObject(NativeBridge.capture(session)).getBoolean("ok"));assertFalse(JSONObject(NativeBridge.renderPlanInfo(session)).getBoolean("ok"))
            val failed=data(NativeBridge.state(session)).getJSONObject("project");assertEquals(1,failed.getJSONArray("layers").getJSONObject(1).getJSONArray("effects").length())
            data(NativeBridge.plugin(session,request.put("enabled",true).toString()));data(NativeBridge.renderPlanInfo(session))
        }finally{NativeBridge.destroy(session)}
    }
    @Test fun tintBlurCurvesAndWaveWarpMatchWgpuInEncodedFrames() {
        val root=root();val p=data(NativeBridge.projectTemplate(0)).put("width",256).put("height",256).put("frames",12)
        // Recenter the implicit camera and a single solid in a small test composition.
        p.getJSONObject("camera").put("created",false)
        val layers=p.getJSONArray("layers");val layer=layers.getJSONObject(1)
        layer.getJSONObject("transform").getJSONObject("position").put("value",JSONArray(listOf(128,128,0)))
        layer.put("size",JSONArray(listOf(128,128)));p.put("layers",JSONArray().put(layer))
        val session=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),session>0)
        try {
            for(effect in listOf("tint","gaussian_blur","curves","wave_warp"))add(session,effect)
            val project=data(NativeBridge.state(session)).getJSONObject("project")
            val effects=project.getJSONArray("layers").getJSONObject(0).getJSONArray("effects")
            val blur=effects.getJSONObject(1).getLong("id")
            data(NativeBridge.command(session,JSONObject().put("op","effect").put("object",2).put("action",JSONObject().put("kind","set").put("effect",blur).put("param","p0001").put("frame",0).put("value",JSONArray(listOf(8,0,0,0)))).toString()))
            val frozen=data(NativeBridge.state(session)).getJSONObject("project").toString()
            val capture=data(NativeBridge.capture(session));val reference=BitmapFactory.decodeFile(capture.getString("path"))
            val info=data(NativeBridge.renderPlanInfo(session));val buffer=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
            assertTrue(NativeBridge.sampleRenderPlanInto(session,0,buffer)>0);assertEquals(0x46584d53,buffer.getInt(0))
            val file=VideoExporter(root,frozen).run{done,_->if(done==3){
                // The export owns a separate registry snapshot; live disabling must not change it.
                val pkg=corePackage(session);val m=pkg.getJSONObject("manifest")
                data(NativeBridge.plugin(session,JSONObject().put("op","enable").put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",pkg.getString("hash")).put("enabled",false).toString()))
            }}
            val retriever=MediaMetadataRetriever();val decoded=try{retriever.setDataSource(file.absolutePath);retriever.getFrameAtIndex(0)!!}finally{retriever.release()}
            var sum=0L;var samples=0;var foreground=0L;var active=0
            for(y in 0 until 256 step 2)for(x in 0 until 256 step 2){val a=reference.getPixel(x,y);val b=decoded.getPixel(x,y);val d=abs(Color.red(a)-Color.red(b))+abs(Color.green(a)-Color.green(b))+abs(Color.blue(a)-Color.blue(b));sum+=d;samples+=3
                if(Color.red(a)>50||Color.green(a)>50||Color.blue(a)>50){foreground+=d;active+=3}}
            val report=JSONObject().put("meanRgb",sum.toDouble()/samples).put("foregroundMeanRgb",foreground.toDouble()/active.coerceAtLeast(1)).put("frames",12)
                .put("frozenPluginResources",true).put("device",android.os.Build.MODEL)
            File(root,"effects-report.json").writeText(report.toString(2))
            assertTrue(report.toString(),report.getDouble("meanRgb")<6&&report.getDouble("foregroundMeanRgb")<8)
            reference.recycle();decoded.recycle()
        }finally{NativeBridge.destroy(session)}
    }

    @Test fun disabledCurvesKeepTheirDataAndExportWithoutLutResources() {
        val root=root();val p=data(NativeBridge.projectTemplate(0)).put("width",64).put("height",64).put("frames",2)
        p.getJSONObject("camera").put("created",false)
        val layer=p.getJSONArray("layers").getJSONObject(1).put("size",JSONArray(listOf(64,64)))
        layer.getJSONObject("transform").getJSONObject("position").put("value",JSONArray(listOf(32,32,0)))
        p.put("layers",JSONArray().put(layer))
        val native=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),native>0)
        try {
            add(native,"curves")
            data(NativeBridge.command(native,"{\"op\":\"effect\",\"object\":2,\"action\":{\"kind\":\"enable\",\"effect\":1,\"enabled\":false}}"))
            val project=data(NativeBridge.state(native)).getJSONObject("project")
            val info=data(NativeBridge.renderPlanInfo(native));val buffer=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
            assertTrue(NativeBridge.sampleRenderPlanInto(native,0,buffer)>0);assertEquals(0,buffer.getInt(48))
            assertTrue(File(data(NativeBridge.capture(native)).getString("path")).isFile)
            assertTrue(VideoExporter(root,project.toString()).run{_,_->}.isFile)
            assertTrue(project.getJSONArray("layers").getJSONObject(0).getJSONArray("effects").getJSONObject(0).getJSONObject("params").getJSONObject("p0001").has("curve"))
        }finally{NativeBridge.destroy(native)}
    }

    @Test fun legacyFullHdPolarAndEdgeGlowUseExactCapacitiesAndExport() {
        val root=root();val p=data(NativeBridge.projectTemplate(0)).put("width",1080).put("height",1920).put("frames",2)
        p.getJSONObject("camera").put("created",false)
        val layer=p.getJSONArray("layers").getJSONObject(1).put("size",JSONArray(listOf(1080,1920)))
        layer.getJSONObject("transform").getJSONObject("position").put("value",JSONArray(listOf(540,960,0)))
        p.put("layers",JSONArray().put(layer))
        val native=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),native>0)
        try {
            add(native,"polar_coordinates");val added=add(native,"glow_edges")
            val effect=added.getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONArray("effects").getJSONObject(1).getLong("id")
            data(NativeBridge.command(native,JSONObject().put("op","effect").put("object",2).put("action",JSONObject().put("kind","set")
                .put("effect",effect).put("param","radius").put("frame",0).put("value",JSONArray(listOf(48,0,0,0)))).toString()))
            val info=data(NativeBridge.renderPlanInfo(native));val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
            assertTrue(NativeBridge.sampleRenderPlanInto(native,0,plan)>0)
            val sizes=IntArray(16);val base=plan.getInt(20)
            for(i in 0 until plan.getInt(12)) {
                val offset=base+i*40;val slot=plan.getInt(offset+12)
                sizes[slot*2]=maxOf(sizes[slot*2],plan.getInt(offset+16))
                sizes[slot*2+1]=maxOf(sizes[slot*2+1],plan.getInt(offset+20))
            }
            val bytes=(0..7).sumOf{i->sizes[i*2].toLong()*sizes[i*2+1]*4}
            assertEquals(119,plan.getInt(40));assertTrue(bytes<64L*1024*1024)
            val capture=data(NativeBridge.capture(native));assertTrue(File(capture.getString("path")).length()>64)
            val frozen=data(NativeBridge.state(native)).getJSONObject("project").toString()
            val started=android.os.SystemClock.elapsedRealtime()
            val file=VideoExporter(root,frozen).run{_,_->}
            assertTrue(file.length()>64)
            File(root,"full-hd-scratch-report.json").writeText(JSONObject().put("scratchBytes",bytes)
                .put("oldScratchBytes",1280L*2048*4*7).put("width",1080).put("height",1920)
                .put("exportMillis",android.os.SystemClock.elapsedRealtime()-started).toString(2))
        } finally {NativeBridge.destroy(native)}
    }

    @Test fun allFiftyNineEffectsCompileOnDeviceAndUnencodedGlesMatchesWgpu() {
        val root=root();val p=data(NativeBridge.projectTemplate(0)).put("width",64).put("height",64).put("frames",12).put("background",JSONArray(listOf(0,0,0,0)))
        p.getJSONObject("camera").put("created",false)
        val layer=p.getJSONArray("layers").getJSONObject(1);layer.put("size",JSONArray(listOf(64,64)))
        layer.getJSONObject("transform").getJSONObject("position").put("value",JSONArray(listOf(32,32,0)))
        val bitmap=android.graphics.Bitmap.createBitmap(64,64,android.graphics.Bitmap.Config.ARGB_8888)
        val pixels=IntArray(4096){i->val x=i%64;val y=i/64;val edge=minOf(x,y,63-x,63-y);Color.argb((edge*255/5).coerceAtMost(255),x*4,y*4,255-x*4)}
        bitmap.setPixels(pixels,0,64,0,0,64,64)
        val input=File(root,"assets/input.png").apply{parentFile!!.mkdirs()};input.outputStream().use{bitmap.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
        layer.put("content",JSONObject().put("kind","image").put("asset",1));p.put("layers",JSONArray().put(layer))
        p.put("assets",JSONArray().put(JSONObject().put("id",1).put("path","assets/input.png").put("width",64).put("height",64)))
        val native=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),native>0)
        val display=EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY);var context=EGL14.EGL_NO_CONTEXT;var surface=EGL14.EGL_NO_SURFACE;val textures=ArrayList<Int>();val reports=JSONArray()
        try {
            val version=IntArray(2);assertTrue(EGL14.eglInitialize(display,version,0,version,1))
            val configs=arrayOfNulls<android.opengl.EGLConfig>(1);val count=IntArray(1)
            assertTrue(EGL14.eglChooseConfig(display,intArrayOf(EGL14.EGL_RED_SIZE,8,EGL14.EGL_GREEN_SIZE,8,EGL14.EGL_BLUE_SIZE,8,EGL14.EGL_ALPHA_SIZE,8,EGL14.EGL_RENDERABLE_TYPE,0x0040,EGL14.EGL_SURFACE_TYPE,EGL14.EGL_PBUFFER_BIT,EGL14.EGL_NONE),0,configs,0,1,count,0))
            context=EGL14.eglCreateContext(display,configs[0],EGL14.EGL_NO_CONTEXT,intArrayOf(EGL14.EGL_CONTEXT_CLIENT_VERSION,3,EGL14.EGL_NONE),0)
            surface=EGL14.eglCreatePbufferSurface(display,configs[0],intArrayOf(EGL14.EGL_WIDTH,1,EGL14.EGL_HEIGHT,1,EGL14.EGL_NONE),0)
            assertTrue(EGL14.eglMakeCurrent(display,surface,surface,context))
            fun texture(w:Int,h:Int,bytes:ByteArray):Int {
                val id=IntArray(1);GLES30.glGenTextures(1,id,0);GLES30.glBindTexture(GLES30.GL_TEXTURE_2D,id[0]);GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_MIN_FILTER,GLES30.GL_LINEAR);GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_MAG_FILTER,GLES30.GL_LINEAR)
                GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_WRAP_S,GLES30.GL_CLAMP_TO_EDGE);GLES30.glTexParameteri(GLES30.GL_TEXTURE_2D,GLES30.GL_TEXTURE_WRAP_T,GLES30.GL_CLAMP_TO_EDGE)
                GLES30.glTexImage2D(GLES30.GL_TEXTURE_2D,0,GLES30.GL_SRGB8_ALPHA8,w,h,0,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,ByteBuffer.allocateDirect(bytes.size).put(bytes).apply{flip()});textures.add(id[0]);return id[0]
            }
            texture(1,1,byteArrayOf(-1,-1,-1,-1));texture(64,64,NativeBridge.assetPixels(native,1)!!)
            val pkg=corePackage(native,"1.3.0");val effects=pkg.getJSONObject("manifest").getJSONArray("effects")
            fun linear(v:Double)=if(v<=.04045)v/12.92 else ((v+.055)/1.055).pow(2.4)
            fun encode(v:Double)=if(v<=.0031308)v*12.92 else 1.055*v.pow(1/2.4)-.055
            for(index in 0 until effects.length()) {
                val name=effects.getJSONObject(index).getString("id");val added=add(native,name,"1.3.0")
                val instance=added.getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONArray("effects").getJSONObject(0).getLong("id")
                fun setParam(id:String,value:Double) {
                    data(NativeBridge.command(native,JSONObject().put("op","effect").put("object",2).put("action",JSONObject().put("kind","set").put("effect",instance).put("param",id).put("frame",0).put("value",JSONArray(listOf(value,0,0,0)))).toString()))
                }
                if(name=="motion_tile") {setParam("tile_width",50.0);setParam("output_width",200.0);setParam("phase",90.0);setParam("mirror",1.0)}
                if(name=="optics_compensation")setParam("fov",90.0)
                if(name=="spherize")setParam("radius",24.0)
                if(name=="simple_choker")setParam("choke",-3.0)
                if(name=="gaussian_blur")data(NativeBridge.command(native,JSONObject().put("op","effect").put("object",2).put("action",JSONObject().put("kind","set").put("effect",instance).put("param","p0001").put("frame",0).put("value",JSONArray(listOf(8,0,0,0)))).toString()))
                if(name=="curves") {
                    val channels=JSONArray().put(JSONArray("[[0,0],[0.5,0.7],[1,1]]"))
                    repeat(4){channels.put(JSONArray("[[0,0],[1,1]]"))}
                    data(NativeBridge.command(native,JSONObject().put("op","effect").put("object",2).put("action",JSONObject().put("kind","set_curve_object").put("effect",instance).put("param","p0001").put("frame",0).put("value",JSONObject().put("channels",channels))).toString()))
                }
                val timed=name in setOf("wave_warp","shake","grain","light_leak","film_damage","digital_damage","scan_lines")
                val frame=if(timed)7 else 0
                val info=data(NativeBridge.renderPlanInfo(native));val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder());assertTrue(NativeBridge.sampleRenderPlanInto(native,0,plan)>0)
                assertTrue(NativeBridge.sampleRenderPlanInto(native,frame,plan)>0)
                val capture=data(NativeBridge.capture(native));val reference=BitmapFactory.decodeFile(capture.getString("path"),BitmapFactory.Options().apply{inPremultiplied=false;inScaled=false})
                // wgpu's EGL backend may change the thread's current context while capturing.
                assertTrue(EGL14.eglMakeCurrent(display,surface,surface,context))
                val gl=GlEffects(info,native,textures)
                try {
                    gl.prepare(plan);gl.passes(plan,0,plan.getInt(12))
                    val pass=plan.getInt(20)+(plan.getInt(12)-1)*40;val w=plan.getInt(pass+16);val h=plan.getInt(pass+20)
                    val raw=ByteBuffer.allocateDirect(w*h*4);GLES30.glReadPixels(0,0,w,h,GLES30.GL_RGBA,GLES30.GL_UNSIGNED_BYTE,raw);assertEquals(GLES30.GL_NO_ERROR,GLES30.glGetError())
                    val dx=(w-64)/2;val dy=(h-64)/2;var rgb=0.0;var alpha=0.0
                    for(y in 0 until 64)for(x in 0 until 64) {
                        val offset=((y+dy)*w+x+dx)*4;val a=(raw.get(offset+3).toInt() and 255)/255.0;val expected=reference.getPixel(x,y)
                        val channels=intArrayOf(Color.red(expected),Color.green(expected),Color.blue(expected))
                        for(c in 0..2){val v=(raw.get(offset+c).toInt() and 255)/255.0;val straight=if(a==0.0)0.0 else encode((linear(v)/a).coerceIn(0.0,1.0))*255;rgb+=abs(straight-channels[c])}
                        alpha+=abs(a*255-Color.alpha(expected))
                    }
                    val record=JSONObject().put("effect",name).put("frame",frame).put("packageHash",pkg.getString("hash")).put("nonIdentityCurve",name=="curves").put("rgbMae",rgb/(4096*3)).put("alphaMae",alpha/4096).put("renderer",GLES30.glGetString(GLES30.GL_RENDERER));reports.put(record)
                    File(root,"gles-wgpu-report.json").writeText(reports.toString(2));assertTrue(record.toString(),record.getDouble("rgbMae")<=3&&record.getDouble("alphaMae")<=3)
                }finally{gl.close();reference.recycle()}
                data(NativeBridge.command(native,JSONObject().put("op","effect").put("object",2).put("action",JSONObject().put("kind","remove").put("effect",instance)).toString()))
            }
            assertEquals(59,reports.length())
        }finally {
            GLES30.glDeleteTextures(textures.size,textures.toIntArray(),0);EGL14.eglMakeCurrent(display,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_CONTEXT);EGL14.eglDestroySurface(display,surface);EGL14.eglDestroyContext(display,context);EGL14.eglTerminate(display);NativeBridge.destroy(native)
        }
    }

    @Test fun rectangleEffectsAndAnimatedTileBoundsMatchEncodedFrames() {
        val reports=JSONArray()
        for(effect in listOf("motion_tile","optics_compensation","spherize","cc_lens","cc_radial_fast_blur","simple_choker","solid_composite")) {
            val root=root();val p=data(NativeBridge.projectTemplate(0)).put("width",128).put("height",128).put("frames",12).put("background",JSONArray(listOf(0,0,0,1)))
            p.getJSONObject("camera").put("created",false)
            val layer=p.getJSONArray("layers").getJSONObject(1).put("size",JSONArray(listOf(32,32))).put("content",JSONObject().put("kind","image").put("asset",1))
            layer.getJSONObject("transform").getJSONObject("position").put("value",JSONArray(listOf(64,64,0)))
            p.put("layers",JSONArray().put(layer)).put("assets",JSONArray().put(JSONObject().put("id",1).put("path","assets/input.png").put("width",32).put("height",32)))
            File(root,"assets").mkdirs()
            val image=android.graphics.Bitmap.createBitmap(32,32,android.graphics.Bitmap.Config.ARGB_8888)
            val pixels=IntArray(1024){i->val x=i%32;val y=i/32;Color.argb(if(x<3||y<3||x>28||y>28)80 else 255,x*8,y*8,if((x/4+y/4)%2==0)220 else 30)}
            image.setPixels(pixels,0,32,0,0,32,32);File(root,"assets/input.png").outputStream().use{image.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};image.recycle()
            val native=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),native>0)
            try {
                add(native,effect,"1.3.0")
                fun set(param:String,value:Double,frame:Int=0)=data(NativeBridge.command(native,JSONObject().put("op","effect").put("object",2).put("action",JSONObject().put("kind","set").put("effect",1).put("param",param).put("frame",frame).put("value",JSONArray(listOf(value,0,0,0)))).toString()))
                when(effect) {
                    "motion_tile"->{set("output_width",200.0);set("output_height",200.0);set("mirror",1.0)
                        data(NativeBridge.command(native,"{\"op\":\"effect\",\"object\":2,\"action\":{\"kind\":\"animate\",\"effect\":1,\"param\":\"output_width\",\"frame\":0,\"enabled\":true}}"));set("output_width",300.0,11)}
                    "optics_compensation"->set("fov",50.0)
                    "spherize"->set("radius",25.0)
                    "cc_lens"->set("size",40.0)
                    "cc_radial_fast_blur"->set("amount",30.0)
                    "simple_choker"->set("choke",3.0)
                    "solid_composite"->set("opacity",50.0)
                }
                data(NativeBridge.seek(native,7.0))
                val reference=BitmapFactory.decodeFile(data(NativeBridge.capture(native)).getString("path"))
                val frozen=data(NativeBridge.state(native)).getJSONObject("project").toString()
                val file=VideoExporter(root,frozen).run{_,_->}
                val retriever=MediaMetadataRetriever();val decoded=try{retriever.setDataSource(file.absolutePath);assertEquals("12",retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_FRAME_COUNT));retriever.getFrameAtIndex(7)!!}finally{retriever.release()}
                var rgb=0L
                for(y in 0 until 128)for(x in 0 until 128) {val a=reference.getPixel(x,y);val b=decoded.getPixel(x,y);rgb+=abs(Color.red(a)-Color.red(b))+abs(Color.green(a)-Color.green(b))+abs(Color.blue(a)-Color.blue(b))}
                val report=JSONObject().put("effect",effect).put("rgbMae",rgb.toDouble()/(128*128*3)).put("frame",7).put("frames",12).put("animatedBounds",effect=="motion_tile")
                reports.put(report);File(root,"rectangle-mp4-report.json").writeText(report.toString(2));assertTrue(report.toString(),report.getDouble("rgbMae")<8)
                reference.recycle();decoded.recycle()
            }finally{NativeBridge.destroy(native)}
        }
        File(root(),"rectangle-mp4-summary.json").writeText(reports.toString(2));assertEquals(7,reports.length())
    }
}
