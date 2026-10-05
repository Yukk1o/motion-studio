package com.motionstudio.editor

import android.graphics.BitmapFactory
import android.graphics.Color
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.UUID

// Test-owned declarations exercise the backend without changing the frontend bridge.
object GeometryBridge {
    init { System.loadLibrary("motion_engine") }
    @JvmStatic external fun sampleGeometryInto(id:Long,frame:Double,parameters:ByteBuffer,vertices:ByteBuffer):String
    @JvmStatic external fun hitCandidates(id:Long,x:Double,y:Double):String
}
@RunWith(AndroidJUnit4::class)
class Layer3dApiTest {
    private fun data(raw:String):JSONObject {val r=JSONObject(raw);assertTrue(r.optString("error"),r.getBoolean("ok"));return r.getJSONObject("data")}
    private fun track(v:Any)=JSONObject().put("value",v).put("keys",JSONArray())
    private fun v(vararg n:Number)=JSONArray(n.toList())
    private fun root()=File(InstrumentationRegistry.getInstrumentation().targetContext.filesDir,"acceptance/layer-3d-"+UUID.randomUUID()).apply{mkdirs()}
    private fun layer(id:Int,angle:Int,color:JSONArray)=JSONObject().put("id",id).put("name","cross-$id").put("size",v(256,256))
        .put("visible",true).put("locked",false).put("content",JSONObject().put("kind","solid").put("color",color))
        .put("transform",JSONObject().put("position",track(v(128,128,0))).put("rotation",track(v(0,angle,0)))
            .put("scale",track(v(100,100,100))).put("opacity",track(1)).put("anchor",v(.5,.5)))
    private fun fixture()=data(NativeBridge.projectTemplate(0)).put("version",3).put("width",256).put("height",256)
        .put("background",v(0,0,0,1)).put("assets",JSONArray()).put("layers",JSONArray().put(layer(1,45,v(1,0,0,1))).put(layer(2,-45,v(0,0,1,1))))
        .apply{getJSONObject("camera").put("created",false).remove("parent")}
    private fun command(id:Long,op:String,vararg fields:Pair<String,Any>)=data(NativeBridge.command(id,JSONObject().put("op",op).apply{fields.forEach{put(it.first,it.second)}}.toString()))
    @Test fun defaultsExplicitSwitchHistoryAndPersistenceDoNotRewriteTransformTracks() {
        val root=root();val id=NativeBridge.create(root.absolutePath,fixture().toString());assertTrue(id>0)
        try {
            val original=data(NativeBridge.state(id));val p=original.getJSONObject("project");
            assertFalse(p.getJSONArray("layers").getJSONObject(0).getBoolean("three_d"));
            assertFalse(original.getJSONObject("capabilities").getJSONObject("layer_3d").getBoolean("default"))
            val tracks=p.getJSONArray("layers").getJSONObject(0).getJSONObject("transform").toString()
            val changed=command(id,"set_layer_3d","object" to 1,"enabled" to true)
            assertEquals(tracks,changed.getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONObject("transform").toString())
            data(NativeBridge.history(id,0));assertFalse(data(NativeBridge.state(id)).getJSONObject("project").getJSONArray("layers").getJSONObject(0).getBoolean("three_d"))
            data(NativeBridge.history(id,1));data(NativeBridge.save(id))
            File(root,"switch-response.json").writeText(changed.toString(2))
        }finally{NativeBridge.destroy(id)}
        val reopened=NativeBridge.create(root.absolutePath,"");assertTrue(reopened>0)
        try{assertTrue(data(NativeBridge.state(reopened)).getJSONObject("project").getJSONArray("layers").getJSONObject(0).getBoolean("three_d"))}finally{NativeBridge.destroy(reopened)}
    }
    @Test fun intersectingOpaqueAndTransparentPlanesAgreeWithGeometryAndPickingApis() {
        val root=root();val id=NativeBridge.create(root.absolutePath,fixture().toString());assertTrue(id>0)
        try {
            for(objectId in listOf(1,2))command(id,"set_layer_3d","object" to objectId,"enabled" to true)
            val parameters=ByteBuffer.allocateDirect(8192*128).order(ByteOrder.nativeOrder())
            val vertices=ByteBuffer.allocateDirect(65536*20).order(ByteOrder.nativeOrder())
            val geometry=data(GeometryBridge.sampleGeometryInto(id,20.5,parameters,vertices))
            assertEquals(3,geometry.getInt("batches"));assertEquals(18,geometry.getInt("vertices"))
            assertEquals(384,geometry.getInt("parameterBytes"));assertEquals(360,geometry.getInt("vertexBytes"))
            var vertexCount=0
            for(i in 0..2){assertEquals(vertexCount,parameters.getFloat(i*128+25*4).toInt());vertexCount+=parameters.getFloat(i*128+26*4).toInt()}
            assertEquals(18,vertexCount);assertTrue((0 until 18).any{k->kotlin.math.abs(vertices.getFloat(k*20+12)-.5f)<.0001f})
            assertEquals(2,data(GeometryBridge.hitCandidates(id,96.0,128.0)).getJSONArray("candidates").getJSONObject(0).getInt("id"))
            assertEquals(1,data(GeometryBridge.hitCandidates(id,160.0,128.0)).getJSONArray("candidates").getJSONObject(0).getInt("id"))
            assertFalse(JSONObject(GeometryBridge.sampleGeometryInto(id,21.0,ByteBuffer.allocateDirect(1),vertices)).getBoolean("ok"))
            assertFalse(JSONObject(GeometryBridge.sampleGeometryInto(id,21.0,parameters,parameters)).getBoolean("ok"))
            assertFalse(JSONObject(GeometryBridge.sampleGeometryInto(id,21.0,parameters.asReadOnlyBuffer(),vertices)).getBoolean("ok"))
            assertFalse(JSONObject(GeometryBridge.sampleGeometryInto(id,21.0,ByteBuffer.allocate(1024),vertices)).getBoolean("ok"))
            assertFalse(JSONObject(GeometryBridge.sampleGeometryInto(id,Double.NaN,parameters,vertices)).getBoolean("ok"))
            fun probes(name:String,opaque:Boolean) {
                val file=File(data(NativeBridge.capture(id)).getString("path"));file.copyTo(File(root,"$name.png"),true)
                val bitmap=BitmapFactory.decodeFile(file.absolutePath)
                try {val left=bitmap.getPixel(96,128);val right=bitmap.getPixel(160,128)
                    assertTrue(Color.blue(left)>Color.red(left));assertTrue(Color.red(right)>Color.blue(right))
                    if(opaque){assertTrue(Color.blue(left)>250&&Color.red(left)<3);assertTrue(Color.red(right)>250&&Color.blue(right)<3)}
                }finally{bitmap.recycle()}
            }
            probes("opaque",true)
            command(id,"set_scalar","object" to 1,"property" to "opacity","frame" to 0,"value" to .5)
            command(id,"set_scalar","object" to 2,"property" to "opacity","frame" to 0,"value" to .5)
            probes("transparent",false)
            val frozen=data(NativeBridge.state(id)).getJSONObject("project").toString()
            val frozenId=NativeBridge.create(root.absolutePath,frozen);assertTrue(frozenId>0)
            try {
                data(GeometryBridge.sampleGeometryInto(frozenId,35.5,parameters,vertices))
                val expected=ByteArray(384);parameters.position(0);parameters.get(expected)
                command(id,"set_vector","object" to 1,"property" to "position","frame" to 0,"value" to v(80,128,0))
                data(GeometryBridge.sampleGeometryInto(frozenId,35.5,parameters,vertices))
                val actual=ByteArray(384);parameters.position(0);parameters.get(actual);assertArrayEquals(expected,actual)
            }finally{NativeBridge.destroy(frozenId)}
            File(root,"geometry-response.json").writeText(geometry.toString(2))
        }finally{NativeBridge.destroy(id)}
    }
    @Test fun flatDragPreservesAspectAndSeparatedInactiveZAnimationUnderAMovedCamera() {
        val root=root();val id=NativeBridge.create(root.absolutePath,fixture().toString());assertTrue(id>0)
        try {
            command(id,"create_camera")
            command(id,"set_vector","object" to 0,"property" to "position","frame" to 0,"value" to v(400,400,-300))
            command(id,"set_vector","object" to 1,"property" to "scale","frame" to 0,"value" to v(120,60,100))
            command(id,"separate_dimensions","object" to 1,"property" to "position")
            command(id,"animate","object" to 1,"property" to "position","axis" to "z","frame" to 15,"enabled" to true)
            command(id,"set_component","object" to 1,"property" to "position","axis" to "z","frame" to 45,"value" to 200)
            val before=data(NativeBridge.seek(id,21.0));val transform=before.getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONObject("transform")
            val z=transform.getJSONObject("position").getJSONObject("axes").getJSONObject("z").toString()
            val scale=transform.getJSONObject("scale").toString()
            fun corners(state:JSONObject)=state.getJSONArray("projectedLayers").getJSONObject(0).getJSONArray("corners")
            val a=corners(before)
            val after=data(NativeBridge.drag(id,1,12.0,-7.0,256,256));val b=corners(after)
            for(i in 0..3){assertEquals(12.0,b.getJSONArray(i).getDouble(0)-a.getJSONArray(i).getDouble(0),.002)
                assertEquals(-7.0,b.getJSONArray(i).getDouble(1)-a.getJSONArray(i).getDouble(1),.002)}
            val changed=after.getJSONObject("project").getJSONArray("layers").getJSONObject(0).getJSONObject("transform")
            assertEquals(z,changed.getJSONObject("position").getJSONObject("axes").getJSONObject("z").toString())
            assertEquals(scale,changed.getJSONObject("scale").toString())
        }finally{NativeBridge.destroy(id)}
    }
}
