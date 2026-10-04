package com.motionstudio.editor

import android.graphics.*
import android.media.MediaExtractor
import android.media.MediaMetadataRetriever
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Test
import org.junit.Assert.*
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import kotlin.math.*

@RunWith(AndroidJUnit4::class)
class ReferenceFilmTest {
    private fun data(raw:String):JSONObject {val r=JSONObject(raw);assertTrue(r.optString("error"),r.optBoolean("ok"));return r.getJSONObject("data")}
    private fun track(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
    private fun animated(start:Any,end:Any,last:Int)=track(start).put("keys",JSONArray()
        .put(JSONObject().put("frame",0).put("value",start).put("ease","in_out"))
        .put(JSONObject().put("frame",last).put("value",end).put("ease","linear")))
    @Test fun specifiedSixSecondReferenceFilmExportsAndHoldsItsLastSecond() {
        val app=InstrumentationRegistry.getInstrumentation().targetContext
        val root=File(app.filesDir,"acceptance/reference-film-"+UUID.randomUUID()).apply{mkdirs()}
        val assets=File(root,"assets").apply{mkdirs()}
        val specs=listOf(256 to 384,256 to 320,128 to 128,800 to 160)
        for(i in specs.indices) {
            val (w,h)=specs[i];val bitmap=Bitmap.createBitmap(w,h,Bitmap.Config.ARGB_8888);val canvas=Canvas(bitmap)
            val paint=Paint(Paint.ANTI_ALIAS_FLAG)
            when(i) {
                0->{paint.shader=LinearGradient(0f,0f,w.toFloat(),h.toFloat(),Color.rgb(10,24,42),Color.rgb(24,62,78),Shader.TileMode.CLAMP);canvas.drawRect(0f,0f,w.toFloat(),h.toFloat(),paint)}
                1->{paint.color=Color.rgb(54,204,176);canvas.drawRoundRect(RectF(12f,12f,244f,308f),24f,24f,paint)
                    paint.color=Color.WHITE;paint.textSize=32f;paint.typeface=Typeface.DEFAULT_BOLD;canvas.drawText("MOTION",42f,142f,paint)
                    paint.strokeWidth=6f;canvas.drawLine(44f,170f,210f,170f,paint)}
                2->{paint.color=Color.rgb(230,193,126);canvas.drawCircle(64f,64f,52f,paint);paint.color=Color.rgb(20,46,57);canvas.drawCircle(64f,64f,24f,paint)}
                3->{paint.color=Color.WHITE;paint.typeface=Typeface.DEFAULT_BOLD;paint.textSize=106f;canvas.drawText("Motion Studio",20f,118f,paint)}
            }
            File(assets,"asset-"+i+".png").outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
        }
        val distance=1920/(2*tan(Math.toRadians(22.5)))
        val camera=JSONObject().put("mode","position")
            .put("position",animated(JSONArray(listOf(540,960,-distance)),JSONArray(listOf(580,940,-distance+350)),150))
            .put("target",animated(JSONArray(listOf(540,960,0)),JSONArray(listOf(580,940,0)),150))
            .put("roll",track(0)).put("fov",track(45)).put("radius",track(distance)).put("azimuth",track(0)).put("elevation",track(0))
        val layerAssets=JSONArray();specs.forEachIndexed{i,(w,h)->layerAssets.put(JSONObject().put("id",i+1).put("path","assets/asset-"+i+".png").put("width",w).put("height",h))}
        val layers=JSONArray()
        val positions=listOf(listOf(540,960,650),listOf(580,1040,0),listOf(280,1320,-500),listOf(540,600,-150))
        val sizes=listOf(listOf(1500,2300),listOf(480,600),listOf(160,160),listOf(820,164))
        for(i in 0..3) {
            val content=if(i==3)JSONObject().put("kind","text").put("text","Motion Studio").put("font","sans-bold")
                .put("color",JSONArray(listOf(1,1,1,1))).put("raster_asset",4) else JSONObject().put("kind","image").put("asset",i+1)
            val transform=JSONObject().put("position",if(i==3)animated(JSONArray(positions[i]),JSONArray(listOf(540,460,-150)),60)else track(JSONArray(positions[i])))
                .put("rotation",track(JSONArray(listOf(0,0,0)))).put("scale",track(JSONArray(listOf(100,100,100))))
                .put("opacity",if(i==3)animated(0,1,60)else track(1)).put("anchor",JSONArray(listOf(.5,.5)))
            layers.put(JSONObject().put("id",i+1).put("name",listOf("背景图","主体卡片","前景装饰","标题")[i]).put("content",content).put("size",JSONArray(sizes[i]))
                .put("visible",true).put("locked",false).put("transform",transform))
        }
        val p=JSONObject().put("version",1).put("name","Motion Studio 首个参考短片").put("width",1080).put("height",1920)
            .put("fps",30).put("frames",180).put("background",JSONArray(listOf(.04,.06,.09,1)))
            .put("camera",camera).put("assets",layerAssets).put("layers",layers)
        val session=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),session>0)
        try {
            data(NativeBridge.save(session))
            for(frame in listOf(0,30,60,150,179)) {
                val state=data(NativeBridge.seek(session,frame.toDouble()))
                val title=state.getJSONArray("sampledLayers").getJSONObject(3)
                if(frame==0)assertEquals(0.0,title.getDouble("opacity"),.001)
                if(frame==30)assertEquals(.5,title.getDouble("opacity"),.001)
                if(frame>=60){assertEquals(1.0,title.getDouble("opacity"),.001);assertEquals(460.0,title.getJSONArray("position").getDouble(1),.001)}
                val capture=data(NativeBridge.capture(session))
                File(capture.getString("path")).copyTo(File(root,"reference-frame-"+frame+".png"),true)
            }
            val at150=BitmapFactory.decodeFile(File(root,"reference-frame-150.png").absolutePath)
            val at179=BitmapFactory.decodeFile(File(root,"reference-frame-179.png").absolutePath)
            assertTrue(at150.sameAs(at179));at150.recycle();at179.recycle()
            val file=VideoExporter(root,p.toString()).run{_,_->}
            file.copyTo(File(root,"MotionStudio-reference.mp4"),true)
            val extractor=MediaExtractor()
            try {extractor.setDataSource(file.absolutePath);assertEquals(1,extractor.trackCount);extractor.selectTrack(0)
                var count=0;while(extractor.sampleTime>=0){assertTrue(abs(extractor.sampleTime-(count.toLong()*1_000_000+15)/30)<=1);count++;if(!extractor.advance())break};assertEquals(180,count)
            }finally{extractor.release()}
            val retriever=MediaMetadataRetriever()
            try {retriever.setDataSource(file.absolutePath);assertEquals("6000",retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_DURATION))
                val decoded=retriever.getFrameAtTime(5_000_000,MediaMetadataRetriever.OPTION_CLOSEST)!!
                File(root,"reference-decoded-frame150.png").outputStream().use{decoded.compress(Bitmap.CompressFormat.PNG,100,it)};decoded.recycle()
            }finally{retriever.release()}
            File(root,"reference-film-validation.json").writeText(JSONObject().put("frames",180).put("durationMs",6000).put("titleFadeAndMoveFrames",JSONArray(listOf(0,60)))
                .put("cameraDollyAndPanFrames",JSONArray(listOf(0,150))).put("holdFrames",JSONArray(listOf(150,179))).put("lastSecondPixelsIdentical",true).toString(2))
            data(NativeBridge.pack(session))
        }finally{NativeBridge.destroy(session)}
    }
}
