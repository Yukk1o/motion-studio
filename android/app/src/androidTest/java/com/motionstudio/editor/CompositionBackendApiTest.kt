package com.motionstudio.editor

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.ImageFormat
import android.hardware.HardwareBuffer
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
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
class CompositionBackendApiTest {
    private val context get()=InstrumentationRegistry.getInstrumentation().targetContext
    private fun root()=File(context.filesDir,"acceptance/compositions-"+UUID.randomUUID()).apply{mkdirs()}
    private fun data(raw:String)=nativeData(raw)
    private fun array(vararg v:Any)=JSONArray(v.toList())
    private fun track(v:Any)=JSONObject().put("value",v).put("keys",JSONArray())
    private fun layer(id:Int,y:Int,color:JSONArray)=JSONObject().put("id",id).put("name","Layer $id").put("visible",true).put("locked",false).put("three_d",false)
        .put("size",array(150,50)).put("content",JSONObject().put("kind","solid").put("color",color))
        .put("transform",JSONObject().put("position",track(array(128,y,0))).put("rotation",track(array(0,0,0))).put("scale",track(array(100,100,100))).put("opacity",track(1)).put("anchor",array(.5,.5)))
    private fun fixture()=data(NativeBridge.projectTemplate(0)).put("width",256).put("height",144).put("fps",30).put("frames",24)
        .put("background",array(.04,.05,.07,1)).put("assets",JSONArray()).put("layers",JSONArray().put(layer(1,36,array(1,.1,.05,.6))).put(layer(2,108,array(.05,.1,1,.7))))
    private fun request(id:Long,composition:String,op:String,vararg fields:Pair<String,Any>)=JSONObject().put("version",1).put("composition",composition).put("op",op).apply{fields.forEach{put(it.first,it.second)}}
    private fun req(id:Long,c:String,op:String,vararg fields:Pair<String,Any>)=data(CompositionBridge.request(id,request(id,c,op,*fields).toString()))
    private fun action(id:Long,c:String,kind:String,vararg fields:Pair<String,Any>)=req(id,c,"action","action" to JSONObject().put("kind",kind).apply{fields.forEach{put(it.first,it.second)}})
    private fun result(state:JSONObject)=state.getJSONArray("edit_results").getJSONObject(0).getJSONObject("result")
    private fun capture(id:Long,c:String,frame:Double):Bitmap {req(id,c,"seek","frame" to frame);return BitmapFactory.decodeFile(data(CompositionBridge.capture(id,c)).getString("path"))}
    private fun error(id:Long,c:String,kind:String,vararg fields:Pair<String,Any>):JSONObject=JSONObject(CompositionBridge.request(id,request(id,c,"action","action" to JSONObject().put("kind",kind).apply{fields.forEach{put(it.first,it.second)}}).toString())).also{assertFalse(it.toString(),it.getBoolean("ok"))}.getJSONObject("error_detail")
    @Test fun contextAtomicHistorySettingsAndPackagesPreserveTheGraph() {
        val root=root();val id=NativeBridge.create(root.absolutePath,fixture().toString());assertTrue(NativeBridge.creationError(),id>0)
        var child=""
        try {
            val original=data(NativeBridge.state(id));assertTrue(original.getJSONObject("capabilities").getBoolean("multiple_compositions"))
            child=result(action(id,"comp-main","precompose","objects" to array(1,2),"name" to "Child")).getString("composition")
            assertEquals(1,req(id,"comp-main","state").getJSONObject("project").getJSONArray("layers").length())
            assertEquals("composition_in_use",error(id,"comp-main","delete","target" to child).getString("code"))
            val parent=req(id,"comp-main","state");assertEquals(array(3).toString(),parent.getJSONObject("composition_context").getJSONArray("selection").toString())
            req(id,child,"open","path" to array("comp-main",child));assertEquals(2,req(id,child,"state").getJSONObject("project").getJSONArray("layers").length())
            assertEquals("cycle",error(id,child,"reference","target" to "comp-main").getString("code"))
            req(id,child,"context","selection" to array(2),"timeline" to JSONObject().put("zoom",2))
            CompositionBridge.command(id,child,JSONObject().put("op","rename").put("object",2).put("name","Edited child"))
            req(id,"comp-main","open");req(id,child,"open","path" to array("comp-main",child));assertEquals(array(2).toString(),req(id,child,"state").getJSONObject("composition_context").getJSONArray("selection").toString())
            val settings=JSONObject().put("name","60 fps child").put("width",256).put("height",144).put("fps",60).put("frames",48).put("timing","preserve_seconds").put("shorten","reject")
            val preview=req(id,child,"settings_preview","settings" to settings);assertTrue(preview.toString(),preview.getBoolean("valid"))
            req(id,child,"settings_apply","settings" to settings,"expected_revision" to preview.getLong("expected_revision"))
            val stale=JSONObject(CompositionBridge.request(id,request(id,child,"settings_apply","settings" to settings,"expected_revision" to preview.getLong("expected_revision")).toString()))
            assertEquals("stale_revision",stale.getJSONObject("error_detail").getString("code"))
            data(NativeBridge.save(id));data(NativeBridge.pack(id))
        }finally{NativeBridge.destroy(id)}
        val reopened=NativeBridge.create(root.absolutePath,"");assertTrue(reopened>0)
        try{assertEquals(2,req(reopened,"comp-main","list").getJSONArray("compositions").length());assertEquals(60,req(reopened,child,"info").getJSONObject("composition").getInt("fps"))}finally{NativeBridge.destroy(reopened)}
    }
    @Test fun twoLevelPixelsExportAnd3dReferenceAgreeWithPreview() {
        val root=root();val id=NativeBridge.create(root.absolutePath,fixture().toString());assertTrue(NativeBridge.creationError(),id>0)
        try {
            val packages=data(CompositionBridge.plugin(id,"comp-main",JSONObject().put("op","catalogue"))).getJSONArray("packages").objects()
            val pkg=packages.last{it.getJSONObject("manifest").getJSONArray("effects").objects().any{d->d.getString("id")=="tint"}}
            val manifest=pkg.getJSONObject("manifest")
            data(CompositionBridge.plugin(id,"comp-main",JSONObject().put("op","add").put("object",1).put("effect","tint").put("plugin",manifest.getString("id")).put("version",manifest.getString("version")).put("hash",pkg.getString("hash"))))
            val before=capture(id,"comp-main",12.0)
            action(id,"comp-main","precompose","objects" to array(1,2),"name" to "inside")
            action(id,"comp-main","precompose","objects" to array(3),"name" to "outside")
            val after=capture(id,"comp-main",12.0);assertBitmap(before,after,3.0)
            val tiny=ByteBuffer.allocateDirect(32).order(ByteOrder.LITTLE_ENDIAN)
            val need=CompositionBridge.sampleFrameBundleInto(id,"comp-main",12.0,tiny);assertTrue(need < -32)
            val bundle=ByteBuffer.allocateDirect(-need).order(ByteOrder.LITTLE_ENDIAN);assertEquals(-need,CompositionBridge.sampleFrameBundleInto(id,"comp-main",12.0,bundle));assertEquals(3,bundle.getInt(8))
            val project=CompositionBridge.freezeProject(id,"comp-main");val movie=VideoExporter(root,project).run{_,_->}
            val reader=MediaMetadataRetriever()
            try{reader.setDataSource(movie.absolutePath);assertEquals("24",reader.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_FRAME_COUNT));assertBitmap(after,reader.getFrameAtIndex(12)!!,6.0)}finally{reader.release()}
            CompositionBridge.command(id,"comp-main",JSONObject().put("op","set_layer_3d").put("object",4).put("enabled",true))
            assertTrue(req(id,"comp-main","state").getJSONObject("project").getJSONArray("layers").getJSONObject(0).getBoolean("three_d"))
            data(CompositionBridge.history(id,"comp-main",0));data(CompositionBridge.history(id,"comp-main",0));data(CompositionBridge.history(id,"comp-main",0))
            assertEquals(2,req(id,"comp-main","state").getJSONObject("project").getJSONArray("layers").length())
            assertEquals(0,req(id,"comp-main","state").getJSONObject("composition_context").getJSONArray("selection").length())
            data(CompositionBridge.history(id,"comp-main",1));data(CompositionBridge.history(id,"comp-main",1));assertBitmap(after,capture(id,"comp-main",12.0),3.0)
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun childMediaScopesFrozenPcmAndNestedVideoExportWorkTogether() {
        val root=root();val p=fixture().put("layers",JSONArray());val id=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),id>0)
        try {
            val settings=JSONObject().put("name","Video child").put("width",256).put("height",144).put("fps",60).put("frames",48)
            val child=result(action(id,"comp-main","create","settings" to settings)).getString("composition");req(id,child,"open")
            fun media(op:String,vararg fields:Pair<String,Any>)=data(CompositionBridge.media(id,child,context,JSONObject().put("op",op).apply{fields.forEach{put(it.first,it.second)}}))
            media("import_media","request_id" to "child-video","kind" to "video","uri" to "content://com.motionstudio.editor.test.audio-fixtures/sound-24fps.mp4","with_audio" to true)
            val deadline=System.nanoTime()+45_000_000_000L
            while(media("media_status","request_id" to "child-video").getString("state")=="running"){assertTrue(System.nanoTime()<deadline);Thread.sleep(10)}
            req(id,"comp-main","open")
            val wrong=JSONObject(MediaBridge.request(id,context,JSONObject().put("op","finish_media_import").put("request_id","child-video").toString()))
            assertEquals("context_mismatch",wrong.getJSONObject("error_detail").getString("code"))
            req(id,child,"open");val imported=media("finish_media_import","request_id" to "child-video").getJSONObject("task");assertEquals(imported.toString(),"succeeded",imported.getString("state"))
            req(id,"comp-main","open");action(id,"comp-main","reference","target" to child);action(id,"comp-main","precompose","objects" to array(1),"name" to "outer")
            assertTrue(req(id,"comp-main","state").getBoolean("has_audio"))
            val consumer=HandlerThread("composition-preview-consumer").apply{start()}
            // PRIVATE accepts the native window format chosen by Vulkan or GLES.
            val surface=ImageReader.newInstance(256,144,ImageFormat.PRIVATE,3,HardwareBuffer.USAGE_GPU_SAMPLED_IMAGE)
            surface.setOnImageAvailableListener({r->r.acquireLatestImage()?.close()},Handler(consumer.looper))
            try {
                data(NativeBridge.surface(id,surface.surface,256,144))
                for(frame in listOf(0.0,1.0,2.0,3.0,4.0,18.0,12.0,6.0)) {
                    val until=System.nanoTime()+15_000_000_000L
                    while(!CompositionBridge.render(id,"comp-main",frame)){assertTrue(data(NativeBridge.state(id)).toString(),System.nanoTime()<until);Thread.sleep(5)}
                }
                val png=capture(id,"comp-main",6.0);assertTrue(png.width==256&&png.height==144)
                assertTrue("Nested source frame should be red",(png.getPixel(128,72) shr 16 and 255)>180)
                data(NativeBridge.surface(id,null,0,0))
            }finally{surface.close();consumer.quitSafely();consumer.join(2000)}

            val handle=data(MediaBridge.freezeAudio(id)).getLong("handle")
            try{val pcm=ByteBuffer.allocateDirect(4096*8).order(ByteOrder.LITTLE_ENDIAN);data(MediaBridge.readFrozenPcmInto(handle,4800,4096,pcm));assertTrue(FloatArray(8192).also{pcm.asFloatBuffer().get(it)}.any{abs(it)>.01f})}finally{data(MediaBridge.releaseFrozenAudio(handle))}
            val video=VideoExporter(root,CompositionBridge.freezeProject(id,"comp-main")).run{_,_->}
            val reader=MediaMetadataRetriever();try{reader.setDataSource(video.absolutePath);assertEquals("24",reader.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_FRAME_COUNT));assertEquals("yes",reader.extractMetadata(MediaMetadataRetriever.METADATA_KEY_HAS_AUDIO));assertNotNull(reader.getFrameAtIndex(12))}finally{reader.release()}
        }finally{NativeBridge.destroy(id)}
    }
    private fun assertBitmap(a:Bitmap,b:Bitmap,limit:Double) {
        assertEquals(a.width,b.width);assertEquals(a.height,b.height);val aa=IntArray(a.width*a.height);val bb=aa.clone();a.getPixels(aa,0,a.width,0,0,a.width,a.height);b.getPixels(bb,0,b.width,0,0,b.width,b.height)
        var sum=0L;for(i in aa.indices)for(shift in listOf(0,8,16))sum+=abs((aa[i] shr shift and 255)-(bb[i] shr shift and 255));val mae=sum.toDouble()/(aa.size*3);assertTrue("RGB MAE $mae exceeds $limit",mae<=limit)
    }
}
