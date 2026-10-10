package com.motionstudio.editor

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
import kotlin.math.pow
import kotlin.math.roundToInt

class LayerCompositingBackendTest {
    private val app get()=InstrumentationRegistry.getInstrumentation().targetContext
    private fun data(raw:String)=JSONObject(raw).let{assertTrue(raw,it.optBoolean("ok"));it.getJSONObject("data")}
    private fun array(vararg v:Any)=JSONArray(v.toList())
    private fun fixture(matte:Boolean):Pair<File,Long> {
        val root=File(app.filesDir,"acceptance/compositing-${UUID.randomUUID()}").apply{mkdirs()}
        val p=data(NativeBridge.projectTemplate(0)).put("width",64).put("height",64).put("frames",60).put("background",array(0,0,0,0))
        p.getJSONObject("camera").put("created",false)
        val template=p.getJSONArray("layers").getJSONObject(0)
        fun layer(id:Int,size:Int,rgba:JSONArray)=JSONObject(template.toString()).put("id",id).put("name","layer $id")
            .put("parent",JSONObject.NULL).put("three_d",false).put("size",array(size,size)).put("visible",true)
            .put("content",JSONObject().put("kind","solid").put("color",rgba)).also{
                it.getJSONObject("transform").getJSONObject("position").put("value",array(32,32,0)).put("keys",JSONArray())
            }
        p.put("layers",if(matte)JSONArray().put(layer(1,24,array(.5,.5,.5,.5)).put("visible",false)).put(layer(2,32,array(.9,.2,.1,.7)))
            else JSONArray().put(layer(1,64,array(.2,.2,.2,1))).put(layer(2,64,array(.7,.7,.7,.5))))
        val id=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),id>0)
        return root to id
    }
    private fun command(id:Long,op:String,fields:JSONObject)=data(NativeBridge.command(id,fields.put("op",op).toString()))
    private fun compare(id:Long,frame:Int=0):ByteArray {
        data(NativeBridge.seek(id,frame.toDouble()))
        val png=BitmapFactory.decodeFile(data(NativeBridge.capture(id)).getString("path"));assertNotNull(png)
        val project=data(NativeBridge.state(id)).getJSONObject("project")
        val info=data(NativeBridge.renderPlanInfo(id));val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
        val gpu=EglMovieRenderer(null,64,64,project,id,info)
        val pixels=ByteBuffer.allocateDirect(64*64*4)
        try {
            if(project.optJSONArray("compositions")?.length()?.let{it>0}==true) {
                var bundle=ByteBuffer.allocateDirect(info.getInt("composition_bundle_buffer_hint")).order(ByteOrder.nativeOrder())
                var bytes=CompositionBridge.sampleFrameBundleInto(id,"comp-main",frame.toDouble(),bundle)
                if(bytes< -1){bundle=ByteBuffer.allocateDirect(-bytes).order(ByteOrder.nativeOrder());bytes=CompositionBridge.sampleFrameBundleInto(id,"comp-main",frame.toDouble(),bundle)}
                assertTrue(NativeBridge.state(id),bytes>=32);gpu.prepareBundle(bundle);gpu.drawBundle(bundle)
            }else{assertTrue(NativeBridge.sampleRenderPlanInto(id,frame,plan)>0);gpu.draw(plan)}
            gpu.readPixelsInto(pixels)
        }finally{gpu.close()}
        val raw=ByteArray(pixels.capacity()).also{pixels.get(it)}
        fun decode(v:Double)=if(v<=.04045)v/12.92 else ((v+.055)/1.055).pow(2.4)
        fun encode(v:Double)=if(v<=.0031308)v*12.92 else 1.055*v.pow(1/2.4)-.055
        var sum=0L;var samples=0
        for(y in 0 until 64)for(x in 0 until 64) {
            val ref=png.getPixel(x,y);val offset=((63-y)*64+x)*4;val alpha=(raw[offset+3].toInt() and 255)/255.0
            val expected=intArrayOf(Color.red(ref),Color.green(ref),Color.blue(ref),Color.alpha(ref))
            for(c in 0..3) {
                val actual=if(c==3)(alpha*255).roundToInt()else if(alpha==0.0)0 else
                    (encode((decode((raw[offset+c].toInt() and 255)/255.0)/alpha).coerceIn(0.0,1.0))*255).roundToInt()
                sum+=abs(actual-expected[c]);samples++
            }
        }
        png.recycle();assertTrue("wgpu/GLES mean error ${sum.toDouble()/samples}",sum.toDouble()/samples<=3.0)
        return raw
    }
    @Test fun fourMatteModesMatchNativeAndRandomSeekDoesNotKeepOldCoverage() {
        val (_,id)=fixture(true)
        try {
            for((mode,expected) in listOf("alpha" to 89,"alpha_inverted" to 89,"luma" to 45,"luma_inverted" to 134)) {
                command(id,"set_track_matte",JSONObject().put("object",2).put("matte",JSONObject().put("source",1).put("mode",mode)))
                val pixels=compare(id);assertTrue(mode,abs((pixels[(32*64+32)*4+3].toInt() and 255)-expected)<=3)
            }
            command(id,"set_track_matte",JSONObject().put("object",2).put("matte",JSONObject().put("source",1).put("mode","alpha")))
            command(id,"trim_layer_clip",JSONObject().put("object",1).put("in_frame",0).put("out_frame",1))
            val first=compare(id);assertEquals(0,compare(id,3)[(32*64+32)*4+3].toInt() and 255);assertArrayEquals(first,compare(id))
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun allBlendModesAndBothSpacesMatchUnencodedNativePixels() {
        val (_,id)=fixture(false)
        try {
            for(space in listOf("linear","srgb"))for(mode in listOf("normal","add","multiply","screen","overlay","darken","lighten",
                "difference","exclusion","subtract","divide","color_dodge","color_burn","hard_light","soft_light")) {
                command(id,"set_layer_blend",JSONObject().put("object",2).put("mode",mode).put("space",space));compare(id)
            }
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun copiedMatteReferencesItsCopiedSourceAndOneUndoRestoresTheGraph() {
        val (root,id)=fixture(true)
        try {
            command(id,"set_track_matte",JSONObject().put("object",2).put("matte",JSONObject().put("source",1)))
            val before=data(NativeBridge.state(id)).getJSONObject("project")
            val paste=LayerClipboard.capture(root,before,setOf(1L,2L))!!.plan(before,0)!!
            data(NativeBridge.command(id,paste.commands.toString()))
            val after=data(NativeBridge.state(id)).getJSONObject("project")
            assertEquals(3L,after.getJSONArray("layers").objects().first{it.getLong("id")==4L}.getJSONObject("track_matte").getLong("source"))
            data(NativeBridge.history(id,0));assertEquals(before.toString(),data(NativeBridge.state(id)).getJSONObject("project").toString())
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun asymmetricProjectedMatteSurvivesNestedCompositionOrientation() {
        val (_,id)=fixture(true)
        try {
            command(id,"set_vector",JSONObject().put("object",1).put("property","position").put("frame",0).put("value",array(28,20,0)))
            command(id,"set_track_matte",JSONObject().put("object",2).put("matte",JSONObject().put("source",1)))
            val flat=compare(id)
            assertTrue((flat[((63-20)*64+32)*4+3].toInt() and 255)>80)
            assertEquals(0,flat[((63-40)*64+32)*4+3].toInt() and 255)
            command(id,"composition",JSONObject().put("action",JSONObject().put("kind","precompose")
                .put("objects",array(1,2)).put("name","nested matte").put("range","composition")))
            val nested=compare(id)
            assertTrue((nested[((63-20)*64+32)*4+3].toInt() and 255)>80)
            assertEquals(0,nested[((63-40)*64+32)*4+3].toInt() and 255)
        }finally{NativeBridge.destroy(id)}
    }
}
