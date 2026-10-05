package com.motionstudio.editor

import android.graphics.Bitmap
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
import java.util.UUID
import kotlin.math.abs

internal fun crossingFrontendFixture(alpha:Double=1.0):JSONObject {
    fun v(vararg n:Number)=JSONArray(n.toList())
    fun track(value:Any)=JSONObject().put("value",value).put("keys",JSONArray())
    fun layer(id:Int,angle:Int,color:JSONArray)=JSONObject().put("id",id).put("name","平面 $id")
        .put("three_d",true).put("size",v(256,256)).put("visible",true).put("locked",false)
        .put("content",JSONObject().put("kind","solid").put("color",color))
        .put("transform",JSONObject().put("position",track(v(128,128,0))).put("rotation",track(v(0,angle,0)))
            .put("scale",track(v(100,100,100))).put("opacity",track(alpha)).put("anchor",v(.5,.5)))
    return JSONObject(NativeBridge.projectTemplate(0)).getJSONObject("data").put("version",3)
        .put("width",256).put("height",256).put("frames",45).put("fps",30)
        .put("background",v(0,0,0,1)).put("assets",JSONArray())
        .put("layers",JSONArray().put(layer(1,45,v(1,0,0,1))).put(layer(2,-45,v(0,0,1,1))))
        .apply{getJSONObject("camera").put("created",false).remove("parent")}
}

@RunWith(AndroidJUnit4::class)
class FrontendGeometryExportTest {
    private fun data(raw:String):JSONObject {val r=JSONObject(raw);assertTrue(r.optString("error"),r.optBoolean("ok"));return r.getJSONObject("data")}
    private fun photo(bitmap:Bitmap,file:File){file.outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)}}
    private fun root()=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"acceptance/geometry-export-"+UUID.randomUUID()).apply{mkdirs()}

    @Test fun effectChainsRetainCrossingGeometryDuringPreviewAndEncodedExport() {
        val root=root();val session=NativeBridge.create(root.absolutePath,crossingFrontendFixture().put("frames",12).toString());assertTrue(session>0)
        try {
            val packages=data(NativeBridge.plugin(session,"{\"op\":\"catalogue\"}")).getJSONArray("packages").objects()
            val pkg=packages.first{it.getJSONObject("manifest").getString("version")=="1.1.0"}
            val m=pkg.getJSONObject("manifest")
            for(id in listOf(1,2))data(NativeBridge.plugin(session,JSONObject().put("op","add").put("object",id).put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",pkg.getString("hash")).put("effect","brightness_contrast").toString()))
            val capture=BitmapFactory.decodeFile(data(NativeBridge.capture(session)).getString("path"))
            photo(capture,File(root,"effects-reference.png"))
            val project=data(NativeBridge.state(session)).getJSONObject("project").toString()
            val video=VideoExporter(root,project).run{_,_->}
            val retriever=MediaMetadataRetriever()
            try {
                retriever.setDataSource(video.absolutePath)
                for(f in listOf(0,6,11)) {
                    val frame=retriever.getFrameAtIndex(f)?:error("Encoded frame missing")
                    try {
                        photo(frame,File(root,"effects-encoded-$f.png"))
                        for(x in listOf(96,160)) {
                            val a=capture.getPixel(x,128);val b=frame.getPixel(x,128)
                            for(component in listOf(Color::red,Color::green,Color::blue))assertTrue("Effect/geometry mismatch at $f x=$x: $a vs $b",abs(component(a)-component(b))<=6)
                        }
                    }finally{frame.recycle()}
                }
            }finally{retriever.release();capture.recycle()}
        }finally{NativeBridge.destroy(session)}
    }

    @Test fun opaqueAndTransparentCrossingsEncodeInDepthOrderAndFreezeAgainstLiveEdits() {
        for(alpha in listOf(1.0,.5)) {
            val root=root();val session=NativeBridge.create(root.absolutePath,crossingFrontendFixture(alpha).toString());assertTrue(session>0)
            try {
                val frozen=data(NativeBridge.state(session)).getJSONObject("project").toString()
                val png=File(data(NativeBridge.capture(session)).getString("path"));png.copyTo(File(root,"reference.png"),true)
                val video=VideoExporter(root,frozen).run{done,_->if(done==6)data(NativeBridge.command(session,
                    JSONObject().put("op","set_vector").put("object",1).put("property","position").put("frame",0)
                        .put("value",JSONArray(listOf(80,128,0))).toString()))}
                val report=JSONObject(File(video.parentFile,video.nameWithoutExtension+"-report.json").readText())
                assertTrue(report.getBoolean("completed"));assertTrue(report.getBoolean("geometrySampling"))
                assertEquals(45,report.getInt("encodedFrames"));assertTrue(report.getLong("vertexTransferBytes")>0)
                assertEquals(0,report.getInt("applicationFrameReadbacks"));assertEquals(0,report.getInt("applicationFrameUploads"))
                val retriever=MediaMetadataRetriever();val evidence=JSONArray()
                try {
                    retriever.setDataSource(video.absolutePath)
                    for(f in listOf(0,15,44)) {
                        val bitmap=retriever.getFrameAtIndex(f)?:error("Frame $f did not decode")
                        try {
                            val left=bitmap.getPixel(96,128);val right=bitmap.getPixel(160,128)
                            assertTrue("Left crossing at $f",Color.blue(left)>Color.red(left)+30)
                            assertTrue("Right crossing at $f",Color.red(right)>Color.blue(right)+30)
                            photo(bitmap,File(root,"encoded-$f.png"))
                            evidence.put(JSONObject().put("frame",f).put("left",left).put("right",right))
                        }finally{bitmap.recycle()}
                    }
                }finally{retriever.release()}
                File(root,"crossing-report.json").writeText(JSONObject().put("opacity",alpha).put("frozenAgainstLiveEdits",true).put("probes",evidence).toString(2))
            }finally{NativeBridge.destroy(session)}
        }
    }

    @Test fun splitTrianglesKeepTextureCoordinatesAndPremultipliedTransparencyInEncodedVideo() {
        val root=root();val assets=File(root,"assets").apply{mkdirs()}
        val image=Bitmap.createBitmap(256,256,Bitmap.Config.ARGB_8888)
        for(y in 0..255)for(x in 0..255)image.setPixel(x,y,Color.argb(160,60+x/2,60+y/2,100))
        photo(image,File(assets,"gradient.png"));image.recycle()
        val project=crossingFrontendFixture(.8)
        project.getJSONArray("layers").getJSONObject(0).put("content",JSONObject().put("kind","image").put("asset",1))
        project.getJSONArray("layers").getJSONObject(1).getJSONObject("content").put("color",JSONArray(listOf(.25,.25,.25,1)))
        project.put("assets",JSONArray().put(JSONObject().put("id",1).put("path","assets/gradient.png").put("width",256).put("height",256)))
        val session=NativeBridge.create(root.absolutePath,project.toString());assertTrue(session>0)
        try {
            val frozen=data(NativeBridge.state(session)).getJSONObject("project").toString()
            val reference=BitmapFactory.decodeFile(data(NativeBridge.capture(session)).getString("path"));photo(reference,File(root,"reference.png"))
            val video=VideoExporter(root,frozen).run{_,_->}
            val retriever=MediaMetadataRetriever()
            try {
                retriever.setDataSource(video.absolutePath)
                val decoded=retriever.getFrameAtIndex(21)?:error("Texture frame did not decode")
                try {
                    var error=0L;var count=0
                    for(y in 80..176 step 8)for(x in 80..176 step 8)if(abs(x-128)>8) {
                        val a=reference.getPixel(x,y);val b=decoded.getPixel(x,y)
                        error+=abs(Color.red(a)-Color.red(b))+abs(Color.green(a)-Color.green(b))+abs(Color.blue(a)-Color.blue(b));count+=3
                    }
                    val mean=error.toDouble()/count
                    assertTrue("Encoded UV/transparency mean RGB error $mean",mean<15)
                    photo(decoded,File(root,"encoded-texture.png"));File(root,"texture-report.json").writeText(JSONObject().put("meanRgbError",mean).put("samples",count/3).toString(2))
                }finally{decoded.recycle()}
            }finally{retriever.release();reference.recycle()}
        }finally{NativeBridge.destroy(session)}
    }
}
