package com.motionstudio.editor

import android.graphics.*
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import kotlin.math.*

internal object PerformanceFixture {
    private fun track(v:Any)=JSONObject().put("value",v).put("keys",JSONArray())
    private fun animation(a:Any,b:Any)=track(a).put("keys",JSONArray().put(JSONObject().put("frame",0).put("value",a).put("ease","in_out"))
        .put(JSONObject().put("frame",150).put("value",b).put("ease","linear")))
    fun create(root:File) {
        val folder=File(root,"assets").apply{mkdirs()};val assets=JSONArray();val layers=JSONArray()
        for(i in 0 until 20) {
            val text=i%5==0;val width=512;val height=if(text)192 else 384
            val bitmap=Bitmap.createBitmap(width,height,Bitmap.Config.ARGB_8888);val canvas=Canvas(bitmap);val paint=Paint(Paint.ANTI_ALIAS_FLAG)
            if(text){paint.color=Color.WHITE;paint.textSize=56f;paint.typeface=Typeface.DEFAULT_BOLD;canvas.drawText("Motion "+i,24f,115f,paint)}
            else {paint.shader=LinearGradient(0f,0f,512f,height.toFloat(),Color.rgb(40+i*7,100+i*4,150),Color.rgb(25,55+i*5,85+i*6),Shader.TileMode.CLAMP)
                canvas.drawRoundRect(RectF(12f,12f,500f,height-12f),24f,24f,paint)}
            val path="assets/layer-"+i+".png";File(root,path).outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
            assets.put(JSONObject().put("id",i+1).put("path",path).put("width",width).put("height",height))
            val content=if(text)JSONObject().put("kind","text").put("text","Motion "+i).put("font","sans-serif")
                .put("color",JSONArray(listOf(1,1,1,1))).put("raster_asset",i+1) else JSONObject().put("kind","image").put("asset",i+1)
            val x=150+(i%4)*260;val y=190+(i/4)*385;val z=(i%3)*100
            val transform=JSONObject().put("position",animation(JSONArray(listOf(x,y,z)),JSONArray(listOf(x+25,y-30,z))))
                .put("rotation",animation(JSONArray(listOf(0,0,-6)),JSONArray(listOf(0,0,6))))
                .put("scale",track(JSONArray(listOf(100,100,100)))).put("opacity",animation(.65,1.0)).put("anchor",JSONArray(listOf(.5,.5)))
            layers.put(JSONObject().put("id",i+1).put("name",if(text)"缓存文字 "+i else "图片 "+i).put("content",content)
                .put("size",JSONArray(listOf(360,if(text)135 else 270))).put("visible",true).put("locked",false).put("transform",transform))
        }
        val distance=1920/(2*tan(Math.toRadians(22.5)))
        val camera=JSONObject().put("mode","position").put("position",animation(JSONArray(listOf(540,960,-distance)),JSONArray(listOf(580,940,-distance+150))))
            .put("target",animation(JSONArray(listOf(540,960,0)),JSONArray(listOf(580,940,0)))).put("roll",track(0)).put("fov",track(45))
            .put("radius",track(distance)).put("azimuth",track(0)).put("elevation",track(0))
        val project=JSONObject().put("version",1).put("name","Motion Studio 固定性能负载").put("width",1080).put("height",1920).put("fps",30).put("frames",180)
            .put("background",JSONArray(listOf(.04,.05,.08,1))).put("camera",camera).put("assets",assets).put("layers",layers)
        val id=NativeBridge.create(root.absolutePath,project.toString());check(id!=0L){NativeBridge.creationError()}
        try{
            var maximum=0.0
            for(frame in 0 until 180) {
                val sampled=JSONObject(NativeBridge.seek(id,frame.toDouble()));check(sampled.getBoolean("ok"))
                val projected=sampled.getJSONObject("data").getJSONArray("projectedLayers");var area=0.0
                for(i in 0 until projected.length()) {
                    val corners=projected.getJSONObject(i).getJSONArray("corners")
                    val xs=(0 until 4).map{corners.getJSONArray(it).getDouble(0).coerceIn(0.0,1080.0)}
                    val ys=(0 until 4).map{corners.getJSONArray(it).getDouble(1).coerceIn(0.0,1920.0)}
                    area+=(xs.max()-xs.min())*(ys.max()-ys.min())
                }
                maximum=max(maximum,area/(1080.0*1920.0))
            }
            check(maximum<=4.0){"Reference fill exceeds four screens"}
            File(root,"workload.json").writeText(JSONObject().put("layerCount",20).put("cachedTextLayers",4).put("imageLayers",16)
                .put("framesChecked",180).put("maximumClippedBoundingBoxFillScreens",maximum).put("fillScope","Conservative clipped projected bounding rectangles, not opaque pixel coverage").toString(2))
            check(JSONObject(NativeBridge.save(id)).getBoolean("ok"))
        }finally{NativeBridge.destroy(id)}
    }
}
