package com.motionstudio.editor

import android.graphics.BitmapFactory
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

class ColorCurvesApiTest {
    private val context get()=InstrumentationRegistry.getInstrumentation().targetContext
    private fun identity()=JSONArray().put(JSONArray(listOf(0,0))).put(JSONArray(listOf(1,1)))
    private fun curve()=JSONObject().put("channels",JSONArray(List(5){identity()}))
        .put("interpolation",JSONArray(List(5){"natural_cubic"}))

    @Test fun nativeGraphExposesFiveIndependentTransfersAndLegacyModeIsStable() {
        val value=curve();value.getJSONArray("channels").put(0,JSONArray().put(JSONArray(listOf(0,0))).put(JSONArray(listOf(.5,1))).put(JSONArray(listOf(1,0))))
        value.getJSONArray("channels").put(4,JSONArray().put(JSONArray(listOf(0,0))).put(JSONArray(listOf(1,.5))))
        val smooth=nativeData(NativeBridge.colorCurveGraph(value.toString()))
        assertEquals(5,smooth.getJSONArray("channels").length());assertEquals("Alpha",smooth.getJSONArray("channels").getJSONObject(4).getString("name"))
        val x=64.0/255;assertEquals(3*x-4*x*x*x,smooth.getJSONArray("outputLut").getJSONArray(64).getDouble(0),.000001)
        assertEquals(.5,smooth.getJSONArray("outputLut").getJSONArray(255).getDouble(3),.000001)
        value.remove("interpolation");val legacy=nativeData(NativeBridge.colorCurveGraph(value.toString()))
        assertEquals(2*x,legacy.getJSONArray("outputLut").getJSONArray(64).getDouble(0),.000001)
        value.getJSONArray("channels").getJSONArray(0).put(1,JSONArray(listOf(0,1)))
        assertFalse(JSONObject(NativeBridge.colorCurveGraph(value.toString())).getBoolean("ok"))
    }

    @Test fun alphaCurveAffectsRealGpuOutputAndFrozenGlesUsesSameLut() {
        val root=File(context.filesDir,"acceptance/color-api-${UUID.randomUUID()}").apply{mkdirs()}
        val p=nativeData(NativeBridge.projectTemplate(0)).put("width",64).put("height",64).put("frames",12).put("background",JSONArray(listOf(0,0,0,0)))
        p.getJSONObject("camera").put("created",false)
        val layer=p.getJSONArray("layers").getJSONObject(0).put("id",1).put("size",JSONArray(listOf(64,64))).put("three_d",false).put("parent",JSONObject.NULL)
        layer.put("content",JSONObject().put("kind","solid").put("color",JSONArray(listOf(.25,.5,.75,1))))
        layer.getJSONObject("transform").getJSONObject("position").put("value",JSONArray(listOf(32,32,0))).put("keys",JSONArray())
        p.put("layers",JSONArray().put(layer))
        val id=NativeBridge.create(root.absolutePath,p.toString());assertTrue(NativeBridge.creationError(),id>0)
        try {
            val pkg=nativeData(NativeBridge.plugin(id,"{\"op\":\"catalogue\"}")).getJSONArray("packages").objects()
                .first{it.getJSONObject("manifest").getJSONArray("effects").objects().any{e->e.getString("id")=="curves"}}
            val m=pkg.getJSONObject("manifest")
            nativeData(NativeBridge.plugin(id,JSONObject().put("op","add").put("object",1).put("effect","curves").put("plugin",m.getString("id"))
                .put("version",m.getString("version")).put("hash",pkg.getString("hash")).toString()))
            var project=nativeData(NativeBridge.state(id)).getJSONObject("project")
            val params=project.getJSONArray("layers").getJSONObject(0).getJSONArray("effects").getJSONObject(0).getJSONObject("params")
            val param=params.keys().asSequence().first{params.getJSONObject(it).getString("kind")=="curve"}
            val c=curve();c.getJSONArray("channels").put(4,JSONArray().put(JSONArray(listOf(0,0))).put(JSONArray(listOf(1,.5))))
            nativeData(NativeBridge.command(id,JSONObject().put("op","effect").put("object",1).put("action",JSONObject().put("kind","set_curve_object")
                .put("effect",1).put("param",param).put("frame",0).put("value",c)).toString()))
            val capture=nativeData(NativeBridge.capture(id));val bitmap=BitmapFactory.decodeFile(capture.getString("path"));val pixel=bitmap.getPixel(32,32);bitmap.recycle()
            assertTrue(abs((pixel ushr 24)-128)<=1)
            assertTrue(abs((pixel shr 16 and 255)-64)<=3);assertTrue(abs((pixel shr 8 and 255)-128)<=3);assertTrue(abs((pixel and 255)-191)<=3)
            project=nativeData(NativeBridge.state(id)).getJSONObject("project")
            val info=nativeData(NativeBridge.renderPlanInfo(id));val plan=ByteBuffer.allocateDirect(info.getInt("bufferBytes")).order(ByteOrder.nativeOrder())
            assertTrue(NativeBridge.sampleRenderPlanInto(id,0,plan)>0)
            val gpu=EglMovieRenderer(null,64,64,project,id,info)
            try {
                gpu.draw(plan);val gl=ByteBuffer.allocateDirect(64*64*4);gpu.readPixelsInto(gl);val alpha=gl.get((32*64+32)*4+3).toInt() and 255
                assertTrue(abs(alpha-128)<=1)
                val lutOffset=plan.getInt(44)
                assertEquals(128,plan.get(lutOffset+255*4+3).toInt() and 255)
                File(root,"alpha-output-report.json").writeText(JSONObject().put("nativeArgb",pixel).put("glesAlpha",alpha).put("lutAlpha",128).toString())
            } finally {gpu.close()}
        } finally {NativeBridge.destroy(id)}
    }
}
