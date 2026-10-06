package com.motionstudio.editor

import android.graphics.BitmapFactory
import android.graphics.Color
import android.media.MediaMetadataRetriever
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

@RunWith(AndroidJUnit4::class)
class PropertyExpressionsTest {
    private fun data(raw:String):JSONObject=JSONObject(raw).let { assertTrue(it.optString("error"),it.optBoolean("ok"));it.getJSONObject("data") }
    private fun root()=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"expressions-test/${UUID.randomUUID()}/project").apply{mkdirs()}
    private fun project():JSONObject {
        val p=data(NativeBridge.projectTemplate(0)).put("width",128).put("height",128).put("frames",12)
        p.getJSONObject("camera").put("created",false)
        val layer=p.getJSONArray("layers").getJSONObject(1).put("size",JSONArray(listOf(32,32)))
        layer.getJSONObject("transform").getJSONObject("position").put("value",JSONArray(listOf(48,64,0)))
        return p.put("layers",JSONArray().put(layer))
    }
    private fun target(property:String)=JSONObject().put("kind","property").put("object",2).put("property",property)
    private fun expression(source:String,target:JSONObject,enabled:Boolean=true)=JSONObject().put("target",target).put("source",source).put("enabled",enabled).put("seed",42).put("profile","motion-studio-ae-js-1")
    private fun set(native:Long,e:JSONObject,frame:Int=0)=data(NativeBridge.command(native,JSONObject().put("op","set_expression").put("expression",e).put("frame",frame).toString()))
    @Test fun javascriptAbiPersistenceUndoAndFrameErrors() {
        val root=root();val native=NativeBridge.create(root.absolutePath,project().toString());assertTrue(NativeBridge.creationError(),native>0)
        try {
            val e=expression("var d=[time*60,0,0]; value+d;",target("position"))
            set(native,e);val state=data(NativeBridge.seek(native,7.0))
            assertEquals(5,state.getJSONObject("project").getInt("version"));assertEquals(62.0,state.getJSONArray("sampledLayers").getJSONObject(0).getJSONArray("position").getDouble(0),1e-5)
            assertEquals(48.0,state.getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONObject("transform").getJSONObject("position").getJSONArray("value").getDouble(0),0.0)
            assertEquals("QuickJS-NG",state.getJSONObject("capabilities").getJSONObject("property_expressions").getString("engine"))
            val frozen=state.getJSONObject("project").toString()
            data(NativeBridge.history(native,0));assertFalse(data(NativeBridge.state(native)).getJSONObject("project").has("expressions"))
            data(NativeBridge.history(native,1));assertEquals(1,data(NativeBridge.state(native)).getJSONObject("project").getJSONArray("expressions").length())
            val copy=NativeBridge.create(root().absolutePath,frozen);assertTrue(NativeBridge.creationError(),copy>0)
            try { assertEquals(62.0,data(NativeBridge.seek(copy,7.0)).getJSONArray("sampledLayers").getJSONObject(0).getJSONArray("position").getDouble(0),1e-5) } finally { NativeBridge.destroy(copy) }
            val before=data(NativeBridge.state(native)).getLong("revision")
            val invalid=JSONObject(NativeBridge.command(native,JSONObject().put("op","set_expression").put("expression",expression("while(true){}",target("position"))).put("frame",0).toString()))
            assertFalse(invalid.getBoolean("ok"));assertTrue(invalid.getString("error"),invalid.getString("error").contains("budget"));assertEquals(before,data(NativeBridge.state(native)).getLong("revision"))
            set(native,expression("time<.1 ? value : unknown()",target("position")))
            assertFalse(JSONObject(NativeBridge.seek(native,7.0)).getBoolean("ok"));assertFalse(JSONObject(NativeBridge.capture(native)).getBoolean("ok"))
            assertFalse(JSONObject(NativeBridge.renderPlanInfo(native)).getBoolean("ok"))
            val failedExport=runCatching { VideoExporter(root,data(NativeBridge.state(native)).getJSONObject("project").toString()).run{_,_->} }
            assertTrue("late expression error must stop MP4",failedExport.isFailure)
            assertTrue(failedExport.exceptionOrNull().toString(),failedExport.exceptionOrNull().toString().contains("expression"))
            assertTrue("partial MP4 must be removed",File(root,"exports").listFiles().orEmpty().none{it.extension=="mp4"})
            set(native,expression("time<.1 ? value : unknown()",target("position"),false),7)
            assertTrue(File(data(NativeBridge.capture(native)).getString("path")).isFile)
            val bad=JSONObject(frozen);bad.getJSONArray("expressions").getJSONObject(0).put("source","unknown()")
            val broken=NativeBridge.create(root().absolutePath,bad.toString());assertTrue("failed source must remain editable",broken>0)
            try { assertTrue(data(NativeBridge.state(broken)).getString("renderError").contains("expression"));assertFalse(JSONObject(NativeBridge.capture(broken)).getBoolean("ok")) } finally { NativeBridge.destroy(broken) }
        } finally { NativeBridge.destroy(native) }
    }
    @Test fun frozenExpressionGeometryAndEffectParametersMatchMp4() {
        val root=root();val native=NativeBridge.create(root.absolutePath,project().toString());assertTrue(NativeBridge.creationError(),native>0)
        try {
            val packages=data(NativeBridge.plugin(native,"{\"op\":\"catalogue\"}")).getJSONArray("packages")
            val pkg=(0 until packages.length()).map{packages.getJSONObject(it)}.first{it.getJSONObject("manifest").getString("version")=="1.1.0"}
            val m=pkg.getJSONObject("manifest")
            data(NativeBridge.plugin(native,JSONObject().put("op","add").put("object",2).put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",pkg.getString("hash")).put("effect","tint").toString()))
            set(native,expression("value+[time*60,0,0]",target("position")))
            set(native,expression("linear(time,0,.4,0,100)",JSONObject().put("kind","effect").put("object",2).put("effect",1).put("param","p0003")))
            val frozen=data(NativeBridge.state(native)).getJSONObject("project").toString()
            val info=data(NativeBridge.renderPlanInfo(native));val buffer=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
            assertTrue(NativeBridge.sampleRenderPlanInto(native,7,buffer)>0)
            val reference=BitmapFactory.decodeFile(data(NativeBridge.capture(native)).getString("path"))
            val file=VideoExporter(root,frozen).run{done,_->if(done==3)data(NativeBridge.command(native,JSONObject().put("op","remove_expression").put("target",target("position")).toString()))}
            val retriever=MediaMetadataRetriever();val decoded=try{retriever.setDataSource(file.absolutePath);retriever.getFrameAtIndex(7)!!}finally{retriever.release()}
            var sum=0L;var count=0;var fg=0L;var fgCount=0
            for(y in 0 until 128)for(x in 0 until 128) {
                val a=reference.getPixel(x,y);val b=decoded.getPixel(x,y)
                val d=abs(Color.red(a)-Color.red(b))+abs(Color.green(a)-Color.green(b))+abs(Color.blue(a)-Color.blue(b));sum+=d;count+=3
                if(x in 46..77 && y in 48..79){fg+=d;fgCount+=3}
            }
            val report=JSONObject().put("frame",7).put("frames",12).put("rgbMae",sum.toDouble()/count).put("foregroundRgbMae",fg.toDouble()/fgCount).put("frozenExpressions",true).put("device",android.os.Build.MODEL)
            File(root,"expression-export-report.json").writeText(report.toString(2));assertTrue(report.toString(),report.getDouble("rgbMae")<6&&report.getDouble("foregroundRgbMae")<8)
            reference.recycle();decoded.recycle()
        } finally { NativeBridge.destroy(native) }
    }
}
