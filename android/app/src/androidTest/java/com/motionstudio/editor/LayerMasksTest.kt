package com.motionstudio.editor

import android.content.Intent
import android.graphics.BitmapFactory
import androidx.compose.ui.test.*
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.lifecycle.ViewModelProvider
import androidx.test.core.app.ActivityScenario
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Rule
import org.junit.Test
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

class LayerMasksTest {
    @get:Rule val compose=createEmptyComposeRule()
    private val context get()=InstrumentationRegistry.getInstrumentation().targetContext
    private fun project():JSONObject {
        val p=nativeData(NativeBridge.projectTemplate(0)).put("width",1080).put("height",1920).put("frames",60).put("background",JSONArray(listOf(0,0,0,0)))
        p.getJSONObject("camera").put("created",false)
        val layer=p.getJSONArray("layers").getJSONObject(0).put("id",1).put("size",JSONArray(listOf(1080,1920))).put("parent",JSONObject.NULL).put("three_d",false)
        layer.put("content",JSONObject().put("kind","solid").put("color",JSONArray(listOf(1,0,0,1))))
        layer.getJSONObject("transform").getJSONObject("position").put("value",JSONArray(listOf(540,960,0))).put("keys",JSONArray())
        return p.put("layers",JSONArray().put(layer))
    }
    private fun root()=File(context.filesDir,"acceptance/masks-${UUID.randomUUID()}").apply{mkdirs()}
    private fun mask():JSONObject {
        val nodes=listOf(200 to 300,800 to 300,800 to 1500,200 to 1500).mapIndexed{i,p->JSONObject().put("id",i+1).put("geometry",JSONObject().put("value",JSONArray(listOf(p.first,p.second,0,0,0,0))).put("keys",JSONArray()))}
        return JSONObject().put("id",1).put("name","蒙版 1").put("path",JSONObject().put("id",1).put("closed",true).put("nodes",JSONArray(nodes)))
    }
    @Test fun nativeAndGlesUseSameMaskCoverageAndFrozenRecords() {
        val dir=root();val id=NativeBridge.create(dir.absolutePath,project().toString());assertTrue(NativeBridge.creationError(),id>0)
        try {
            nativeData(NativeBridge.command(id,JSONObject().put("op","mask").put("object",1).put("action",JSONObject().put("kind","add").put("mask",mask())).toString()))
            val png=nativeData(NativeBridge.capture(id));val bitmap=BitmapFactory.decodeFile(png.getString("path"))
            assertEquals(255,bitmap.getPixel(500,900) ushr 24);assertEquals(0,bitmap.getPixel(100,900) ushr 24);bitmap.recycle()
            val p=nativeData(NativeBridge.state(id)).getJSONObject("project")
            val info=nativeData(NativeBridge.renderPlanInfo(id));assertEquals(5,info.getInt("version"));assertEquals(128,info.getInt("headerBytes"));assertEquals(2,info.getJSONArray("maskPrograms").length())
            val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder());assertTrue(NativeBridge.sampleRenderPlanInto(id,0,plan)>0)
            assertEquals(1,plan.getInt(116))
            val gpu=EglMovieRenderer(null,1080,1920,p,id,info)
            try {gpu.draw(plan);val pixels=ByteBuffer.allocateDirect(1080*1920*4);gpu.readPixelsInto(pixels);assertEquals(255,pixels.get((900*1080+500)*4+3).toInt() and 255);assertEquals(0,pixels.get((900*1080+100)*4+3).toInt() and 255)}finally{gpu.close()}
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun featherSubtractAndBlurAgreeAcrossNativeAndGles() {
        val dir=root();val id=NativeBridge.create(dir.absolutePath,project().toString());assertTrue(id>0)
        try {
            val first=mask().put("feather",JSONObject().put("value",JSONArray(listOf(24,40))).put("keys",JSONArray()))
            nativeData(NativeBridge.command(id,JSONObject().put("op","mask").put("object",1).put("action",JSONObject().put("kind","add").put("mask",first)).toString()))
            val cut=mask().put("id",2).put("mode","subtract").put("opacity",JSONObject().put("value",50).put("keys",JSONArray()))
            nativeData(NativeBridge.command(id,JSONObject().put("op","mask").put("object",1).put("action",JSONObject().put("kind","add").put("mask",cut)).toString()))
            val pkg=nativeData(NativeBridge.plugin(id,"{\"op\":\"catalogue\"}")).getJSONArray("packages").objects().first{it.getJSONObject("manifest").getJSONArray("effects").objects().any{e->e.getString("id")=="gaussian_blur"}}
            val manifest=pkg.getJSONObject("manifest")
            nativeData(NativeBridge.plugin(id,JSONObject().put("op","add").put("object",1).put("effect","gaussian_blur").put("plugin",manifest.getString("id")).put("version",manifest.getString("version")).put("hash",pkg.getString("hash")).toString()))
            nativeData(NativeBridge.command(id,JSONObject().put("op","effect").put("object",1).put("action",JSONObject().put("kind","set").put("effect",1).put("param","p0001").put("frame",0).put("value",JSONArray(listOf(20,0,0,0)))).toString()))
            val png=nativeData(NativeBridge.capture(id));val bitmap=BitmapFactory.decodeFile(png.getString("path"))
            val info=nativeData(NativeBridge.renderPlanInfo(id));val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder());assertTrue(NativeBridge.sampleRenderPlanInto(id,0,plan)>0)
            val gpu=EglMovieRenderer(null,1080,1920,nativeData(NativeBridge.state(id)).getJSONObject("project"),id,info)
            try {
                gpu.draw(plan);val pixels=ByteBuffer.allocateDirect(1080*1920*4);gpu.readPixelsInto(pixels)
                for(y in listOf(290,300,310,900,1490,1500,1510))for(x in listOf(190,200,210,500,790,800,810)) {
                    val expected=bitmap.getPixel(x,y) ushr 24;val actual=pixels.get(((1919-y)*1080+x)*4+3).toInt() and 255
                    assertTrue("($x,$y): native=$expected GLES=$actual",kotlin.math.abs(expected-actual)<=3)
                }
                assertTrue((bitmap.getPixel(190,900) ushr 24)>0)
            }finally{gpu.close();bitmap.recycle()}
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun nativePanelCreatesMaskAnimatesOpacityAndUndoRestores() {
        val dir=root();val id=NativeBridge.create(dir.absolutePath,project().toString());assertTrue(id>0);nativeData(NativeBridge.save(id));NativeBridge.destroy(id)
        var vm:EditorViewModel?=null
        ActivityScenario.launch<AcceptanceActivity>(Intent(context,AcceptanceActivity::class.java).putExtra("projectDirectory",dir.absolutePath)).use{scenario->
            scenario.onActivity{vm=ViewModelProvider(it)[EditorViewModel::class.java]}
            compose.waitUntil(20000){vm?.state?.project!=null}
            scenario.onActivity{vm!!.select(1);vm!!.openMasks()}
            compose.onNodeWithTag("mask-add-rectangle").performClick()
            compose.waitUntil(10000){vm!!.masks().size==1};assertNull(vm!!.state.error)
            compose.onNodeWithTag("mask-panel").assertIsDisplayed();compose.onNodeWithTag("vector-preview-overlay").assertIsDisplayed()
            scenario.onActivity{vm!!.setPropertyValue(1,"mask:1:opacity",0,60);vm!!.maskAction(JSONObject().put("kind","options").put("mask",1).put("inverted",true))}
            compose.waitUntil(10000){vm!!.maskData()?.getBoolean("inverted")==true}
            assertEquals(60.0,vm!!.maskData()!!.getJSONObject("opacity").getDouble("value"),0.001)
            scenario.onActivity{vm!!.undo();vm!!.undo()}
            compose.waitUntil(10000){vm!!.maskData()!!.getJSONObject("opacity").getDouble("value")==100.0}
            scenario.onActivity{vm!!.vectorPathId=2;vm!!.chooseMask(1)}
            assertEquals(1L,vm!!.vectorPathId)
            scenario.onActivity{vm!!.property="mask:1:opacity";vm!!.addKey()}
            compose.waitUntil(10000){vm!!.maskTrackRaw(1,"mask:1:opacity")!!.getJSONArray("keys").length()==1}
            scenario.onActivity{vm!!.seek(30.0);vm!!.setPropertyValue(1,"mask:1:opacity",30,50)}
            compose.waitUntil(10000){vm!!.maskTrackRaw(1,"mask:1:opacity")!!.getJSONArray("keys").length()==2};assertNull(vm!!.state.error)
            scenario.onActivity{vm!!.seek(15.0)}
            compose.waitUntil(10000){(vm!!.maskValue(1,"mask:1:opacity") as? Number)?.toDouble()==75.0}
            compose.waitForIdle();val shot=InstrumentationRegistry.getInstrumentation().uiAutomation.takeScreenshot()
            File(dir,"mask-panel.png").outputStream().use{shot.compress(android.graphics.Bitmap.CompressFormat.PNG,100,it)};shot.recycle()
            scenario.onActivity{vm!!.undo()}
            compose.waitUntil(10000){vm!!.maskTrackRaw(1,"mask:1:opacity")!!.getJSONArray("keys").length()==1};assertNull(vm!!.state.error)
        }
    }
}
