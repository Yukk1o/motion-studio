package com.motionstudio.editor

import android.graphics.*
import android.media.*
import android.os.Debug
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
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlin.math.*

/** Actual H.264 frames, not a comparison of two copies of the same draw buffer. */
@RunWith(AndroidJUnit4::class)
class ExportParityTest {
    private fun data(raw:String):JSONObject {val r=JSONObject(raw);assertTrue(r.optString("error"),r.optBoolean("ok"));return r.getJSONObject("data")}
    private fun track(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
    private fun curve(index:Int,velocity:Boolean=false):JSONObject {
        val kind=when(index%3){0->"quadratic";1->"cubic";else->"elastic"}
        val shape=JSONObject().put("kind",kind)
        when(kind) {
            "quadratic"->shape.put("control",JSONArray(listOf(.35,if(velocity)2.0 else .8)))
            "cubic"->shape.put("control1",JSONArray(listOf(.2,if(velocity)1.5 else -.2)))
                .put("control2",JSONArray(listOf(.75,if(velocity)2.0 else 1.2)))
            "elastic"->shape.put("oscillations",2.5).put("damping",6.0)
        }
        if(kind!="elastic")shape.put("start",0.0).put("end",if(velocity)0.0 else 1.0)
        return JSONObject().put("space",if(velocity)"velocity"else"progress").put("shape",shape)
    }
    private fun animated(a:Any,b:Any,end:Int,definition:JSONObject)=track(a).put("keys",JSONArray()
        .put(JSONObject().put("frame",0).put("value",a).put("ease","linear").put("curve",definition))
        .put(JSONObject().put("frame",end).put("value",b).put("ease","linear")))
    private fun layer(id:Int,kind:String,position:List<Number>):JSONObject {
        val content=when(kind) {
            "image"->JSONObject().put("kind",kind).put("asset",1)
            "text"->JSONObject().put("kind",kind).put("text","Motion Studio").put("font","sans-bold")
                .put("color",JSONArray(listOf(1,1,1,1))).put("raster_asset",2)
            "solid"->JSONObject().put("kind",kind).put("color",JSONArray(listOf(.9,.3,.16,.8)))
            else->JSONObject().put("kind","null")
        }
        return JSONObject().put("id",id).put("name","export-"+id).put("visible",true).put("locked",false)
            .put("content",content).put("size",JSONArray(if(kind=="text")listOf(190,44)else listOf(180,280)))
            .put("transform",JSONObject().put("position",track(JSONArray(position))).put("rotation",track(JSONArray(listOf(0,0,0))))
                .put("scale",track(JSONArray(listOf(100,100,100)))).put("opacity",track(1)).put("anchor",JSONArray(listOf(.5,.5))))
    }
    private fun fixture(root:File):Long {
        val assets=File(root,"assets").apply{mkdirs()}
        val image=Bitmap.createBitmap(256,256,Bitmap.Config.ARGB_8888)
        val pixels=IntArray(256*256){i->val x=i%256;val y=i/256;val edge=minOf(x,y,255-x,255-y)
            Color.argb((min(1.0,edge/24.0)*180).toInt(),30+x/2,220-y/3,190)}
        image.setPixels(pixels,0,256,0,0,256,256)
        File(assets,"transparent.png").outputStream().use{image.compress(Bitmap.CompressFormat.PNG,100,it)};image.recycle()
        val text=Bitmap.createBitmap(640,144,Bitmap.Config.ARGB_8888)
        Canvas(text).drawText("Motion Studio",12f,104f,Paint(Paint.ANTI_ALIAS_FLAG).apply{color=Color.WHITE;textSize=82f;typeface=Typeface.DEFAULT_BOLD})
        File(assets,"text.png").outputStream().use{text.compress(Bitmap.CompressFormat.PNG,100,it)};text.recycle()
        val p=data(NativeBridge.projectTemplate(0)).put("name","A13 曲线与父子级编码").put("background",JSONArray(listOf(.04,.06,.09,1)))
        p.put("assets",JSONArray().put(JSONObject().put("id",1).put("path","assets/transparent.png").put("width",256).put("height",256))
            .put(JSONObject().put("id",2).put("path","assets/text.png").put("width",640).put("height",144)))
        val layers=JSONArray()
        for(i in 0 until 20) {
            val kind=if(i==19)"solid"else if(i%5==0)"text"else"image"
            val x=105+i%5*212;val y=245+i/5*420;val z=(i%3-1)*120
            val l=layer(i+1,kind,listOf(x,y,z));val t=l.getJSONObject("transform")
            t.put("position",animated(JSONArray(listOf(x,y,z)),JSONArray(listOf(x+70,y-55,z)),150,curve(i)))
                .put("rotation",animated(JSONArray(listOf(0,0,i%4*7)),JSONArray(listOf(4,-5,i%4*7+12)),150,curve(1)))
                .put("scale",animated(JSONArray(listOf(90,90,100)),JSONArray(listOf(105,105,100)),150,curve(0)))
                .put("opacity",animated(.2,.95,120,curve(i,true)))
                .put("anchor",JSONArray(if(i%2==0)listOf(.25,.75)else listOf(.5,.5)))
            layers.put(l)
        }
        for(id in 21..23)layers.put(layer(id,"null",listOf(540,960,0)))
        p.put("layers",layers)
        val session=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),session>0)
        try {
            for((child,parent) in listOf(21 to 23,22 to 23,0 to 21,2 to 22,6 to 2,20 to 22))data(NativeBridge.command(session,
                JSONObject().put("op","parent").put("object",child).put("parent",parent).put("frame",0).toString()))
            val rig=data(NativeBridge.state(session)).getJSONObject("project")
            val rigLayers=rig.getJSONArray("layers")
            for(id in 21..23) {
                val transform=rigLayers.getJSONObject(id-1).getJSONObject("transform")
                transform.put("position",animated(JSONArray(listOf(540,960,0)),JSONArray(listOf(570+id%3*20,945,-30-id%3*25)),150,curve(id,id==22)))
                transform.put("rotation",animated(JSONArray(listOf(0,0,0)),JSONArray(listOf(0,0,if(id==21)2 else -1)),150,curve(1)))
            }
            data(NativeBridge.replace(session,rig.toString()));data(NativeBridge.save(session))
            return session
        }catch(error:Throwable){NativeBridge.destroy(session);throw error}
    }
    private fun pss():Int=Debug.MemoryInfo().let{Debug.getMemoryInfo(it);it.totalPss}
    private fun root(name:String)=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"acceptance/"+name+"-"+UUID.randomUUID()).apply{mkdirs()}
    private fun photo(bitmap:Bitmap,path:File){path.outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)}}
    private fun compare(reference:Bitmap,decoded:Bitmap):JSONObject {
        assertEquals(reference.width,decoded.width);assertEquals(reference.height,decoded.height)
        val a=IntArray(reference.width*reference.height);val b=IntArray(a.size)
        reference.getPixels(a,0,reference.width,0,0,reference.width,reference.height);decoded.getPixels(b,0,decoded.width,0,0,decoded.width,decoded.height)
        var total=0L;var samples=0;var foreground=0L;var active=0;val errors=ArrayList<Int>()
        for(y in 2 until reference.height step 4)for(x in 2 until reference.width step 4) {
            val pixel=a[y*reference.width+x];val other=b[y*reference.width+x]
            val error=abs(Color.red(pixel)-Color.red(other))+abs(Color.green(pixel)-Color.green(other))+abs(Color.blue(pixel)-Color.blue(other))
            total+=error;samples+=3
            if(abs(Color.red(pixel)-10)+abs(Color.green(pixel)-15)+abs(Color.blue(pixel)-23)>36){foreground+=error;active+=3;errors.add(error)}
        }
        errors.sort()
        return JSONObject().put("meanRgbError",total.toDouble()/samples).put("foregroundMeanRgbError",foreground.toDouble()/active.coerceAtLeast(1))
            .put("foregroundP95MeanRgbError",if(errors.isEmpty())0.0 else errors[(errors.size*.95).toInt().coerceAtMost(errors.lastIndex)]/3.0).put("activeSamples",active/3)
    }
    @Test fun parentedCustomCurvesExport180FramesAndIndexedFramesMatchWgpu() {
        val root=root("export-parity");val session=fixture(root)
        try {
            val original=data(NativeBridge.state(session)).getJSONObject("project").toString()
            val selected=listOf(0,15,21,30,60,90,150,179)
            for(frame in selected) {
                data(NativeBridge.seek(session,frame.toDouble()));val capture=data(NativeBridge.capture(session))
                File(capture.getString("path")).copyTo(File(root,"wgpu-"+frame+".png"),true)
            }
            val memory=JSONArray().put(JSONObject().put("stage","before").put("pssKiB",pss()))
            var edited=false
            val file=VideoExporter(root,original).run{done,_->
                if(done%15==0)memory.put(JSONObject().put("encodedFrame",done).put("pssKiB",pss()))
                if(done==8){data(NativeBridge.command(session,JSONObject().put("op","set_scalar").put("object",0).put("property","fov").put("frame",0).put("value",70).toString()));edited=true}
            }
            assertTrue(edited);assertNotEquals(original,data(NativeBridge.state(session)).getJSONObject("project").toString())
            val codecReport=JSONObject(File(file.parentFile,file.nameWithoutExtension+"-report.json").readText())
            assertTrue(codecReport.getBoolean("completed"));assertEquals(180,codecReport.getInt("encodedFrames"));assertEquals(0,codecReport.getJSONArray("cleanupErrors").length())
            assertTrue("1080p30 export throughput below target: $codecReport",codecReport.getDouble("throughputFps")>=30.0)
            assertEquals(0,codecReport.getInt("applicationFrameReadbacks"));assertEquals(0,codecReport.getInt("applicationFrameUploads"))
            val extractor=MediaExtractor();var maxPtsError=0L
            try {
                extractor.setDataSource(file.absolutePath);assertEquals(1,extractor.trackCount);extractor.selectTrack(0)
                var count=0;var last=-1L
                while(extractor.sampleTime>=0) {
                    val pts=extractor.sampleTime;assertTrue(pts>last);maxPtsError=max(maxPtsError,abs(pts-(count.toLong()*1_000_000+15)/30));assertTrue(maxPtsError<=1)
                    last=pts;count++;if(!extractor.advance())break
                }
                assertEquals(180,count)
            }finally{extractor.release()}
            val retriever=MediaMetadataRetriever();val comparisons=JSONArray()
            try {
                retriever.setDataSource(file.absolutePath)
                assertEquals("180",retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_FRAME_COUNT))
                assertEquals("6000",retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION))
                for(frame in selected) {
                    val reference=BitmapFactory.decodeFile(File(root,"wgpu-"+frame+".png").absolutePath)
                    val decoded=retriever.getFrameAtIndex(frame)?:error("Indexed frame could not be decoded")
                    try {
                        photo(decoded,File(root,"decoded-"+frame+".png"))
                        val comparison=compare(reference,decoded).put("frame",frame);comparisons.put(comparison)
                        File(root,"frame-comparisons.json").writeText(comparisons.toString(2))
                        assertTrue("Frame $frame: $comparison",comparison.getDouble("meanRgbError")<6.0&&comparison.getDouble("foregroundMeanRgbError")<8.0)
                    }finally{reference.recycle();decoded.recycle()}
                }
            }finally{retriever.release()}
            memory.put(JSONObject().put("stage","afterDecode").put("pssKiB",pss()))
            file.copyTo(File(root,"MotionStudio-parented-curves.mp4"),true)
            File(root,"export-parity-report.json").writeText(JSONObject().put("codec",codecReport).put("indexedFrames",comparisons)
                .put("maxTimestampErrorUs",maxPtsError).put("memorySamples",memory).put("frozenAgainstLiveEdits",true)
                .put("drawableLayers",20).put("nullObjects",3).put("multilevelParents",true)
                .put("scope","MuMu; memory includes test process and PNG/decoded bitmap checks, not phone performance acceptance").toString(2))
        }finally{NativeBridge.destroy(session)}
    }

    @Test fun repeatedCancellationReleasesResourcesAndAllowsTheNextFullExport() {
        val root=root("export-cancel");val session=fixture(root)
        try {
            val original=data(NativeBridge.state(session)).getJSONObject("project").toString()
            val reports=JSONArray()
            for(limit in listOf(8,45,120)) {
                val task=VideoExporter(root,original);var failed=false
                try{task.run{done,_->if(done>=limit)task.cancelled.set(true)}}catch(_:IllegalStateException){failed=true}
                assertTrue(failed)
                assertFalse(File(root,"exports").listFiles()!!.any{it.extension=="mp4"})
                val latest=File(root,"exports").listFiles()!!.filter{it.name.endsWith("-report.json")}.maxBy{it.lastModified()}
                val report=JSONObject(latest.readText());assertTrue(report.getBoolean("cancelled"));assertFalse(report.getBoolean("completed"));assertEquals(0,report.getJSONArray("cleanupErrors").length())
                reports.put(JSONObject().put("cancelAfter",limit).put("pssKiBAfter",pss()).put("codecReport",report))
            }
            val finished=VideoExporter(root,original).run{_,_->};assertTrue(finished.isFile)
            File(root,"repeated-cancellation.json").writeText(JSONObject().put("attempts",reports).put("nextFullExportCompleted",true).put("pssKiBAfterComplete",pss()).toString(2))
        }finally{NativeBridge.destroy(session)}
    }

    @Test fun exportedMp4PlaysAtItsDeclaredDurationOnAnActualDecoderSurface() {
        val root=root("export-playback");val session=fixture(root)
        try {
            val file=VideoExporter(root,data(NativeBridge.state(session)).getJSONObject("project").toString()).run{_,_->}
            val reader=ImageReader.newInstance(1080,1920,ImageFormat.PRIVATE,4)
            val consumer=HandlerThread("acceptance-video-consumer").apply{start()}
            val frames=AtomicInteger();val done=CountDownLatch(1);var error:String?=null
            reader.setOnImageAvailableListener({source->source.acquireLatestImage()?.use{frames.incrementAndGet()}},Handler(consumer.looper))
            val player=MediaPlayer();val started=System.nanoTime()
            try {
                player.setSurface(reader.surface);player.setDataSource(file.absolutePath)
                player.setOnCompletionListener{done.countDown()};player.setOnErrorListener{_,what,extra->error="$what/$extra";done.countDown();true}
                player.prepare();assertEquals(6000,player.duration);assertEquals(1080,player.videoWidth);assertEquals(1920,player.videoHeight)
                player.start();assertTrue("Playback did not complete",done.await(15,TimeUnit.SECONDS));assertNull(error)
                val elapsed=(System.nanoTime()-started)/1e9
                assertTrue("Not enough decoded surface frames: "+frames.get(),frames.get()>=120)
                assertTrue("Playback clock was not respected: $elapsed",elapsed>=5.5&&elapsed<12.0)
                File(root,"surface-playback.json").writeText(JSONObject().put("durationMs",6000).put("elapsedSeconds",elapsed)
                    .put("surfaceFramesConsumed",frames.get()).put("completionCallback",true).put("decoderError",JSONObject.NULL).toString(2))
            }finally{player.release();reader.setOnImageAvailableListener(null,null);consumer.quitSafely();consumer.join(3000);reader.close()}
        }finally{NativeBridge.destroy(session)}
    }
}
