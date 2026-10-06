package com.motionstudio.editor

import android.graphics.BitmapFactory
import android.graphics.PixelFormat
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class VideoInputApiTest {
    private val context get()=InstrumentationRegistry.getInstrumentation().targetContext
    private fun data(raw:String):JSONObject {val r=JSONObject(raw);assertTrue(r.toString(),r.optBoolean("ok"));return r.getJSONObject("data")}
    private fun req(id:Long,op:String,vararg fields:Pair<String,Any>)=data(MediaBridge.request(id,context,JSONObject().put("op",op).apply{fields.forEach{put(it.first,it.second)}}.toString()))
    private fun root()=File(context.filesDir,"acceptance/video-input-"+UUID.randomUUID()).apply{mkdirs()}
    private fun create(root:File,fps:Int):Long {
        val p=data(NativeBridge.projectTemplate(0)).put("width",256).put("height",144).put("fps",fps).put("frames",fps*2).put("layers",JSONArray())
        return NativeBridge.create(root.absolutePath,p.toString()).also{assertTrue(it>0)}
    }
    private fun await(id:Long,key:String):JSONObject {
        val deadline=System.nanoTime()+90_000_000_000L
        while(true){val t=req(id,"media_status","request_id" to key);if(t.getString("state")!="running")return t;assertTrue("import timeout",System.nanoTime()<deadline);Thread.sleep(10)}
    }
    private fun frame(id:Long,time:Double,seq:Long,w:Int,h:Int):Pair<JSONObject,ByteArray> {
        val deadline=System.nanoTime()+30_000_000_000L
        var s=req(id,"request_video_frame","object" to 1,"frame" to time,"sequence" to seq)
        while(s.getString("state")=="pending"){assertTrue(s.toString(),System.nanoTime()<deadline);Thread.sleep(10);s=req(id,"request_video_frame","object" to 1,"frame" to time,"sequence" to seq)}
        assertEquals(s.toString(),"ready",s.getString("state"))
        val buffer=ByteBuffer.allocateDirect(w*h*4)
        val report=data(MediaBridge.readVideoFrameInto(id,1,seq,buffer))
        assertEquals(w,report.getInt("width"));assertEquals(h,report.getInt("height"))
        return report to ByteArray(buffer.capacity()).also{buffer.get(it)}
    }
    private fun verify4kGpuPreview(id:Long,root:File,w:Int,h:Int) {
        val consumer=HandlerThread("4k-preview-consumer").apply{start()}
        val reader=ImageReader.newInstance(256,144,PixelFormat.RGBA_8888,3)
        reader.setOnImageAvailableListener({r->r.acquireLatestImage()?.close()},Handler(consumer.looper))
        try {
            data(NativeBridge.surface(id,reader.surface,256,144));data(NativeBridge.previewMode(id,1,0))
            val deadline=System.nanoTime()+30_000_000_000L
            while(!NativeBridge.render(id,0.0)){assertTrue(System.nanoTime()<deadline);Thread.sleep(10)}
            val metrics=data(NativeBridge.previewInfo(id)).getJSONObject("video")
            assertEquals(1L,metrics.getLong("gpuConversions"))
            assertEquals(w.toLong()*h*3/2,metrics.getLong("uploadBytes"))
            assertTrue(metrics.getLong("cacheBytes")<=metrics.getLong("cacheBudgetBytes"))
            val capture=data(NativeBridge.capture(id));val bitmap=BitmapFactory.decodeFile(capture.getString("path"))
            try {assertEquals(256,bitmap.width);assertEquals(144,bitmap.height);assertEquals(255,bitmap.getPixel(128,72) ushr 24)}finally{bitmap.recycle()}
            File(root,"gpu-preview.json").writeText(metrics.toString(2))
        } finally {
            data(NativeBridge.surface(id,null,0,0));reader.close();consumer.quitSafely();consumer.join(3000)
        }
    }
    @Test fun uhd60SourceSurvivesReopenIn24fpsComposition()=verifySource("uhd-60.mp4",24)
    @Test fun dciHevc60SourceSurvivesReopenIn120fpsComposition()=verifySource("dci-60.mp4",120)
    @Test fun portrait4kSourceSurvivesReopen()=verifySource("portrait-60.mp4",60)
    @Test fun source240fpsSurvivesReopen()=verifySource("hfr-240.mp4",240)
    @Test fun fractionalSourceRateSurvivesReopenIn25fpsComposition()=verifySource("fractional-59.94.mp4",25)
    @Test fun ultraWideSourceKeepsOriginalAspectRatio()=verifySource("ultrawide-60.mp4",24)
    @Test fun squareSourceKeepsOriginalAspectRatio()=verifySource("square-60.mp4",60)
    @Test fun tallSourceKeepsOriginalAspectRatio()=verifySource("tall-60.mp4",25)
    private fun verifySource(name:String,fps:Int) {
            val root=root();var id=create(root,fps)
            try {
                req(id,"import_media","kind" to "video","with_audio" to false,"request_id" to "import","uri" to "content://com.motionstudio.editor.test.audio-fixtures/inputs/$name")
                val task=await(id,"import");assertEquals("$name: $task","ready",task.getString("state"))
                val done=req(id,"finish_media_import","request_id" to "import");assertEquals("succeeded",done.getJSONObject("task").getString("state"))
                val a=done.getJSONObject("task").getJSONObject("metadata").getJSONObject("video")
                val w=a.getInt("display_width");val h=a.getInt("display_height")
                val rate=a.getDouble("nominal_frame_rate")
                when(name) {
                    "uhd-60.mp4"->{assertEquals(3840,w);assertEquals(2160,h);assertEquals(60.0,rate,.1)}
                    "dci-60.mp4"->{assertEquals(4096,w);assertEquals(2160,h);assertEquals(60.0,rate,.1);assertEquals("video/hevc",a.getString("mime"))}
                    "portrait-60.mp4"->{assertEquals(2160,w);assertEquals(3840,h)}
                    "hfr-240.mp4"->assertEquals(240.0,rate,.1)
                    "ultrawide-60.mp4"->{assertEquals(3840,w);assertEquals(480,h)}
                    "square-60.mp4"->{assertEquals(2560,w);assertEquals(2560,h)}
                    "tall-60.mp4"->{assertEquals(480,w);assertEquals(3840,h)}
                    else->assertEquals(60000.0/1001,rate,.05)
                }
                val project=data(NativeBridge.state(id)).getJSONObject("project")
                assertEquals(fps,project.getInt("fps"))
                val size=project.getJSONArray("layers").getJSONObject(0).getJSONArray("size")
                assertEquals(w.toDouble(),size.getDouble(0),0.0);assertEquals(h.toDouble(),size.getDouble(1),0.0)
                val samples=JSONArray()
                for((i,time)in listOf(0.0,fps*.06).withIndex()) {
                    val decoded=frame(id,time,i+1L,w,h);samples.put(decoded.first);File(root,"frame-$i.rgba").writeBytes(decoded.second)
                }
                val thumbnail=req(id,"video_thumbnail","asset" to a.getLong("id"));assertTrue(File(thumbnail.getString("path")).isFile)
                data(NativeBridge.save(id));NativeBridge.destroy(id);id=NativeBridge.create(root.absolutePath,"");assertTrue(id>0)
                assertEquals(a.toString(),data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("video_assets").getJSONObject(0).toString())
                val reopened=frame(id,0.0,1,w,h);assertArrayEquals(File(root,"frame-0.rgba").readBytes(),reopened.second)
                val metrics=data(NativeBridge.previewInfo(id)).getJSONObject("video")
                assertTrue(metrics.toString(),metrics.getLong("cacheBytes")<=metrics.getLong("cacheBudgetBytes"))
                assertTrue(metrics.getLong("cacheBudgetBytes")<=metrics.getLong("maxCacheBudgetBytes"))
                if(name=="uhd-60.mp4") {
                    verify4kGpuPreview(id,root,w,h)
                    val frozen=data(MediaBridge.freezeVideo(id)).getLong("handle");req(id,"release_video_frames")
                    try {
                        val deadline=System.nanoTime()+30_000_000_000L
                        var value=data(MediaBridge.requestFrozenVideoFrame(frozen,1,0.0,1))
                        while(value.getString("state")=="pending"){assertTrue(System.nanoTime()<deadline);Thread.sleep(10);value=data(MediaBridge.requestFrozenVideoFrame(frozen,1,0.0,1))}
                        assertEquals(value.toString(),"ready",value.getString("state"))
                        val buffer=ByteBuffer.allocateDirect(w*h*4);data(MediaBridge.readFrozenVideoFrameInto(frozen,1,1,buffer))
                        val bytes=ByteArray(buffer.capacity());buffer.get(bytes);assertArrayEquals(reopened.second,bytes)
                    }finally{data(MediaBridge.releaseFrozenVideo(frozen))}
                }
                File(root,"input-report.json").writeText(JSONObject().put("fixture",name).put("asset",a).put("composition_fps",fps).put("samples",samples).put("metrics",metrics).toString(2))
            }finally{NativeBridge.destroy(id)}
    }
    @Test fun higherRate4kProbeRecordsActualDeviceOutcomeWithoutProjectMutation() {
        val root=root();val id=create(root,120)
        try {
            val before=data(NativeBridge.state(id)).getJSONObject("project").toString()
            val caps=req(0,"media_capabilities","video_query" to JSONObject().put("mime","video/avc").put("width",4096).put("height",2160).put("frame_rate",120.0))
            assertTrue(caps.getJSONObject("query_result").getBoolean("backend_eligible"))
            req(id,"probe_media","kind" to "video","with_audio" to false,"request_id" to "probe","uri" to "content://com.motionstudio.editor.test.audio-fixtures/inputs/dci-120.mp4")
            val result=await(id,"probe")
            // Backend eligibility does not promise this device supports H.264 level 6.
            when(result.getString("state")) {
                "succeeded"->{val a=result.getJSONObject("metadata").getJSONObject("video");assertEquals(4096,a.getInt("display_width"));assertEquals(120.0,a.getDouble("nominal_frame_rate"),.1)}
                "failed"->assertTrue(result.toString(),result.getString("error").isNotBlank())
                else->fail(result.toString())
            }
            assertEquals(before,data(NativeBridge.state(id)).getJSONObject("project").toString());assertFalse(data(NativeBridge.state(id)).getBoolean("canUndo"))
            File(root,"device-probe.json").writeText(JSONObject().put("fixture","dci-120.mp4").put("capabilities",caps).put("probe",result).toString(2))
        } finally {NativeBridge.destroy(id)}
    }
    @Test fun capabilityQueriesSeparateSourceLimitsFromDeviceSizeAndRate() {
        val caps=req(0,"media_capabilities","video_query" to JSONObject().put("mime","video/avc").put("width",3840).put("height",2160).put("frame_rate",60.0))
        assertEquals(4096,caps.getJSONObject("video").getInt("max_dimension"));assertEquals(240,caps.getJSONObject("video").getInt("max_fps"))
        assertTrue(caps.getJSONObject("video").getBoolean("arbitrary_aspect_ratio"));assertTrue(caps.getJSONObject("video").getBoolean("preserves_source_aspect_ratio"))
        assertEquals(240,caps.getJSONObject("composition").getJSONArray("fps_range").getInt(1))
        val result=caps.getJSONObject("query_result");assertTrue(result.getBoolean("backend_eligible"));assertFalse(result.getBoolean("real_time_guaranteed"));assertTrue(result.getJSONArray("decoders").length()>0)
        val decoder=result.getJSONArray("decoders").getJSONObject(0).getJSONObject("capabilities");assertTrue(decoder.has("size_supported"));assertTrue(decoder.has("size_and_rate_supported"))
        val outside=req(0,"media_capabilities","video_query" to JSONObject().put("mime","video/avc").put("width",7680).put("height",4320).put("frame_rate",60.0))
        assertFalse(outside.getJSONObject("query_result").getBoolean("backend_eligible"))
        val invalid=JSONObject(MediaBridge.request(0,context,"{\"op\":\"media_capabilities\",\"video_query\":{\"mime\":\"video/avc\",\"width\":0,\"height\":2160,\"frame_rate\":60}}"));assertFalse(invalid.getBoolean("ok"))
        File(root(),"capabilities.json").writeText(caps.toString(2))
    }
    @Test fun unsupportedSourceBoundsFailWithoutProjectMutation() {
        for(name in listOf("too-wide.mp4","too-fast.mp4")) {
            val root=root();val id=create(root,24)
            try {
                val before=data(NativeBridge.state(id)).getJSONObject("project").toString()
                req(id,"import_media","kind" to "video","with_audio" to false,"request_id" to "reject","uri" to "content://com.motionstudio.editor.test.audio-fixtures/inputs/$name")
                val t=await(id,"reject");assertEquals(t.toString(),"failed",t.getString("state"));assertEquals(before,data(NativeBridge.state(id)).getJSONObject("project").toString());assertFalse(data(NativeBridge.state(id)).getBoolean("canUndo"))
            }finally{NativeBridge.destroy(id)}
        }
    }
}
