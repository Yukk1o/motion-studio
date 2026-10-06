package com.motionstudio.editor

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
class MediaFormatApiTest {
    private val context get()=InstrumentationRegistry.getInstrumentation().targetContext
    private fun data(raw:String):JSONObject {val r=JSONObject(raw);assertTrue(r.optString("error"),r.optBoolean("ok"));return r.getJSONObject("data")}
    private fun req(id:Long,op:String,vararg fields:Pair<String,Any>)=data(MediaBridge.request(id,context,JSONObject().put("op",op).apply{fields.forEach{put(it.first,it.second)}}.toString()))
    private fun root()=File(context.filesDir,"acceptance/media-formats-"+UUID.randomUUID()).apply{mkdirs()}
    private fun create(root:File):Long {val p=data(NativeBridge.projectTemplate(0)).put("width",256).put("height",144).put("fps",60).put("frames",240).put("layers",JSONArray());val id=NativeBridge.create(root.absolutePath,p.toString());assertTrue(id>0);return id}
    private fun await(id:Long,key:String):JSONObject {val deadline=System.nanoTime()+60_000_000_000L;while(true){val t=req(id,"media_status","request_id" to key);if(t.getString("state")!="running")return t;assertTrue("media timeout",System.nanoTime()<deadline);Thread.sleep(10)}}
    private fun import(id:Long,name:String,kind:String,key:String="import"):JSONObject {
        req(id,"import_media","kind" to kind,"request_id" to key,"uri" to "content://com.motionstudio.editor.test.audio-fixtures/formats/$name")
        val t=await(id,key);assertEquals(name+": "+t.toString(),"ready",t.getString("state"));val c=req(id,"finish_media_import","request_id" to key);assertEquals(c.toString(),"succeeded",c.getJSONObject("task").getString("state"));return c
    }
    private fun pcm(id:Long,start:Long,frames:Int=4096):FloatArray {
        val b=ByteBuffer.allocateDirect(frames*8).order(ByteOrder.LITTLE_ENDIAN);data(MediaBridge.readPcmInto(id,start,frames,b));return FloatArray(frames*2).also{b.asFloatBuffer().get(it)}
    }
    private fun frame(id:Long,time:Double,seq:Long):Pair<JSONObject,ByteArray> {
        val deadline=System.nanoTime()+20_000_000_000L
        var s=req(id,"request_video_frame","object" to 1,"frame" to time,"sequence" to seq)
        while(s.getString("state")=="pending"){assertTrue("video frame timeout",System.nanoTime()<deadline);Thread.sleep(5);s=req(id,"request_video_frame","object" to 1,"frame" to time,"sequence" to seq)}
        assertEquals(s.toString(),"ready",s.getString("state"));val b=ByteBuffer.allocateDirect(256*144*4);val read=data(MediaBridge.readVideoFrameInto(id,1,seq,b));s.put("source_transfer",read.getString("source_transfer")).put("decoder",read.getString("decoder"));return s to ByteArray(b.capacity()).also{b.get(it)}
    }
    @Test fun capabilityQueryReportsDeviceDecodersAndBackendRestrictionsSeparately() {
        val caps=req(0,"media_capabilities");assertEquals(1,caps.getInt("schema_version"));assertTrue(caps.getBoolean("requires_probe"));assertFalse(caps.getBoolean("decoder_presence_guarantees_file_support"));assertTrue(caps.getJSONArray("decoders").length()>0)
        assertEquals(192000,caps.getJSONObject("audio").getInt("max_sample_rate"));assertEquals(4,caps.getJSONObject("video").getJSONArray("mime_types").length());assertFalse(caps.getJSONObject("video").getBoolean("hdr"))
        File(root(),"capabilities.json").writeText(caps.toString(2))
    }
    @Test fun platformExtractorMetadataIsRecordedForIndependentParityAnalysis() {
        val report=JSONObject()
        for(name in listOf("avc.mkv","vp8.webm","vp9.webm","vp9-delayed.webm")) {
            val uri=android.net.Uri.parse("content://com.motionstudio.editor.test.audio-fixtures/formats/$name")
            context.contentResolver.openAssetFileDescriptor(uri,"r")!!.use {fd->
                val ex=android.media.MediaExtractor()
                try {
                    ex.setDataSource(fd.fileDescriptor,fd.startOffset,fd.length)
                    val tracks=JSONArray()
                    for(i in 0 until ex.trackCount) {
                        val format=ex.getTrackFormat(i);val t=JSONObject().put("track",i).put("format",format.toString());ex.selectTrack(i)
                        val pts=JSONArray();repeat(4){pts.put(ex.sampleTime);ex.advance()};t.put("initial_pts_us",pts);ex.unselectTrack(i);ex.seekTo(0,android.media.MediaExtractor.SEEK_TO_PREVIOUS_SYNC);tracks.put(t)
                    }
                    report.put(name,tracks)
                }finally{ex.release()}
            }
        }
        val output=File(root(),"extractor.json");output.writeText(report.toString(2));android.util.Log.i("MediaFormatMetadata",report.toString())
    }
    @Test fun newAudioFormatsActuallyDecodePcmWaveformsAndRebuildOwnedSources() {
        for(name in listOf("pcm8.wav","pcm24.wav","pcm32.wav","float32.wav","float64.wav","lossless.flac","lossless.m4a","vorbis.ogg","linear.aiff","opus.ogg","adts.aac")) {
            val root=root();var id=create(root)
            try {
                val t=import(id,name,"audio");val a=t.getJSONObject("task").getJSONObject("metadata");assertTrue(name+": "+a,abs(a.getLong("duration_us")-2_000_000L)<160000L)
                val samples=pcm(id,10000);assertTrue(name,samples.any{abs(it)>.1f})
                val wave=req(id,"audio_waveform","asset" to a.getLong("id"),"count" to 4096);assertEquals(10000,wave.getInt("bucket_duration_us"));assertTrue(wave.getJSONArray("buckets").length()>=195)
                File(root,"reference.f32").outputStream().use {f->val b=ByteBuffer.allocate(samples.size*4).order(ByteOrder.LITTLE_ENDIAN);b.asFloatBuffer().put(samples);f.write(b.array())}
                val frozen=data(MediaBridge.freezeAudio(id)).getLong("handle")
                data(NativeBridge.save(id));NativeBridge.destroy(id);id=createOrReopen(root)
                val cache=File(root,"cache/audio-v1/"+File(a.getString("path")).nameWithoutExtension+".pcm");assertTrue(cache.delete())
                req(id,"prepare_audio","request_id" to "rebuild","asset" to a.getLong("id"));assertEquals(await(id,"rebuild").toString(),"succeeded",await(id,"rebuild").getString("state"));assertArrayEquals(samples,pcm(id,10000),0f)
                val b=ByteBuffer.allocateDirect(samples.size*4).order(ByteOrder.LITTLE_ENDIAN);data(MediaBridge.readFrozenPcmInto(frozen,10000,4096,b));val actual=FloatArray(samples.size);b.asFloatBuffer().get(actual);assertArrayEquals(samples,actual,0f);data(MediaBridge.releaseFrozenAudio(frozen))
                File(root,"audio-report.json").writeText(JSONObject().put("fixture",name).put("asset",a).toString(2))
            }finally{NativeBridge.destroy(id)}
        }
    }
    private fun createOrReopen(root:File):Long {val id=NativeBridge.create(root.absolutePath,"");assertTrue(id>0);return id}
    @Test fun commonContainersAndVideoCodecsReturnActualFramesWithOriginalAudio() {
        for(name in listOf("avc.mov","avc.mkv","hevc.mp4","hevc.mkv","vp8.webm","vp8-601.webm","vp9.webm","vp9-delayed.webm")) {
            val root=root();var id=create(root)
            try {
                val t=import(id,name,"video");val m=t.getJSONObject("task").getJSONObject("metadata");val a=m.getJSONObject("video");assertEquals(1,File(root,"assets").listFiles()!!.size);assertTrue(m.has("audio"));
                val samples=JSONArray()
                for((i,time)in listOf(3.0,45.0,15.0,75.0).withIndex()) {val pair=frame(id,time,i+1L);samples.put(pair.first);File(root,"frame-$i.rgba").writeBytes(pair.second)}
                val audio=pcm(id,if(name=="vp9-delayed.webm")18000 else 8000);assertTrue(name,audio.any{abs(it)>.1f})
                if(name=="vp9-delayed.webm")assertTrue(pcm(id,7000,1000).all{abs(it)<.01f})
                data(NativeBridge.save(id));NativeBridge.destroy(id);id=createOrReopen(root);assertArrayEquals(audio,pcm(id,if(name=="vp9-delayed.webm")18000 else 8000),0f)
                val stem=File(a.getString("path")).nameWithoutExtension
                assertTrue(File(root,"cache/video-v1/$stem.pts").delete());req(id,"prepare_video","request_id" to "rebuild","asset" to a.getLong("id"));assertEquals("succeeded",await(id,"rebuild").getString("state"));frame(id,45.0,1)
                File(root,"video-report.json").writeText(JSONObject().put("fixture",name).put("metadata",m).put("samples",samples).toString(2))
            }finally{NativeBridge.destroy(id)}
        }
    }
    @Test fun unsupportedBitDepthsFailWithoutChangingProjectOrHistory() {
        for(name in listOf("reject-hevc10.mp4","reject-vp9-10.webm")) {
            val root=root();val id=create(root)
            try {
                val before=data(NativeBridge.state(id)).getJSONObject("project").toString()
                req(id,"import_media","kind" to "video","request_id" to "reject","uri" to "content://com.motionstudio.editor.test.audio-fixtures/formats/$name")
                val t=await(id,"reject");assertEquals(t.toString(),"failed",t.getString("state"));assertEquals(before,data(NativeBridge.state(id)).getJSONObject("project").toString());assertFalse(data(NativeBridge.state(id)).getBoolean("canUndo"))
            }finally{NativeBridge.destroy(id)}
        }
    }
}
