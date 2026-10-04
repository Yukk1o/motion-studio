package com.motionstudio.editor

import android.graphics.*
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMetadataRetriever
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.*
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class AcceptanceInstrumentedTest {
    private lateinit var root:File
    private var session=0L
    @Before fun setup() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        root=File(context.filesDir,"acceptance/"+UUID.randomUUID()).apply{mkdirs()}
        session=NativeBridge.create(root.absolutePath,"");assertTrue(session>0)
    }
    @After fun teardown() {if(session!=0L)NativeBridge.destroy(session)}
    private fun data(raw:String):JSONObject {
        val envelope=JSONObject(raw)
        assertTrue(envelope.optString("error"),envelope.optBoolean("ok"))
        return envelope.getJSONObject("data")
    }
    private fun command(json:JSONObject)=data(NativeBridge.command(session,json.toString()))
    private fun project()=data(NativeBridge.state(session)).getJSONObject("project")

    @Test fun draggingPreservesScaleRotationDimensionsAndDepth() {
        command(JSONObject().put("op","set_vector").put("object",2).put("property","scale").put("frame",0)
            .put("value",JSONArray(listOf(170,65,100))))
        command(JSONObject().put("op","set_vector").put("object",2).put("property","rotation").put("frame",0)
            .put("value",JSONArray(listOf(0,0,25))))
        val before=project().getJSONArray("layers").getJSONObject(1)
        val position=before.getJSONObject("transform").getJSONObject("position").getJSONArray("value")
        data(NativeBridge.history(session,2))
        repeat(10){data(NativeBridge.drag(session,2,4.0,-2.0,540,960))}
        data(NativeBridge.history(session,3))
        val after=project().getJSONArray("layers").getJSONObject(1)
        val transform=after.getJSONObject("transform")
        assertEquals(before.getJSONArray("size").toString(),after.getJSONArray("size").toString())
        assertEquals(before.getJSONObject("transform").getJSONObject("scale").toString(),transform.getJSONObject("scale").toString())
        assertEquals(before.getJSONObject("transform").getJSONObject("rotation").toString(),transform.getJSONObject("rotation").toString())
        val moved=transform.getJSONObject("position").getJSONArray("value")
        assertEquals(position.getDouble(0)+80,moved.getDouble(0),.01)
        assertEquals(position.getDouble(1)-40,moved.getDouble(1),.01)
        assertEquals(position.getDouble(2),moved.getDouble(2),.001)
        data(NativeBridge.history(session,0))
        assertEquals(before.toString(),project().getJSONArray("layers").getJSONObject(1).toString())
    }

    @Test fun nativeKeyframesUndoAndIndependentObservation() {
        command(JSONObject().put("op","animate").put("object",2).put("property","position").put("frame",0).put("enabled",true))
        command(JSONObject().put("op","set_vector").put("object",2).put("property","position").put("frame",60).put("value",JSONArray(listOf(740,960,0))))
        val middle=data(NativeBridge.seek(session,30.0)).getJSONArray("sampledLayers")
        val layer=(0 until middle.length()).map{middle.getJSONObject(it)}.first{it.getLong("id")==2L}
        assertEquals(640.0,layer.getJSONArray("position").getDouble(0),.01)
        val before=project().toString()
        command(JSONObject().put("op","move_key").put("object",2).put("property","position").put("from",60).put("to",0))
        data(NativeBridge.history(session,0));assertEquals(before,project().toString())
        val revision=data(NativeBridge.state(session)).getLong("revision")
        data(NativeBridge.view(session,2));data(NativeBridge.observe(session,true,10.0,15.0))
        val observed=data(NativeBridge.state(session))
        assertEquals(before,observed.getJSONObject("project").toString())
        assertEquals(revision,observed.getLong("revision"))
        data(NativeBridge.view(session,0))
    }
    @Test fun actualPngAndPackageRoundtripKeepCameraFrameAndResources() {
        val assets=File(root,"assets").apply{mkdirs()}
        val image=Bitmap.createBitmap(32,32,Bitmap.Config.ARGB_8888)
        Canvas(image).drawColor(Color.argb(128,255,0,0))
        File(assets,"alpha.png").outputStream().use{image.compress(Bitmap.CompressFormat.PNG,100,it)}
        image.recycle()
        val cmds=JSONArray().put(JSONObject().put("op","register_asset").put("asset",JSONObject()
            .put("id",11).put("path","assets/alpha.png").put("width",32).put("height",32)))
            .put(JSONObject().put("op","content").put("object",2).put("content",JSONObject().put("kind","image").put("asset",11))
                .put("size",JSONArray(listOf(600,600))))
        data(NativeBridge.command(session,cmds.toString()))
        data(NativeBridge.seek(session,72.0))
        val capture=data(NativeBridge.capture(session))
        val pixels=BitmapFactory.decodeFile(capture.getString("path"))
        assertNotNull(pixels);assertEquals(1080,pixels.width);assertEquals(1920,pixels.height);pixels.recycle()
        data(NativeBridge.save(session));val before=project().toString()
        val pack=data(NativeBridge.pack(session));val archive=File(pack.getString("path"));assertTrue(archive.length()>0)
        val imported=data(NativeBridge.importProject(session,archive.absolutePath))
        assertEquals(before,imported.getJSONObject("project").toString())
        assertTrue(File(imported.getString("root"),"assets/alpha.png").exists())
        val reopened=NativeBridge.create(imported.getString("root"),"")
        assertTrue(reopened>0)
        try {assertEquals(before,data(NativeBridge.state(reopened)).getJSONObject("project").toString())}
        finally {NativeBridge.destroy(reopened)}
    }
    private fun referenceProject():String {
        val p=project()
        val assets=File(root,"assets").apply{mkdirs()}
        val bitmap=Bitmap.createBitmap(128,128,Bitmap.Config.ARGB_8888)
        val canvas=Canvas(bitmap);canvas.drawColor(Color.rgb(30,110,130))
        val paint=Paint(Paint.ANTI_ALIAS_FLAG).apply{color=Color.WHITE;textSize=34f;typeface=Typeface.DEFAULT_BOLD}
        canvas.drawText("Motion",5f,62f,paint)
        File(assets,"reference.png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
        p.put("assets",JSONArray().put(JSONObject().put("id",1).put("path","assets/reference.png").put("width",128).put("height",128)))
        val layers=JSONArray()
        fun track(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
        for(i in 0 until 20) {
            val x=100+i%5*210;val y=250+i/5*360;val z=(i%3-1)*150
            val position=track(JSONArray(listOf(x,y,z))).put("keys",JSONArray()
                .put(JSONObject().put("frame",0).put("value",JSONArray(listOf(x,y,z))).put("ease","in_out"))
                .put(JSONObject().put("frame",150).put("value",JSONArray(listOf(x+60,y-50,z))).put("ease","linear")))
            layers.put(JSONObject().put("id",i+1).put("name","reference-"+i).put("visible",true).put("locked",false)
                .put("content",JSONObject().put("kind","image").put("asset",1)).put("size",JSONArray(listOf(180,280)))
                .put("transform",JSONObject().put("position",position).put("rotation",track(JSONArray(listOf(0,0,i%4*8))))
                    .put("scale",track(JSONArray(listOf(100,100,100)))).put("opacity",track(.85))
                    .put("anchor",JSONArray(listOf(.5,.5)))))
        }
        p.put("layers",layers);return p.toString()
    }
    @Test fun systemCodecExports180FramesWithMeasuredTimestamps() {
        val json=referenceProject()
        val exporter=VideoExporter(root,json)
        val file=exporter.run{_,_->}
        assertTrue(file.length()>0)
        val report=JSONObject(File(file.parentFile,file.nameWithoutExtension+"-report.json").readText())
        assertTrue(report.getBoolean("completed"));assertEquals(180,report.getInt("encodedFrames"))
        assertEquals(0,report.getInt("applicationFrameReadbacks"))
        assertEquals(0,report.getJSONArray("cleanupErrors").length())
        val packetTimes=report.getJSONArray("timestampsUs")
        for(i in 0 until 180)assertEquals((i.toLong()*1_000_000+15)/30,packetTimes.getLong(i))
        val extractor=MediaExtractor()
        var maxTimestampError=0L
        try {
            extractor.setDataSource(file.absolutePath);assertEquals(1,extractor.trackCount)
            val format=extractor.getTrackFormat(0)
            assertEquals("video/avc",format.getString(MediaFormat.KEY_MIME))
            assertEquals(1080,format.getInteger(MediaFormat.KEY_WIDTH));assertEquals(1920,format.getInteger(MediaFormat.KEY_HEIGHT))
            extractor.selectTrack(0)
            var count=0;var previous=-1L
            while(extractor.sampleTime>=0) {
                val pts=extractor.sampleTime;assertTrue(pts>previous)
                // MP4's media timescale quantizes microsecond packet timestamps.
                val error=kotlin.math.abs((count.toLong()*1_000_000+15)/30-pts)
                maxTimestampError=maxOf(maxTimestampError,error)
                assertTrue("MP4 timestamp drift at frame "+count,error<=1)
                previous=pts;count++
                if(!extractor.advance())break
            }
            assertEquals(180,count)
            assertTrue(kotlin.math.abs(format.getLong(MediaFormat.KEY_DURATION)-6_000_000L)<=1000)
        } finally {extractor.release()}
        data(NativeBridge.replace(session,json));data(NativeBridge.seek(session,0.0))
        val png=data(NativeBridge.capture(session))
        val reference=BitmapFactory.decodeFile(png.getString("path"))
        val retriever=MediaMetadataRetriever()
        var meanError=0.0
        try {
            retriever.setDataSource(file.absolutePath)
            val decoded=retriever.getFrameAtTime(0,MediaMetadataRetriever.OPTION_CLOSEST)!!
            assertEquals(reference.width,decoded.width);assertEquals(reference.height,decoded.height)
            var error=0L;var samples=0
            for(y in 4 until decoded.height step 8)for(x in 4 until decoded.width step 8) {
                val a=reference.getPixel(x,y);val b=decoded.getPixel(x,y)
                error+=kotlin.math.abs(Color.red(a)-Color.red(b))+kotlin.math.abs(Color.green(a)-Color.green(b))+kotlin.math.abs(Color.blue(a)-Color.blue(b))
                samples+=3
            }
            meanError=error.toDouble()/samples
            File(file.parentFile,"decoded-first-frame.png").outputStream().use{decoded.compress(Bitmap.CompressFormat.PNG,100,it)}
            decoded.recycle()
            assertTrue("MP4 does not match compositor, RGB mean error="+meanError,meanError<6.0)
        } finally {retriever.release();reference.recycle()}
        File(file.parentFile,"mp4-validation.json").writeText(JSONObject().put("frames",180).put("durationUs",6_000_000)
            .put("maxTimestampErrorUs",maxTimestampError).put("firstFrameMeanAbsoluteRgbError",meanError).toString(2))
        // Preserve output and report for retrieval from this test-owned directory.
    }
    @Test fun exportCancellationDeletesPartialVideoAndRecordsTeardown() {
        val exporter=VideoExporter(root,referenceProject())
        var failed=false
        try {exporter.run{done,_->if(done>=8)exporter.cancelled.set(true)}}catch(_:IllegalStateException){failed=true}
        assertTrue(failed)
        val output=File(root,"exports")
        assertTrue(output.listFiles()?.none{it.extension=="mp4"}?:false)
        val reportFile=output.listFiles()!!.first{it.name.endsWith("-report.json")}
        val report=JSONObject(reportFile.readText());assertTrue(report.getBoolean("cancelled"));assertFalse(report.getBoolean("completed"))
    }
}
