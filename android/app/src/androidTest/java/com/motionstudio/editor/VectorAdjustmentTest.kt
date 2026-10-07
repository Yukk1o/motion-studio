package com.motionstudio.editor

import android.graphics.BitmapFactory
import android.graphics.Bitmap
import android.graphics.Color
import android.media.MediaMetadataRetriever
import android.opengl.EGL14
import android.opengl.GLES30 as GL
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.abs

class VectorAdjustmentTest {
    private fun data(text:String?):JSONObject {val o=JSONObject(text?:error("JNI returned null"));assertTrue(o.toString(),o.getBoolean("ok"));return o.getJSONObject("data")}
    @Test fun fullscreenTriangleArithmeticCoversPbuffer() {
        val display=EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY)
        val version=IntArray(2);assertTrue(EGL14.eglInitialize(display,version,0,version,1))
        val configs=arrayOfNulls<android.opengl.EGLConfig>(1);val count=IntArray(1)
        assertTrue(EGL14.eglChooseConfig(display,intArrayOf(EGL14.EGL_SURFACE_TYPE,EGL14.EGL_PBUFFER_BIT,EGL14.EGL_RENDERABLE_TYPE,0x0040,EGL14.EGL_RED_SIZE,8,EGL14.EGL_GREEN_SIZE,8,EGL14.EGL_BLUE_SIZE,8,EGL14.EGL_NONE),0,configs,0,1,count,0))
        val context=EGL14.eglCreateContext(display,configs[0],EGL14.EGL_NO_CONTEXT,intArrayOf(EGL14.EGL_CONTEXT_CLIENT_VERSION,3,EGL14.EGL_NONE),0)
        val surface=EGL14.eglCreatePbufferSurface(display,configs[0],intArrayOf(EGL14.EGL_WIDTH,16,EGL14.EGL_HEIGHT,16,EGL14.EGL_NONE),0)
        assertTrue(EGL14.eglMakeCurrent(display,surface,surface,context))
        try {
            fun compile(kind:Int,source:String):Int {val s=GL.glCreateShader(kind);GL.glShaderSource(s,source);GL.glCompileShader(s);val ok=IntArray(1);GL.glGetShaderiv(s,GL.GL_COMPILE_STATUS,ok,0);assertEquals(GL.glGetShaderInfoLog(s),1,ok[0]);return s}
            val report=JSONObject()
            for((name,body)in listOf("array" to "const vec2 p[3]=vec2[3](vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));void main(){gl_Position=vec4(p[gl_VertexID],0.,1.);}","arithmetic" to "void main(){vec2 p=vec2(float((gl_VertexID<<1)&2),float(gl_VertexID&2))*2.-1.;gl_Position=vec4(p,0.,1.);}")) {
                val vs=compile(GL.GL_VERTEX_SHADER,"#version 300 es\n$body")
                val fs=compile(GL.GL_FRAGMENT_SHADER,"#version 300 es\nprecision highp float;out vec4 result;void main(){result=vec4(1.,0.,0.,1.);}")
                val p=GL.glCreateProgram();GL.glAttachShader(p,vs);GL.glAttachShader(p,fs);GL.glLinkProgram(p);val ok=IntArray(1);GL.glGetProgramiv(p,GL.GL_LINK_STATUS,ok,0);assertEquals(GL.glGetProgramInfoLog(p),1,ok[0])
                GL.glViewport(0,0,16,16);GL.glClearColor(0f,0f,0f,1f);GL.glClear(GL.GL_COLOR_BUFFER_BIT);GL.glUseProgram(p);GL.glDrawArrays(GL.GL_TRIANGLES,0,3)
                val pixel=ByteBuffer.allocateDirect(4);GL.glReadPixels(8,8,1,1,GL.GL_RGBA,GL.GL_UNSIGNED_BYTE,pixel)
                report.put(name,JSONArray((0..3).map{pixel.get(it).toInt() and 255}));report.put(name+"Error",GL.glGetError())
                GL.glDeleteProgram(p);GL.glDeleteShader(vs);GL.glDeleteShader(fs)
            }
            val root=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"vector-adjustment-test").apply{mkdirs()};File(root,"fullscreen-report.json").writeText(report.toString(2))
            assertEquals(report.toString(),255,report.getJSONArray("arithmetic").getInt(0))
            assertEquals(report.toString(),0,report.getInt("arithmeticError"))
        }finally{EGL14.eglMakeCurrent(display,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_CONTEXT);EGL14.eglDestroySurface(display,surface);EGL14.eglDestroyContext(display,context);EGL14.eglTerminate(display)}
    }
    @Test fun shapesAdjustmentAndEncodedOutputMatchPreview() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val root=File(context.filesDir,"vector-adjustment-test").apply{mkdirs()}
        val project=data(NativeBridge.projectTemplate(0)).put("version",6).put("width",256).put("height",256).put("frames",6).put("layers",JSONArray()).put("background",JSONArray(listOf(0.03,0.04,0.07,1)))
        project.getJSONObject("camera").put("created",false)
        val session=NativeBridge.create(root.absolutePath,project.toString());assertTrue(NativeBridge.creationError(),session>0)
        fun command(o:JSONObject)=data(NativeBridge.command(session,o.toString()))
        fun shape(id:Int,kind:String,x:Int,y:Int,color:List<Double>) {
            command(JSONObject().put("op","add_shape").put("id",id).put("name",kind).put("shape",kind).put("size",JSONArray(listOf(100,100))).put("position",JSONArray(listOf(x,y,0))))
            val layer=data(NativeBridge.state(session)).getJSONObject("project").getJSONArray("layers").getJSONObject(id-1)
            val vector=layer.getJSONObject("content").getJSONObject("vector")
            vector.getJSONObject("fill").put("value",JSONArray(color))
            command(JSONObject().put("op","vector").put("object",id).put("action",JSONObject().put("action","replace").put("vector",vector)))
        }
        try {
            val caps=data(NativeBridge.state(session)).getJSONObject("capabilities");assertEquals(25,caps.getJSONObject("vector_drawing").getJSONArray("shape_catalog").length())
            shape(1,"ring",70,78,listOf(0.9,0.1,0.2,0.7));shape(2,"heart",170,176,listOf(0.1,0.8,0.3,1.0))
            val baseCapture=data(NativeBridge.capture(session))
            File(baseCapture.getString("path")).copyTo(File(root,"base.png"),overwrite=true)
            command(JSONObject().put("op","add_adjustment").put("id",3).put("name","Tint lower layers"))
            val packageInfo=data(NativeBridge.plugin(session,"{\"op\":\"catalogue\"}")).getJSONArray("packages")
            var pkg:JSONObject?=null
            for(i in 0 until packageInfo.length()) {val entry=packageInfo.getJSONObject(i);if(entry.getJSONObject("manifest").getString("id")=="com.motionstudio.effects.ae2021"&&entry.getJSONObject("manifest").getString("version")=="1.3.0")pkg=entry}
            val entry=pkg?:error("core package missing");val manifest=entry.getJSONObject("manifest")
            data(NativeBridge.plugin(session,JSONObject().put("op","add").put("object",3).put("effect","tint").put("plugin",manifest.getString("id")).put("version",manifest.getString("version")).put("hash",entry.getString("hash")).toString()))
            val state=data(NativeBridge.state(session))
            File(root,"state.json").writeText(state.toString(2))
            val frozen=state.getJSONObject("project").toString()
            File(root,"project.json").writeText(frozen)
            val info=data(NativeBridge.renderPlanInfo(session))
            val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
            val planBytes=NativeBridge.sampleRenderPlanInto(session,0,plan)
            assertTrue(planBytes>0)
            val copy=ByteArray(planBytes);plan.position(0);plan.get(copy)
            File(root,"plan.bin").writeBytes(copy)
            val capture=data(NativeBridge.capture(session));val reference=BitmapFactory.decodeFile(capture.getString("path"))
            val file=VideoExporter(root,frozen).run{_,_->}
            val retriever=MediaMetadataRetriever();val decoded=try{retriever.setDataSource(file.absolutePath);retriever.getFrameAtIndex(0)!!}finally{retriever.release()}
            File(root,"decoded.png").outputStream().use{decoded.compress(Bitmap.CompressFormat.PNG,100,it)}
            var sum=0L;var samples=0;var foreground=0L;var active=0
            for(y in 0 until 256 step 2)for(x in 0 until 256 step 2) {val a=reference.getPixel(x,y);val b=decoded.getPixel(x,y);val d=abs(Color.red(a)-Color.red(b))+abs(Color.green(a)-Color.green(b))+abs(Color.blue(a)-Color.blue(b));sum+=d;samples+=3
                if(Color.red(a)>50||Color.green(a)>50||Color.blue(a)>50){foreground+=d;active+=3}}
            val report=JSONObject().put("meanRgb",sum.toDouble()/samples).put("foregroundMeanRgb",foreground.toDouble()/active.coerceAtLeast(1)).put("foregroundSamples",active)
            File(root,"vector-adjustment-report.json").writeText(report.toString(2))
            assertTrue("Preview must contain visible shapes: $report",active>0)
            assertTrue(report.toString(),report.getDouble("meanRgb")<6&&report.getDouble("foregroundMeanRgb")<8)
            reference.recycle();decoded.recycle()
        }finally{NativeBridge.destroy(session)}
    }
}
