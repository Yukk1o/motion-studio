package com.motionstudio.editor

import android.content.Context
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

@RunWith(AndroidJUnit4::class)
class AudioBackendApiTest {
    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext
    private fun data(raw: String): JSONObject { val r=JSONObject(raw); assertTrue(r.optString("error"),r.optBoolean("ok")); return r.getJSONObject("data") }
    private fun root()=File(context.filesDir,"acceptance/audio-api-"+UUID.randomUUID()).apply{mkdirs()}
    private fun req(id:Long,op:String,vararg fields:Pair<String,Any>):JSONObject = data(MediaBridge.request(id,context,
        JSONObject().put("op",op).apply{fields.forEach{put(it.first,it.second)}}.toString()))
    private fun await(id:Long,request:String):JSONObject {
        val limit=System.nanoTime()+30_000_000_000L
        while(true){val t=req(id,"media_status","request_id" to request);if(t.getString("state")!="running")return t
            assertTrue("Audio task timeout",System.nanoTime()<limit);Thread.sleep(10)}
    }
    private fun create(root:File):Long {
        val p=data(NativeBridge.projectTemplate(0)).put("version",4).put("width",256).put("height",256).put("frames",180).put("layers",JSONArray())
        val id=NativeBridge.create(root.absolutePath,p.toString());assertTrue(id>0);return id
    }
    private fun import(id:Long,name:String,request:String,at:Int=0):JSONObject {
        req(id,"import_media","request_id" to request,"kind" to "audio","uri" to "content://com.motionstudio.editor.test.audio-fixtures/$name","at_frame" to at)
        val ready=await(id,request);assertEquals(ready.toString(),"ready",ready.getString("state"))
        val committed=req(id,"finish_media_import","request_id" to request);assertEquals(committed.toString(),"succeeded",committed.getJSONObject("task").getString("state"));return committed
    }
    private fun pcm(id:Long,start:Long,frames:Int=4096):FloatArray {
        val b=ByteBuffer.allocateDirect(frames*8).order(ByteOrder.LITTLE_ENDIAN);val result=data(MediaBridge.readPcmInto(id,start,frames,b));assertEquals(frames,result.getInt("frames"));return FloatArray(frames*2).also{b.asFloatBuffer().get(it)}
    }
    @Test fun actualContentUrisDecodeAllFormatsAndAudioSurvivesReopen() {
        val root=root();var id=create(root)
        try{
            val report=JSONArray()
            for((index,name) in listOf("tone-stereo-48000.wav","tone-mono-44100.mp3","tone-stereo-48000.m4a").withIndex()) {
                val result=import(id,name,"format-$index",30);val asset=result.getJSONObject("task").getJSONObject("metadata")
                assertTrue(kotlin.math.abs(asset.getLong("duration_us")-2_000_000L)<=1000L)
                val state=result.getJSONObject("state");assertEquals(0,state.getJSONArray("projectedLayers").length())
                val wave=req(id,"audio_waveform","asset" to asset.getLong("id"),"first_bucket" to 0,"count" to 25)
                assertEquals(10000,wave.getInt("bucket_duration_us"));assertTrue(wave.getJSONArray("buckets").getJSONObject(15).getDouble("rms")>.1)
                report.put(asset)
            }
            assertTrue(pcm(id,48000+5000).any{kotlin.math.abs(it)>.2})
            val frozen=data(MediaBridge.freezeAudio(id));val handle=frozen.getLong("handle")
            try {
                val ref=ByteBuffer.allocateDirect(4096*8).order(ByteOrder.LITTLE_ENDIAN);data(MediaBridge.readFrozenPcmInto(handle,53000,4096,ref))
                data(NativeBridge.command(id,"{\"op\":\"set_audio\",\"object\":1,\"muted\":true}"))
                val after=ByteBuffer.allocateDirect(4096*8).order(ByteOrder.LITTLE_ENDIAN);data(MediaBridge.readFrozenPcmInto(handle,53000,4096,after));assertEquals(ref,after)
            }finally{data(MediaBridge.releaseFrozenAudio(handle))}
            data(NativeBridge.save(id));NativeBridge.destroy(id);id=NativeBridge.create(root.absolutePath,"");assertTrue(id>0)
            assertEquals(3,data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("audio_assets").length())
            assertTrue(pcm(id,54000).any{kotlin.math.abs(it)>.1})
            File(root,"audio-format-report.json").writeText(JSONObject().put("formats",report).put("contentUriProviderOffsets",true).put("reopened",true).toString(2))
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun cancelFailuresUndoAndBufferValidationLeaveProjectsAndSourcesIntact() {
        val root=root();val id=create(root)
        try {
            req(id,"import_media","request_id" to "cancel","kind" to "audio","uri" to "content://com.motionstudio.editor.test.audio-fixtures/tone-stereo-48000.wav")
            assertEquals("ready",await(id,"cancel").getString("state"));assertEquals("cancelled",req(id,"cancel_media_import","request_id" to "cancel").getString("state"))
            assertEquals("cancelled",req(id,"finish_media_import","request_id" to "cancel").getJSONObject("task").getString("state"))
            assertEquals(0,data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").length())
            req(id,"import_media","request_id" to "bad","kind" to "audio","uri" to "content://com.motionstudio.editor.test.audio-fixtures/invalid.bin")
            assertEquals("failed",await(id,"bad").getString("state"))
            val good=import(id,"tone-stereo-48000.wav","good");val asset=good.getJSONObject("task").getJSONObject("metadata");val source=File(root,asset.getString("path"));assertTrue(source.exists())
            data(NativeBridge.history(id,0));assertEquals(0,data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").length());assertTrue(source.exists());data(NativeBridge.history(id,1))
            for(b in listOf(ByteBuffer.allocate(8192),ByteBuffer.allocateDirect(3),ByteBuffer.allocateDirect(8192).asReadOnlyBuffer())) {
                assertFalse(JSONObject(MediaBridge.readPcmInto(id,0,1024,b)).getBoolean("ok"))
            }
            val sentinel=ByteBuffer.allocateDirect(8192);for(i in 0 until sentinel.capacity())sentinel.put(i,0x5a)
            assertFalse(JSONObject(MediaBridge.readPcmInto(id,-1,1024,sentinel)).getBoolean("ok"));assertTrue((0 until sentinel.capacity()).all{sentinel.get(it)==0x5a.toByte()})
            req(id,"import_media","request_id" to "missing","kind" to "audio","uri" to "content://com.motionstudio.editor.test.audio-fixtures/missing.wav")
            assertEquals("failed",await(id,"missing").getString("state"))
            assertFalse(JSONObject(NativeBridge.command(id,"{\"op\":\"set_layer_3d\",\"object\":1,\"enabled\":true}")).getBoolean("ok"))
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun uriOpeningIsAsyncAndCancellationCannotPublishAfterTheProviderReturns() {
        val root=root();val id=create(root)
        try {
            val start=System.nanoTime()
            req(id,"import_media","request_id" to "slow","kind" to "audio","uri" to "content://com.motionstudio.editor.test.audio-fixtures/slow.wav")
            assertTrue("Provider opening blocked editor queue",System.nanoTime()-start<1_000_000_000L)
            assertEquals(0,data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").length())
            assertEquals("cancelled",req(id,"cancel_media_import","request_id" to "slow").getString("state"))
            Thread.sleep(1700)
            assertEquals("cancelled",req(id,"finish_media_import","request_id" to "slow").getJSONObject("task").getString("state"))
            assertEquals(0,data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").length())
            assertFalse(root.listFiles().orEmpty().any{it.name.startsWith(".media-import-")})
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun probeDoesNotCommitAndFrozenPcmRemainsReadableOnAnotherThreadAfterClose() {
        val root=root();val id=create(root)
        var closed=false
        try {
            req(id,"probe_media","request_id" to "probe","kind" to "audio","uri" to "content://com.motionstudio.editor.test.audio-fixtures/tone-stereo-48000.m4a")
            assertEquals("succeeded",await(id,"probe").getString("state"))
            assertEquals(0,data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").length())
            req(id,"release_media_task","request_id" to "probe")
            import(id,"tone-stereo-48000.wav","import")
            val frozen=data(MediaBridge.freezeAudio(id));val handle=frozen.getLong("handle");val total=frozen.getLong("total_frames")
            val output=ByteBuffer.allocateDirect(1024*8).order(ByteOrder.LITTLE_ENDIAN)
            for(i in 0 until output.capacity())output.put(i,0x5a)
            val tail=data(MediaBridge.readPcmInto(id,total-17,1024,output));assertEquals(17,tail.getInt("frames"));assertTrue(tail.getBoolean("end_of_stream"));assertTrue((136 until output.capacity()).all{output.get(it)==0x5a.toByte()})
            NativeBridge.destroy(id);closed=true
            val executor=java.util.concurrent.Executors.newSingleThreadExecutor()
            try {
                val values=executor.submit<FloatArray>{val b=ByteBuffer.allocateDirect(4096*8).order(ByteOrder.LITTLE_ENDIAN);data(MediaBridge.readFrozenPcmInto(handle,24000,4096,b));FloatArray(8192).also{b.asFloatBuffer().get(it)}}.get(10,java.util.concurrent.TimeUnit.SECONDS)
                assertTrue(values.any{kotlin.math.abs(it)>.2})
            }finally{executor.shutdownNow();data(MediaBridge.releaseFrozenAudio(handle))}
        }finally{if(!closed)NativeBridge.destroy(id)}
    }
    @Test fun clipEditsAndRebuiltDiskCacheUseTheSameAbsoluteSampleClock() {
        val root=root();val id=create(root)
        try {
            val imported=import(id,"tone-mono-44100.mp3","mp3");val a=imported.getJSONObject("task").getJSONObject("metadata")
            val original=pcm(id,48000)
            data(NativeBridge.command(id,"{\"op\":\"split_layer_clip\",\"object\":1,\"frame\":30}"));assertArrayEquals(original,pcm(id,48000),0f)
            data(NativeBridge.command(id,"{\"op\":\"move_layer_clip\",\"object\":2,\"in_frame\":60}"));assertArrayEquals(original,pcm(id,96000),0f)
            data(NativeBridge.command(id,"{\"op\":\"set_audio\",\"object\":2,\"volume\":0.5}"));val half=pcm(id,96000);assertArrayEquals(original.map{it*.5f}.toFloatArray(),half,0f)
            val cache=File(root,"cache/audio-v1/"+File(a.getString("path")).nameWithoutExtension+".pcm");assertTrue(cache.delete())
            req(id,"prepare_audio","request_id" to "rebuild","asset" to a.getLong("id"));assertEquals("succeeded",await(id,"rebuild").getString("state"))
            // Invalidate the live mixer's revision so it opens the rebuilt file.
            data(NativeBridge.command(id,"{\"op\":\"set_audio\",\"object\":2,\"volume\":1.0}"));assertArrayEquals(original,pcm(id,96000),0f)
        }finally{NativeBridge.destroy(id)}
    }
}
