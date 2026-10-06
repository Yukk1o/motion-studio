package com.motionstudio.editor

import android.graphics.BitmapFactory
import android.graphics.Color
import android.media.MediaMetadataRetriever
import android.opengl.EGL14
import android.opengl.GLES30 as GL
import android.util.Base64
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
class SceneEffectsTest {
    private fun data(raw:String)=JSONObject(raw).let{assertTrue(it.optString("error"),it.optBoolean("ok"));it.getJSONObject("data")}
    private fun root()=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"scene-effects-test/${UUID.randomUUID()}/project").apply{mkdirs()}
    private fun project():JSONObject {
        val p=data(NativeBridge.projectTemplate(0)).put("width",128).put("height",128).put("frames",12).put("background",JSONArray("[0,0,0,0]"))
        p.getJSONObject("camera").put("created",false)
        val l=p.getJSONArray("layers").getJSONObject(1).put("size",JSONArray("[128,128]")).put("content",JSONObject("{\"kind\":\"solid\",\"color\":[0,0,0,0]}"))
        l.getJSONObject("transform").getJSONObject("position").put("value",JSONArray("[64,64,0]"))
        return p.put("layers",JSONArray().put(l))
    }
    private fun pkg(native:Long):JSONObject {
        val packages=data(NativeBridge.plugin(native,"{\"op\":\"catalogue\"}")).getJSONArray("packages")
        return (0 until packages.length()).map{packages.getJSONObject(it)}.first{
            it.getJSONObject("manifest").getString("id")=="com.motionstudio.effects.scene" &&
            it.getJSONObject("manifest").getString("version")=="1.1.0"
        }
    }
    private fun add(native:Long,id:String):JSONObject {
        val p=pkg(native);val m=p.getJSONObject("manifest")
        return data(NativeBridge.plugin(native,JSONObject().put("op","add").put("object",2).put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",p.getString("hash")).put("effect",id).toString()))
    }
    private fun editor(native:Long,token:String,message:JSONObject)=data(NativeBridge.plugin(native,JSONObject().put("op","editor_message").put("token",token).put("message",message).toString()))
    @Test fun editorAssetsScopedEditsCancelAndGpuPreview() {
        val root=root();val native=NativeBridge.create(root.absolutePath,project().toString());assertTrue(NativeBridge.creationError(),native>0)
        try {
            add(native,"lens_flare")
            val opened=data(NativeBridge.plugin(native,"{\"op\":\"editor_open\",\"object\":2,\"instance\":1}"));val token=opened.getString("token")
            assertEquals(1,opened.getInt("protocol"));var state=opened.getJSONObject("state");assertEquals(7,state.getJSONObject("scene").getJSONArray("elements").length())
            val asset=data(NativeBridge.plugin(native,JSONObject().put("op","editor_asset").put("token",token).put("path","ui/editor.html").toString()))
            assertTrue(String(Base64.decode(asset.getString("base64"),Base64.DEFAULT),Charsets.UTF_8).contains("场景效果编辑器"))
            assertFalse(JSONObject(NativeBridge.plugin(native,JSONObject().put("op","editor_asset").put("token",token).put("path","shaders/sprite.wgsl").toString())).getBoolean("ok"))
            state=editor(native,token,JSONObject().put("op","begin").put("revision",state.getLong("revision")))
            for(v in listOf(2,3,4))state=editor(native,token,JSONObject().put("op","set").put("revision",state.getLong("revision")).put("param","intensity").put("value",JSONArray(listOf(v,0,0,0))))
            val preview=editor(native,token,JSONObject().put("op","preview").put("width",128).put("height",128));val bytes=Base64.decode(preview.getString("png"),Base64.DEFAULT)
            val bitmap=BitmapFactory.decodeByteArray(bytes,0,bytes.size);assertNotNull(bitmap);assertTrue(Color.alpha(bitmap.getPixel(64,64))>0);bitmap.recycle()
            state=editor(native,token,JSONObject().put("op","cancel").put("revision",state.getLong("revision")));assertEquals(1.0,state.getJSONObject("values").getJSONArray("intensity").getDouble(0),0.0)
            assertFalse(JSONObject(NativeBridge.plugin(native,JSONObject().put("op","editor_message").put("token","wrong").put("message",JSONObject("{\"op\":\"state\"}")).toString())).getBoolean("ok"))
            data(NativeBridge.plugin(native,JSONObject().put("op","editor_close").put("token",token).toString()))
            assertFalse(JSONObject(NativeBridge.plugin(native,JSONObject().put("op","editor_message").put("token",token).put("message",JSONObject("{\"op\":\"state\"}")).toString())).getBoolean("ok"))
        }finally{NativeBridge.destroy(native)}
    }
    @Test fun sixGeneratorsMatchGlesWithoutVideoEncoding() {
        val root=root();val native=NativeBridge.create(root.absolutePath,project().toString());assertTrue(NativeBridge.creationError(),native>0)
        val display=EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY);var context=EGL14.EGL_NO_CONTEXT;var surface=EGL14.EGL_NO_SURFACE;var white=0;val reports=JSONArray()
        try {
            val versions=IntArray(2);assertTrue(EGL14.eglInitialize(display,versions,0,versions,1))
            val configs=arrayOfNulls<android.opengl.EGLConfig>(1);val count=IntArray(1)
            assertTrue(EGL14.eglChooseConfig(display,intArrayOf(EGL14.EGL_RED_SIZE,8,EGL14.EGL_GREEN_SIZE,8,EGL14.EGL_BLUE_SIZE,8,EGL14.EGL_ALPHA_SIZE,8,EGL14.EGL_RENDERABLE_TYPE,0x0040,EGL14.EGL_SURFACE_TYPE,EGL14.EGL_PBUFFER_BIT,EGL14.EGL_NONE),0,configs,0,1,count,0))
            context=EGL14.eglCreateContext(display,configs[0],EGL14.EGL_NO_CONTEXT,intArrayOf(EGL14.EGL_CONTEXT_CLIENT_VERSION,3,EGL14.EGL_NONE),0)
            surface=EGL14.eglCreatePbufferSurface(display,configs[0],intArrayOf(EGL14.EGL_WIDTH,1,EGL14.EGL_HEIGHT,1,EGL14.EGL_NONE),0)
            assertTrue(EGL14.eglMakeCurrent(display,surface,surface,context))
            val ids=IntArray(1);GL.glGenTextures(1,ids,0);white=ids[0];GL.glBindTexture(GL.GL_TEXTURE_2D,white)
            GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MIN_FILTER,GL.GL_LINEAR);GL.glTexParameteri(GL.GL_TEXTURE_2D,GL.GL_TEXTURE_MAG_FILTER,GL.GL_LINEAR)
            GL.glTexImage2D(GL.GL_TEXTURE_2D,0,GL.GL_SRGB8_ALPHA8,1,1,0,GL.GL_RGBA,GL.GL_UNSIGNED_BYTE,ByteBuffer.allocateDirect(4).put(byteArrayOf(-1,-1,-1,-1)).apply{flip()})
            fun decode(v:Double)=if(v<=.04045)v/12.92 else ((v+.055)/1.055).pow(2.4)
            fun encode(v:Double)=if(v<=.0031308)v*12.92 else 1.055*v.pow(1/2.4)-.055
            for(id in listOf("lens_flare","starfield","sparks","dust","snow","energy")) {
                add(native,id)
                if(id!="lens_flare")data(NativeBridge.command(native,JSONObject().put("op","effect").put("object",2).put("action",JSONObject().put("kind","set").put("effect",1).put("param","extent").put("frame",0).put("value",JSONArray("[80,80,40,0]"))).toString()))
                val info=data(NativeBridge.renderPlanInfo(native));val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
                assertTrue(NativeBridge.sampleRenderPlanInto(native,7,plan)>0);val expectedBytes=ByteArray(plan.getInt(28));plan.duplicate().apply{position(0);get(expectedBytes)}
                assertTrue(NativeBridge.sampleRenderPlanInto(native,2,plan)>0);assertTrue(NativeBridge.sampleRenderPlanInto(native,7,plan)>0)
                val repeated=ByteArray(plan.getInt(28));plan.duplicate().apply{position(0);get(repeated)};assertArrayEquals(expectedBytes,repeated)
                assertEquals(4,plan.getInt(4));assertTrue(plan.getInt(68)>0)
                val capture=data(NativeBridge.capture(native));val reference=BitmapFactory.decodeFile(capture.getString("path"),BitmapFactory.Options().apply{inPremultiplied=false;inScaled=false})
                assertTrue(EGL14.eglMakeCurrent(display,surface,surface,context))
                val gl=GlEffects(info,native,listOf(white))
                try {
                    gl.prepare(plan);gl.passes(plan,0,plan.getInt(12));val raw=ByteBuffer.allocateDirect(128*128*4)
                    GL.glReadPixels(0,0,128,128,GL.GL_RGBA,GL.GL_UNSIGNED_BYTE,raw);assertEquals(GL.GL_NO_ERROR,GL.glGetError())
                    var rgb=0.0;var alpha=0.0
                    for(y in 0 until 128)for(x in 0 until 128) {
                        val at=(y*128+x)*4;val a=(raw.get(at+3).toInt() and 255)/255.0;val pixel=reference.getPixel(x,y)
                        val channels=intArrayOf(Color.red(pixel),Color.green(pixel),Color.blue(pixel))
                        for(c in 0..2){val v=(raw.get(at+c).toInt() and 255)/255.0;val straight=if(a==0.0)0.0 else encode((decode(v)/a).coerceIn(0.0,1.0))*255;rgb+=abs(straight-channels[c])}
                        alpha+=abs(a*255-Color.alpha(pixel))
                    }
                    val record=JSONObject().put("effect",id).put("rgbMae",rgb/(128*128*3)).put("alphaMae",alpha/(128*128)).put("instances",plan.getInt(68)).put("packageHash",pkg(native).getString("hash"));reports.put(record)
                    File(root,"scene-gles-wgpu-report.json").writeText(reports.toString(2));assertTrue(record.toString(),record.getDouble("rgbMae")<=3&&record.getDouble("alphaMae")<=3)
                }finally{gl.close();reference.recycle()}
                data(NativeBridge.command(native,"{\"op\":\"effect\",\"object\":2,\"action\":{\"kind\":\"remove\",\"effect\":1}}"))
            }
        }finally {
            GL.glDeleteTextures(1,intArrayOf(white),0);EGL14.eglMakeCurrent(display,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_CONTEXT);EGL14.eglDestroySurface(display,surface);EGL14.eglDestroyContext(display,context);EGL14.eglTerminate(display);NativeBridge.destroy(native)
        }
    }
    @Test fun frozenParticleMovieExportsAndCapacityFailureRemovesOutput() {
        val root=root();val native=NativeBridge.create(root.absolutePath,project().put("background",JSONArray("[0,0,0,1]")).toString());assertTrue(NativeBridge.creationError(),native>0)
        try {
            add(native,"sparks");data(NativeBridge.seek(native,7.0));val referencePath=data(NativeBridge.capture(native)).getString("path");val frozen=data(NativeBridge.state(native)).getJSONObject("project").toString()
            var edited=false
            val movie=VideoExporter(root,frozen).run{_,_->if(!edited){data(NativeBridge.command(native,"{\"op\":\"effect\",\"object\":2,\"action\":{\"kind\":\"set\",\"effect\":1,\"param\":\"rate\",\"frame\":0,\"value\":[10000,0,0,0]}}"));edited=true}}
            val retriever=MediaMetadataRetriever()
            try {
                retriever.setDataSource(movie.absolutePath);assertEquals("128",retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH))
                val decoded=retriever.getFrameAtTime(233333L,MediaMetadataRetriever.OPTION_CLOSEST)!!
                val expected=BitmapFactory.decodeFile(referencePath);var rgb=0.0;var foreground=0.0;var foregroundCount=0
                for(y in 0 until 128)for(x in 0 until 128) {
                    val a=expected.getPixel(x,y);val b=decoded.getPixel(x,y)
                    val difference=abs(Color.red(a)-Color.red(b))+abs(Color.green(a)-Color.green(b))+abs(Color.blue(a)-Color.blue(b));rgb+=difference
                    if(maxOf(Color.red(a),Color.green(a),Color.blue(a))>16){foreground+=difference;foregroundCount++}
                }
                val report=JSONObject().put("rgbMae",rgb/(128*128*3)).put("foregroundMae",foreground/(foregroundCount*3)).put("foregroundPixels",foregroundCount).put("frame",7).put("frozen",true)
                File(root,"scene-mp4-parity.json").writeText(report.toString(2));assertTrue(report.toString(),foregroundCount>0&&report.getDouble("rgbMae")<6&&report.getDouble("foregroundMae")<8)
                decoded.recycle();expected.recycle()
            }finally{retriever.release()}
            val exports=File(root,"exports");val before=exports.listFiles().orEmpty().filter{it.extension=="mp4"}.map{it.name}.toSet()
            val invalid=data(NativeBridge.state(native)).getJSONObject("project").toString()
            assertTrue(runCatching{VideoExporter(root,invalid).run{_,_->}}.isFailure)
            assertEquals(before,exports.listFiles().orEmpty().filter{it.extension=="mp4"}.map{it.name}.toSet())
        }finally{NativeBridge.destroy(native)}
    }
}
