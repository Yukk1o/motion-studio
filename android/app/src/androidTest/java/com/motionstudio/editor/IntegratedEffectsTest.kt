package com.motionstudio.editor

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Color
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID
import kotlin.math.abs

/** Exercises interactions between the merged capabilities with an ordinary phone project. */
class IntegratedEffectsTest {
    private fun command(id:Long,value:JSONObject)=nativeData(NativeBridge.command(id,value.toString()))
    private fun effect(id:Long,layer:Long,action:JSONObject)=command(id,JSONObject().put("op","effect").put("object",layer).put("action",action))
    private fun add(id:Long,layer:Long,name:String):Long {
        val pkg=nativeData(NativeBridge.plugin(id,"{\"op\":\"catalogue\"}")).getJSONArray("packages").objects()
            .first{it.getJSONObject("manifest").getJSONArray("effects").objects().any{e->e.getString("id")==name}}
        val manifest=pkg.getJSONObject("manifest")
        nativeData(NativeBridge.plugin(id,JSONObject().put("op","add").put("object",layer).put("effect",name)
            .put("plugin",manifest.getString("id")).put("version",manifest.getString("version")).put("hash",pkg.getString("hash")).toString()))
        return nativeData(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").objects().first{it.getLong("id")==layer}
            .getJSONArray("effects").objects().last().getLong("id")
    }
    private fun set(id:Long,instance:Long,param:String,value:List<Number>)=effect(id,2,JSONObject().put("kind","set").put("effect",instance)
        .put("param",param).put("frame",0).put("value",JSONArray(value)))

    @Test fun maskedColorCurvesAndSpriteOnlyParticlesSurviveSaveAndFrozenExport() {
        val context=InstrumentationRegistry.getInstrumentation().targetContext
        val root=File(context.filesDir,"acceptance/integrated-${UUID.randomUUID()}").apply{mkdirs()}
        val source=File(root,"assets/sprite.png").apply{parentFile!!.mkdirs()}
        Bitmap.createBitmap(16,8,Bitmap.Config.ARGB_8888).also{bitmap->
            for(y in 0 until 8)for(x in 0 until 16)bitmap.setPixel(x,y,if(x<8)Color.rgb(255,70,20)else Color.rgb(20,100,255))
            source.outputStream().use{bitmap.compress(Bitmap.CompressFormat.PNG,100,it)};bitmap.recycle()
        }
        val project=nativeData(NativeBridge.projectTemplate(0)).put("width",1080).put("height",1920).put("frames",60)
            .put("background",JSONArray(listOf(0,0,0,1))).put("assets",JSONArray().put(JSONObject().put("id",7).put("path","assets/sprite.png").put("width",16).put("height",8)))
        project.getJSONObject("camera").put("created",false)
        val original=project.getJSONArray("layers").getJSONObject(0)
        fun layer(id:Int,position:List<Int>,color:List<Int>)=JSONObject(original.toString()).put("id",id).put("size",JSONArray(listOf(1080,1920)))
            .put("parent",JSONObject.NULL).put("three_d",false).put("content",JSONObject().put("kind","solid").put("color",JSONArray(color))).also{
                it.getJSONObject("transform").getJSONObject("position").put("value",JSONArray(position)).put("keys",JSONArray())
            }
        project.put("layers",JSONArray().put(layer(1,listOf(540,960,0),listOf(1,0,0,1))).put(layer(2,listOf(800,1700,0),listOf(0,0,0,0))))
        var native=NativeBridge.create(root.absolutePath,project.toString());assertTrue(NativeBridge.creationError(),native>0)
        var frozen=0L
        try {
            command(native,JSONObject().put("op","set_color").put("object",1).put("value",JSONArray(listOf(.25,.5,.75,1))))
            val nodes=listOf(200 to 300,700 to 300,700 to 1200,200 to 1200).mapIndexed{i,p->JSONObject().put("id",i+1)
                .put("geometry",JSONObject().put("value",JSONArray(listOf(p.first,p.second,0,0,0,0))).put("keys",JSONArray()))}
            val mask=JSONObject().put("id",1).put("name","Integrated mask").put("opacity",JSONObject().put("value",50).put("keys",JSONArray()))
                .put("path",JSONObject().put("id",1).put("closed",true).put("nodes",JSONArray(nodes)))
            command(native,JSONObject().put("op","mask").put("object",1).put("action",JSONObject().put("kind","add").put("mask",mask)))
            val curves=add(native,1,"curves")
            val stored=nativeData(NativeBridge.state(native)).getJSONObject("project").getJSONArray("layers").getJSONObject(0)
                .getJSONArray("effects").getJSONObject(0).getJSONObject("params")
            val param=stored.keys().asSequence().first{stored.getJSONObject(it).getString("kind")=="curve"}
            fun identity()=JSONArray().put(JSONArray(listOf(0,0))).put(JSONArray(listOf(1,1)))
            val value=JSONObject().put("channels",JSONArray(List(5){identity()})).put("interpolation",JSONArray(List(5){"natural_cubic"}))
            value.getJSONArray("channels").put(4,JSONArray().put(JSONArray(listOf(0,0))).put(JSONArray(listOf(1,.5))))
            effect(native,1,JSONObject().put("kind","set_curve_object").put("effect",curves).put("param",param).put("frame",0).put("value",value))
            val particle=add(native,2,"particle_emitter")
            effect(native,2,JSONObject().put("kind","set_scene").put("effect",particle).put("scene",JSONObject()
                .put("particle_space","world_birth").put("sprite_asset",7).put("occlusion",false).put("elements",JSONArray())))
            for((name,v) in listOf("rate" to 1,"speed" to 0,"spread" to 0,"size" to 100,"end_size" to 100,"fade" to 0,"prewarm" to 0))
                set(native,particle,name,listOf(v,0,0,0))
            set(native,particle,"color",listOf(1,1,1,1));set(native,particle,"end_color",listOf(1,1,1,1))
            nativeData(NativeBridge.save(native));NativeBridge.destroy(native);native=0
            native=NativeBridge.create(root.absolutePath,"");assertTrue(NativeBridge.creationError(),native>0)
            val reopened=nativeData(NativeBridge.state(native)).getJSONObject("project")
            assertEquals(8,reopened.getInt("version"));assertEquals(1,reopened.getJSONArray("layers").getJSONObject(0).getJSONArray("masks").length())
            val frozenJson=reopened.toString()
            frozen=NativeBridge.create(root.absolutePath,frozenJson);assertTrue(frozen>0)
            command(native,JSONObject().put("op","set_color").put("object",1).put("value",JSONArray(listOf(1,0,0,1))))
            assertEquals(frozenJson,nativeData(NativeBridge.state(frozen)).getJSONObject("project").toString())
            val reference=BitmapFactory.decodeFile(nativeData(NativeBridge.capture(frozen)).getString("path"))
            try {
                assertEquals(Color.BLACK,reference.getPixel(100,900))
                val inside=reference.getPixel(500,900);assertTrue(Color.blue(inside)>Color.red(inside)&&Color.red(inside)>0)
                assertTrue("PNG sprite must be visible without an image layer",(750..850 step 5).any{x->(1670..1730 step 5).any{y->Color.red(reference.getPixel(x,y))>80||Color.blue(reference.getPixel(x,y))>80}})
                val info=nativeData(NativeBridge.renderPlanInfo(frozen))
                val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
                assertTrue(NativeBridge.sampleRenderPlanInto(frozen,0,plan)>0)
                val gpu=EglMovieRenderer(null,1080,1920,reopened,frozen,info)
                try {
                    gpu.draw(plan);assertEquals("The sprite is the only demanded image",1L,gpu.imageUploads)
                    val pixels=ByteBuffer.allocateDirect(1080*1920*4);gpu.readPixelsInto(pixels)
                    val points=mutableListOf(100 to 900,500 to 900,200 to 600,699 to 600,900 to 100)
                    for(y in 1650..1750 step 10)for(x in 740..860 step 10)points.add(x to y)
                    var rgb=0.0
                    for((x,y) in points) {
                        val argb=reference.getPixel(x,y);val at=((1919-y)*1080+x)*4
                        for((c,expected) in listOf(Color.red(argb),Color.green(argb),Color.blue(argb)).withIndex())rgb+=abs(expected-(pixels.get(at+c).toInt() and 255))
                        assertTrue("Alpha ($x,$y)",abs(Color.alpha(argb)-(pixels.get(at+3).toInt() and 255))<=3)
                    }
                    val error=rgb/(points.size*3)
                    File(root,"integration-report.json").writeText(JSONObject().put("rgbMae",error).put("points",points.size).put("imageUploads",gpu.imageUploads)
                        .put("project","1080x1920 @30fps").put("savedReopened",true).put("frozen",true).toString(2))
                    assertTrue("Integrated wgpu/GLES RGB MAE=$error",error<=3)
                    gpu.draw(plan);assertEquals("A second frame reuses the sprite texture",1L,gpu.imageUploads)
                } finally {gpu.close()}
            } finally {reference.recycle()}
        } finally {if(frozen>0)NativeBridge.destroy(frozen);if(native>0)NativeBridge.destroy(native)}
    }
}
