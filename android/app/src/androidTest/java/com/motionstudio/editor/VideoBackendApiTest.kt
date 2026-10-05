package com.motionstudio.editor

import android.graphics.BitmapFactory
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
class VideoBackendApiTest {
    private val context get()=InstrumentationRegistry.getInstrumentation().targetContext
    private fun data(raw:String):JSONObject {val r=JSONObject(raw);assertTrue(r.optString("error"),r.optBoolean("ok"));return r.getJSONObject("data")}
    private fun req(id:Long,op:String,vararg fields:Pair<String,Any>)=data(MediaBridge.request(id,context,JSONObject().put("op",op).apply{fields.forEach{put(it.first,it.second)}}.toString()))
    private fun command(id:Long,op:String,vararg fields:Pair<String,Any>)=data(NativeBridge.command(id,JSONObject().put("op",op).apply{fields.forEach{put(it.first,it.second)}}.toString()))
    private fun root()=File(context.filesDir,"acceptance/video-api-"+UUID.randomUUID()).apply{mkdirs()}
    private fun create(root:File):Long {val p=data(NativeBridge.projectTemplate(0)).put("width",256).put("height",144).put("fps",60).put("frames",240).put("layers",JSONArray());val id=NativeBridge.create(root.absolutePath,p.toString());assertTrue(id>0);return id}
    private fun await(id:Long,key:String):JSONObject {val deadline=System.nanoTime()+45_000_000_000L;while(true){val t=req(id,"media_status","request_id" to key);if(t.getString("state")!="running")return t;assertTrue("Video task timeout",System.nanoTime()<deadline);Thread.sleep(10)}}
    private fun import(id:Long,name:String,key:String="import",audio:Boolean=true):JSONObject {
        req(id,"import_media","kind" to "video","request_id" to key,"uri" to "content://com.motionstudio.editor.test.audio-fixtures/$name","with_audio" to audio)
        val t=await(id,key);assertEquals(t.toString(),"ready",t.getString("state"));val committed=req(id,"finish_media_import","request_id" to key);assertEquals(committed.toString(),"succeeded",committed.getJSONObject("task").getString("state"));return committed
    }
    private fun frame(id:Long,objectId:Long,time:Double,seq:Long):Pair<JSONObject,ByteArray> {
        val deadline=System.nanoTime()+15_000_000_000L
        var status=req(id,"request_video_frame","object" to objectId,"frame" to time,"sequence" to seq)
        while(status.getString("state")=="pending"){assertTrue("Video frame timeout",System.nanoTime()<deadline);Thread.sleep(5);status=req(id,"request_video_frame","object" to objectId,"frame" to time,"sequence" to seq)}
        assertEquals(status.toString(),"ready",status.getString("state"))
        val b=ByteBuffer.allocateDirect(status.getInt("width")*status.getInt("height")*4)
        val metadata=data(MediaBridge.readVideoFrameInto(id,objectId,seq,b));val bytes=ByteArray(metadata.getInt("bytes"));b.get(bytes);return metadata to bytes
    }
    private fun pixel(bytes:ByteArray,width:Int,x:Int,y:Int)=IntArray(3){bytes[(y*width+x)*4+it].toInt() and 255}
    private fun pcm(id:Long,start:Long,frames:Int=1024):FloatArray {val b=ByteBuffer.allocateDirect(frames*8).order(ByteOrder.LITTLE_ENDIAN);data(MediaBridge.readPcmInto(id,start,frames,b));return FloatArray(frames*2).also{b.asFloatBuffer().get(it)}}
    private fun assertActive(values:FloatArray){assertTrue("Expected decoded sound",values.any{abs(it)>.03f})}
    private fun assertSilent(values:FloatArray){assertTrue("Unexpected sound in silence",values.all{abs(it)<.003f})}

    @Test fun actualMp4FramesComposeAndReopenFromTheOwnedCopy() {
        val root=root();var id=create(root)
        try {
            val imported=import(id,"silent-24fps.mp4");val edit=imported.getJSONObject("task").getJSONObject("edit_result");assertFalse(edit.getBoolean("has_audio"))
            val first=frame(id,1,15.0,1);assertEquals(250000L,first.first.getLong("pts_us"));assertTrue(pixel(first.second,256,128,72)[0]>180)
            val later=frame(id,1,45.0,2);assertTrue(pixel(later.second,256,128,72)[1]>180)
            File(root,"silent-075.rgba").writeBytes(later.second)
            data(NativeBridge.seek(id,45.0));val capture=data(NativeBridge.capture(id));val bitmap=BitmapFactory.decodeFile(capture.getString("path"));assertTrue((bitmap.getPixel(128,72) shr 8 and 255)>180);bitmap.recycle()
            data(NativeBridge.seek(id,60.0));val auto=JSONObject(NativeBridge.capture(id));assertTrue(auto.optBoolean("ok")||auto.optString("error").contains("video capture pending"))
            // Automatic preview/capture generations must not invalidate the public sequence namespace.
            assertArrayEquals(later.second,frame(id,1,45.0,2).second)
            assertEquals(-1,NativeBridge.sampleInto(id,45,ByteBuffer.allocateDirect(4096)))
            data(NativeBridge.save(id));NativeBridge.destroy(id);id=NativeBridge.create(root.absolutePath,"");assertTrue(id>0)
            val reopened=frame(id,1,75.0,1);assertTrue(pixel(reopened.second,256,128,72)[2]>180)
            val thumbnail=req(id,"video_thumbnail","asset" to edit.getLong("asset"));assertTrue(File(thumbnail.getString("path")).isFile)
            File(root,"video-report.json").writeText(JSONObject().put("metadata",imported.getJSONObject("task").getJSONObject("metadata")).put("capture",capture).put("samples",JSONArray().put(first.first).put(later.first).put(reopened.first)).toString())
        } finally {NativeBridge.destroy(id)}
    }

    @Test fun videoAudioPreservesLeadingDelayAndUsesTheSameClipEdits() {
        val root=root();val id=create(root)
        try {
            val imported=import(id,"audio-delayed.mp4");assertTrue(imported.getJSONObject("task").getJSONObject("edit_result").getBoolean("has_audio"))
            assertSilent(pcm(id,7200));assertActive(pcm(id,18000)) // delay .25s + original pulse .10s
            val original=pcm(id,18000)
            command(id,"split_layer_clip","object" to 1,"frame" to 15);assertArrayEquals(original,pcm(id,18000),0f)
            command(id,"move_layer_clip","object" to 2,"in_frame" to 30);assertArrayEquals(original,pcm(id,30000),0f)
            command(id,"set_audio","object" to 2,"volume" to .5);assertArrayEquals(original.map{it*.5f}.toFloatArray(),pcm(id,30000),0f)
            command(id,"flags","object" to 2,"visible" to false,"locked" to false);assertActive(pcm(id,30000))
            command(id,"set_audio","object" to 2,"muted" to true);assertSilent(pcm(id,30000))
            val audio=imported.getJSONObject("task").getJSONObject("metadata").getJSONObject("audio")
            assertEquals(1,File(root,"assets").listFiles()!!.size)
            File(root,"audio-report.json").writeText(audio.toString())
        } finally {NativeBridge.destroy(id)}
    }

    @Test fun variableFrameTimesRotationAndLatestRequestsReturnActualFrames() {
        for(name in listOf("variable.mp4","rotated-90.mp4")) {
            val root=root();val id=create(root)
            try {
                val imported=import(id,name);val video=imported.getJSONObject("task").getJSONObject("metadata").getJSONObject("video")
                if(name=="variable.mp4")assertTrue(video.getBoolean("variable_frame_rate")) else {assertEquals(90,video.getInt("rotation"));assertEquals(144,video.getInt("display_width"));assertEquals(256,video.getInt("display_height"))}
                val samples=JSONArray()
                for((n,time)in listOf(0.0,6.0,30.0,12.0,42.0).withIndex()) {
                    val value=frame(id,1,time,(n+1).toLong());samples.put(value.first);File(root,"$name-$n.rgba").writeBytes(value.second)
                }
                for(n in 0..29)req(id,"request_video_frame","object" to 1,"frame" to (n%20).toDouble(),"sequence" to 100L+n)
                val final=frame(id,1,15.0,200);assertEquals(200L,final.first.getLong("sequence"));assertTrue(final.first.getLong("pts_us")<=250000L&&250000L<final.first.getLong("end_us"))
                assertFalse(JSONObject(MediaBridge.readVideoFrameInto(id,1,129,ByteBuffer.allocateDirect(256*144*4))).getBoolean("ok"))
                val stale=JSONObject(MediaBridge.request(id,context,JSONObject().put("op","request_video_frame").put("object",1).put("frame",180.0).put("sequence",129).toString()))
                assertFalse(stale.getBoolean("ok"));assertArrayEquals(final.second,frame(id,1,15.0,200).second)
                File(root,"video-report.json").writeText(JSONObject().put("metadata",imported.getJSONObject("task").getJSONObject("metadata")).put("samples",samples).toString())
            } finally {NativeBridge.destroy(id)}
        }
    }

    @Test fun twoInstancesKeepDistinctFramesAndFrozenReaderSurvivesClose() {
        val root=root();val id=create(root);var handle=0L
        try {
            import(id,"silent-24fps.mp4");command(id,"duplicate","object" to 1);command(id,"move_layer_clip","object" to 2,"in_frame" to 30)
            val left=frame(id,1,45.0,1);val right=frame(id,2,45.0,1)
            assertTrue(pixel(left.second,256,128,72)[1]>180);assertTrue(pixel(right.second,256,128,72)[0]>180)
            command(id,"move_layer_clip","object" to 2,"in_frame" to 15)
            assertFalse(JSONObject(MediaBridge.readVideoFrameInto(id,2,1,ByteBuffer.allocateDirect(256*144*4))).getBoolean("ok"))
            data(NativeBridge.history(id,0));assertArrayEquals(right.second,frame(id,2,45.0,1).second)
            handle=data(MediaBridge.freezeVideo(id)).getLong("handle");req(id,"release_video_frames")
            NativeBridge.destroy(id)
            var failure:Throwable?=null
            val thread=Thread {try {
                val deadline=System.nanoTime()+15_000_000_000L;var result=data(MediaBridge.requestFrozenVideoFrame(handle,2,45.0,1))
                while(result.getString("state")=="pending"){assertTrue(System.nanoTime()<deadline);Thread.sleep(5);result=data(MediaBridge.requestFrozenVideoFrame(handle,2,45.0,1))}
                assertEquals(result.toString(),"ready",result.getString("state"));val b=ByteBuffer.allocateDirect(256*144*4)
                data(MediaBridge.readFrozenVideoFrameInto(handle,2,1,b));val bytes=ByteArray(b.capacity());b.get(bytes);assertArrayEquals(right.second,bytes)
            }catch(e:Throwable){failure=e}}
            thread.start();thread.join(20000);assertFalse(thread.isAlive);failure?.let{throw it}
        } finally {if(handle>0)data(MediaBridge.releaseFrozenVideo(handle));NativeBridge.destroy(id)}
    }

    @Test fun probeCancelFaultAndRebuiltCachesDoNotPublishPartialImports() {
        val root=root();val id=create(root)
        try {
            req(id,"probe_media","request_id" to "probe","kind" to "video","uri" to "content://com.motionstudio.editor.test.audio-fixtures/silent-24fps.mp4")
            assertEquals("succeeded",await(id,"probe").getString("state"));assertEquals(0,data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").length())
            req(id,"import_media","request_id" to "cancel","kind" to "video","uri" to "content://com.motionstudio.editor.test.audio-fixtures/silent-24fps.mp4")
            assertEquals("ready",await(id,"cancel").getString("state"));assertEquals("cancelled",req(id,"cancel_media_import","request_id" to "cancel").getString("state"));req(id,"finish_media_import","request_id" to "cancel")
            req(id,"import_media","request_id" to "invalid","kind" to "video","uri" to "content://com.motionstudio.editor.test.audio-fixtures/invalid.bin")
            assertEquals("failed",await(id,"invalid").getString("state"));assertEquals(0,data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").length())
            import(id,"sound-24fps.mp4","valid");assertActive(pcm(id,6000))
            req(id,"release_video_frames");File(root,"cache/video-v1").listFiles()!!.forEach{assertTrue(it.delete())}
            req(id,"prepare_video","request_id" to "rebuild","asset" to 1);assertEquals("succeeded",await(id,"rebuild").getString("state"));val value=frame(id,1,15.0,1)
            val tooSmall=ByteBuffer.allocateDirect(16).apply{for(i in 0..15)put(i,85)}
            assertFalse(JSONObject(MediaBridge.readVideoFrameInto(id,1,1,tooSmall)).getBoolean("ok"));for(i in 0..15)assertEquals(85,tooSmall.get(i).toInt())
            assertFalse(JSONObject(MediaBridge.readVideoFrameInto(id,1,1,ByteBuffer.allocateDirect(value.second.size).asReadOnlyBuffer())).getBoolean("ok"))
            val outside=req(id,"request_video_frame","object" to 1,"frame" to 180.0,"sequence" to 2);assertEquals("outside",outside.getString("state"))
        } finally {NativeBridge.destroy(id)}
    }

    @Test fun largeOwnedSourcesAndPackageReopenExceedOldImageLimits() {
        val root=root();val id=create(root)
        val input=File(context.filesDir,"large-video-"+UUID.randomUUID()+".mp4")
        try {
            val size=128L*1024*1024+1024
            val testContext=InstrumentationRegistry.getInstrumentation().context
            testContext.assets.open("video/silent-24fps.mp4").use { source->input.outputStream().use {source.copyTo(it,65536)} }
            // A valid MP4 free box tests file/package streaming, not decode complexity.
            val padding=size-input.length()
            java.io.DataOutputStream(java.io.FileOutputStream(input,true)).use { out->
                out.writeInt(padding.toInt());out.writeBytes("free")
                var remaining=padding-8;val block=ByteArray(65536)
                while(remaining>0){val n=minOf(remaining,block.size.toLong()).toInt();out.write(block,0,n);remaining-=n}
            }
            val began=System.nanoTime()
            for(n in 1..2) {
                val key="large-$n"
                req(id,"import_media","kind" to "video","request_id" to key,"uri" to android.net.Uri.fromFile(input).toString())
                assertEquals("ready",await(id,key).getString("state"))
                val imported=req(id,"finish_media_import","request_id" to key)
                assertEquals("succeeded",imported.getJSONObject("task").getString("state"))
                assertEquals(size,imported.getJSONObject("task").getJSONObject("metadata").getJSONObject("video").getLong("bytes"))
            }
            val importUs=(System.nanoTime()-began)/1000
            assertEquals(size*2,File(root,"assets").listFiles()!!.sumOf{it.length()})
            assertTrue(input.delete())
            val first=frame(id,1,45.0,1);assertTrue(pixel(first.second,256,128,72)[1]>180)
            val archive=data(NativeBridge.pack(id)).getString("path")
            java.util.zip.ZipFile(archive).use { zip->
                val entries=zip.entries().asSequence().toList()
                assertEquals(3,entries.size);assertEquals(size*2,entries.filter{it.name!="project.json"}.sumOf{it.size})
            }
            val restored=data(NativeBridge.importProject(id,archive));assertEquals(2,restored.getJSONObject("project").getJSONArray("layers").length())
            val restoredRoot=File(restored.getString("root"));assertFalse(File(restoredRoot,"cache/video-v1").exists())
            for(asset in 1L..2L){val key="restore-$asset";req(id,"prepare_video","request_id" to key,"asset" to asset);assertEquals("succeeded",await(id,key).getString("state"))}
            assertArrayEquals(first.second,frame(id,2,45.0,1).second)
            File(root,"large-report.json").writeText(JSONObject().put("source_bytes",size).put("project_source_bytes",size*2).put("import_us",importUs).put("restored_root",restoredRoot.absolutePath).toString())
        } finally {NativeBridge.destroy(id);if(input.exists())assertTrue(input.delete())}
    }
}
