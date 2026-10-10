package com.motionstudio.editor

import androidx.test.platform.app.InstrumentationRegistry
import android.graphics.ImageFormat
import android.hardware.HardwareBuffer
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

/** Backend/renderer acceptance only; no editor UI or file-picker automation. */
class OfficialMotionBackendTest {
    private val app get()=InstrumentationRegistry.getInstrumentation().targetContext
    private fun data(raw:String):JSONObject {val v=JSONObject(raw);assertTrue(raw,v.getBoolean("ok"));return v.getJSONObject("data")}
    private fun array(vararg values:Any)=JSONArray(values.toList())
    private fun fixture():Pair<File,Long> {
        val root=File(app.filesDir,"official-motion-${UUID.randomUUID()}").apply{mkdirs()}
        val p=data(NativeBridge.projectTemplate(0)).put("width",64).put("height",64).put("frames",60).put("background",array(0,0,0,0))
        p.getJSONObject("camera").put("created",false)
        val layer=p.getJSONArray("layers").getJSONObject(0).put("id",1).put("three_d",false).put("parent",JSONObject.NULL).put("size",array(64,64))
        layer.put("content",JSONObject().put("kind","solid").put("color",array(.2,.4,.7,1)))
        layer.getJSONObject("transform").getJSONObject("position").put("value",array(32,32,0)).put("keys",JSONArray())
        p.put("layers",JSONArray().put(layer))
        val id=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),id>0);return root to id
    }
    private fun add(id:Long,objectId:Int,effect:String) {
        val packages=data(NativeBridge.plugin(id,"{\"op\":\"catalogue\"}")).getJSONArray("packages")
        val pkg=(0 until packages.length()).map{packages.getJSONObject(it)}.first{it.getJSONObject("manifest").getString("id")=="com.motionstudio.effects.ae2021"&&it.getJSONObject("manifest").getString("version")=="2.0.0"}
        val m=pkg.getJSONObject("manifest")
        data(NativeBridge.plugin(id,JSONObject().put("op","add").put("object",objectId).put("effect",effect).put("plugin",m.getString("id")).put("version",m.getString("version")).put("hash",pkg.getString("hash")).toString()))
    }
    @Test fun systemAndImportedFontsShareAtlasAndTextResources() {
        val (_,id)=fixture()
        try {
            val system=JSONObject(NativeBridge.systemFonts()).getJSONArray("fonts");assertTrue(system.length()>0)
            val font=(0 until system.length()).map{system.getJSONObject(it)}.first{it.getJSONArray("axes").length()==0&&it.getLong("bytes")<=32L*1024*1024&&it.getString("name").contains("Roboto",true)}
            val imported=data(NativeBridge.plugin(id,JSONObject().put("op","font_import").put("path",font.getString("path")).put("face_index",font.getInt("face_index")).toString())).getJSONObject("font")
            add(id,1,"ascii")
            val atlas=data(NativeBridge.plugin(id,JSONObject().put("op","font_atlas").put("font",imported.getString("id")).put("size",32).put("characters"," .#@").put("object",1).put("instance",1).toString()))
            assertEquals(4,atlas.getJSONObject("layout").getString("characters").length)
            val text=data(NativeBridge.plugin(id,JSONObject().put("op","font_text").put("font",imported.getString("id")).put("size",32).put("text","Motion\nStudio").put("width",160).toString()))
            assertTrue(text.getJSONObject("asset").getInt("height")>32)
            val project=data(NativeBridge.state(id)).getJSONObject("project");assertTrue(project.getJSONArray("fonts").length()>0)
            assertEquals("asset",project.getJSONArray("layers").getJSONObject(0).getJSONArray("effects").getJSONObject(0).getJSONObject("image_input").getString("kind"))
            data(NativeBridge.save(id))
            val info=data(NativeBridge.renderPlanInfo(id));val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
            assertTrue(NativeBridge.sampleRenderPlanInto(id,0,plan)>0)
            val gpu=EglMovieRenderer(null,64,64,project,id,info);try{gpu.draw(plan);gpu.readPixelsInto(ByteBuffer.allocateDirect(64*64*4))}finally{gpu.close()}
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun hiddenEffectedLayerMaterialEvaluatesInGlesWithoutFeedback() {
        val (_,id)=fixture()
        try {
            val p=data(NativeBridge.state(id)).getJSONObject("project");val source=JSONObject(p.getJSONArray("layers").getJSONObject(0).toString()).put("id",2).put("visible",false)
            data(NativeBridge.command(id,JSONObject().put("op","add").put("layer",source).toString()))
            add(id,2,"noise_generator");add(id,1,"displacement_map")
            data(NativeBridge.plugin(id,JSONObject().put("op","image_input").put("object",1).put("instance",1).put("input",JSONObject().put("kind","layer").put("layer",2)).toString()))
            val project=data(NativeBridge.state(id)).getJSONObject("project")
            val info=data(NativeBridge.renderPlanInfo(id));val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder());val gpu=EglMovieRenderer(null,64,64,project,id,info)
            fun render(frame:Int):ByteArray {plan.clear();assertTrue(NativeBridge.sampleRenderPlanInto(id,frame,plan)>0);gpu.draw(plan);val pixels=ByteBuffer.allocateDirect(64*64*4);gpu.readPixelsInto(pixels);return ByteArray(pixels.capacity()).also{pixels.get(it)}}
            try {val first=render(7);render(28);assertArrayEquals(first,render(7))}finally{gpu.close()}
            add(id,2,"displacement_map")
            val cycle=JSONObject(NativeBridge.plugin(id,JSONObject().put("op","image_input").put("object",2).put("instance",2).put("input",JSONObject().put("kind","layer").put("layer",1)).toString()))
            assertFalse(cycle.getBoolean("ok"))
            assertTrue(cycle.toString(),cycle.toString().contains("cycle"))
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun all36ShadersExecuteOnTheGlesDriverAndRandomSeekIsStable() {
        val (_,id)=fixture()
        try {
            val packages=data(NativeBridge.plugin(id,"{\"op\":\"catalogue\"}")).getJSONArray("packages")
            val pack=(0 until packages.length()).map{packages.getJSONObject(it)}.first{it.getJSONObject("manifest").getString("id")=="com.motionstudio.effects.ae2021"&&it.getJSONObject("manifest").getString("version")=="2.0.0"}
            val all=pack.getJSONObject("manifest").getJSONArray("effects");val effects=JSONArray((0 until all.length()).map{all.getJSONObject(it)}.filter{it.optString("compatibility_profile")=="motion-native-v1"});assertEquals(36,effects.length())
            for(index in 0 until effects.length()) {
                val name=effects.getJSONObject(index).getString("id");add(id,1,name)
                val project=data(NativeBridge.state(id)).getJSONObject("project");val info=data(NativeBridge.renderPlanInfo(id));val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder());val gpu=EglMovieRenderer(null,64,64,project,id,info)
                fun render(frame:Int):ByteArray {plan.clear();assertTrue("$name frame $frame",NativeBridge.sampleRenderPlanInto(id,frame,plan)>0);gpu.draw(plan);val pixels=ByteBuffer.allocateDirect(64*64*4);gpu.readPixelsInto(pixels);return ByteArray(pixels.capacity()).also{pixels.get(it)}}
                try{val first=render(7);render(31);assertArrayEquals("$name depends on playback history",first,render(7))}finally{gpu.close()}
                data(NativeBridge.command(id,JSONObject().put("op","effect").put("object",1).put("action",JSONObject().put("kind","remove").put("effect",1)).toString()))
            }
        }finally{NativeBridge.destroy(id)}
    }

    @Test fun generatedFontAtlasRefreshesAnAlreadyAttachedPreview() {
        val (_,id)=fixture()
        val consumer=HandlerThread("font-preview-consumer").apply{start()}
        val reader=ImageReader.newInstance(64,64,ImageFormat.PRIVATE,3,HardwareBuffer.USAGE_GPU_SAMPLED_IMAGE)
        reader.setOnImageAvailableListener({it.acquireLatestImage()?.close()},Handler(consumer.looper))
        fun render() {
            val deadline=SystemClock.elapsedRealtime()+5000
            while(!NativeBridge.render(id,0.0)) {
                assertTrue(NativeBridge.state(id),SystemClock.elapsedRealtime()<deadline)
                Thread.sleep(10)
            }
        }
        try {
            data(NativeBridge.surface(id,reader.surface,64,64));render()
            add(id,1,"ascii")
            data(NativeBridge.plugin(id,"""{"op":"font_atlas","size":32,"characters":" .#@","object":1,"instance":1}"""))
            render()
            assertFalse(NativeBridge.state(id).contains("is not registered"))
        }finally{
            data(NativeBridge.surface(id,null,0,0));NativeBridge.destroy(id)
            reader.setOnImageAvailableListener(null,null);consumer.quitSafely();consumer.join(3000);reader.close()
        }
    }

    @Test fun copiedMaterialGraphBindsToCopiedSourcesAndMissingExternalSourcesRejectPaste() {
        val (root,id)=fixture()
        try {
            val p=data(NativeBridge.state(id)).getJSONObject("project")
            val source=JSONObject(p.getJSONArray("layers").getJSONObject(0).toString()).put("id",2).put("visible",false)
            data(NativeBridge.command(id,JSONObject().put("op","add").put("layer",source).toString()))
            add(id,1,"displacement_map")
            data(NativeBridge.plugin(id,"""{"op":"image_input","object":1,"instance":1,"input":{"kind":"layer","layer":2}}"""))
            val before=data(NativeBridge.state(id)).getJSONObject("project")
            val clip=LayerClipboard.capture(root,before,setOf(1L,2L))!!
            val external=LayerClipboard.capture(root,before,setOf(1L))!!
            val paste=clip.plan(before,0)!!
            data(NativeBridge.command(id,paste.commands.toString()))
            val copied=data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").objects().first{it.getLong("id")==3L}
            assertEquals(4L,copied.getJSONArray("effects").getJSONObject(0).getJSONObject("image_input").getLong("layer"))
            data(NativeBridge.command(id,"""{"op":"remove","object":2,"frame":0}"""))
            val after=data(NativeBridge.state(id)).getJSONObject("project")
            assertFalse("A stale external material cannot be silently reused",external.available(after))
            assertNull(external.plan(after,0))
        }finally{NativeBridge.destroy(id)}
    }

    @Test fun copyingMutualSourceStageInputsIsAtomicAndKeepsTheCopiedGraph() {
        val (root,id)=fixture()
        try {
            val p=data(NativeBridge.state(id)).getJSONObject("project")
            val source=JSONObject(p.getJSONArray("layers").getJSONObject(0).toString()).put("id",2)
            data(NativeBridge.command(id,JSONObject().put("op","add").put("layer",source).toString()))
            for(objectId in listOf(1,2)) {
                add(id,objectId,"displacement_map")
                data(NativeBridge.plugin(id,JSONObject().put("op","image_input").put("object",objectId).put("instance",1)
                    .put("input",JSONObject().put("kind","layer").put("stage","source").put("layer",3-objectId)).toString()))
            }
            val before=data(NativeBridge.state(id)).getJSONObject("project")
            val paste=LayerClipboard.capture(root,before,setOf(1L,2L))!!.plan(before,0)!!
            data(NativeBridge.command(id,paste.commands.toString()))
            val copies=data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").objects().associateBy{it.getLong("id")}
            for((objectId,sourceId) in listOf(3L to 4L,4L to 3L)) {
                val input=copies.getValue(objectId).getJSONArray("effects").getJSONObject(0).getJSONObject("image_input")
                assertEquals(sourceId,input.getLong("layer"));assertEquals("source",input.getString("stage"))
            }
            data(NativeBridge.history(id,0))
            assertEquals("Paste must be undone in one step",before.toString(),data(NativeBridge.state(id)).getJSONObject("project").toString())
            data(NativeBridge.history(id,1))
            assertEquals(4,data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").length())
        }finally{NativeBridge.destroy(id)}
    }
}
